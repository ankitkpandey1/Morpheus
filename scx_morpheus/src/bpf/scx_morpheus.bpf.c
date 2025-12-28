// SPDX-License-Identifier: GPL-2.0
/*
 * scx_morpheus.bpf.c - sched_ext BPF scheduler for Morpheus-Hybrid
 *
 * This scheduler implements the kernel side of the Morpheus-Hybrid protocol:
 * - Tracks worker thread budgets
 * - Emits yield hints via ring buffer
 * - Enforces gated escalation for unresponsive workers
 *
 * Key design principles:
 * - Language-neutral: operates on worker threads, not async tasks
 * - Cooperative by default: only escalates when explicitly permitted
 * - Safe: respects critical sections and escapability flags
 *
 * ARCHITECTURAL GUARDRAILS (Non-Goals):
 * - Per-task kernel scheduling: operates on worker threads only
 * - Bytecode-level preemption: safe points controlled by language runtime
 * - Kernel-managed budgets: budgets are advisory only
 *
 * Copyright (C) 2024 Ankit Kumar Pandey <ankitkpandey1@gmail.com>
 */

/* Include vmlinux.h first - provides all kernel types */
#include "vmlinux.h"

/* Include our sched_ext compatibility header */
#include "compat.bpf.h"

/* Error codes */
#ifndef ENOMEM
#define ENOMEM 12
#endif

/* Include shared types between kernel and userspace */
#include "morpheus_shared.h"

char _license[] SEC("license") = "GPL";

/*
 * ============================================================================
 * Configuration - set from userspace before loading
 * ============================================================================
 */
const volatile u64 slice_ns = MORPHEUS_DEFAULT_SLICE_NS;
const volatile u64 grace_period_ns = MORPHEUS_GRACE_PERIOD_NS;
const volatile u32 max_workers = MORPHEUS_MAX_WORKERS;
const volatile bool debug_mode = false;

/* Delta #1: Observer vs Enforcer mode */
const volatile u32 scheduler_mode = MORPHEUS_MODE_OBSERVER_ONLY;

/*
 * ============================================================================
 * Statistics
 * ============================================================================
 */
struct morpheus_stats {
    u64 hints_emitted;
    u64 hints_dropped;
    u64 escalations;
    u64 escalations_blocked;
    u64 ticks_total;
    u64 state_checks_skipped;  /* Hints skipped due to worker state */
};

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __type(key, u32);
    __type(value, struct morpheus_stats);
    __uint(max_entries, 1);
} stats_map SEC(".maps");

/*
 * ============================================================================
 * SCB Map - Shared Control Blocks, one per worker
 * ============================================================================
 */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __type(key, u32);
    __type(value, struct morpheus_scb);
    __uint(max_entries, MORPHEUS_MAX_WORKERS);
    __uint(map_flags, BPF_F_MMAPABLE);
} scb_map SEC(".maps");

/*
 * Worker TID Map - Maps OS thread ID to worker_id
 */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, u32);  /* pid (TID) */
    __type(value, u32); /* worker_id */
    __uint(max_entries, MORPHEUS_MAX_WORKERS);
} worker_tid_map SEC(".maps");

/*
 * ============================================================================
 * Delta #4: Global Pressure Aggregator
 * ============================================================================
 */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __type(key, u32);
    __type(value, struct morpheus_global_pressure);
    __uint(max_entries, 1);
} global_pressure_map SEC(".maps");

/*
 * Hint Ring Buffer - Kernel → Userspace events
 */
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, MORPHEUS_RINGBUF_SIZE);
} hint_ringbuf SEC(".maps");

/*
 * Per-task state for tracking runtime
 */
struct task_ctx {
    u64 last_tick_ns;
    u64 runtime_ns;
    u32 worker_id;
    bool is_morpheus_worker;
};

struct {
    __uint(type, BPF_MAP_TYPE_TASK_STORAGE);
    __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int);
    __type(value, struct task_ctx);
} task_ctx_map SEC(".maps");

/*
 * Dispatch queue
 */
#define MORPHEUS_DSQ_ID 0

/* User exit info for graceful shutdown */
UEI_DEFINE(uei);

/*
 * ============================================================================
 * Helpers
 * ============================================================================
 */

static __always_inline struct morpheus_stats *get_stats(void)
{
    u32 key = 0;
    return bpf_map_lookup_elem(&stats_map, &key);
}

static __always_inline struct morpheus_scb *get_scb(u32 worker_id)
{
    if (worker_id >= max_workers)
        return NULL;
    return bpf_map_lookup_elem(&scb_map, &worker_id);
}

static __always_inline struct task_ctx *get_task_ctx(struct task_struct *p)
{
    return bpf_task_storage_get(&task_ctx_map, p, NULL, 0);
}

/*
 * Delta #2: Check if worker state allows hints
 */
static __always_inline bool worker_can_receive_hints(u32 state)
{
    return state == MORPHEUS_WORKER_STATE_RUNNING;
}

/*
 * Delta #2: Check if worker state allows escalation
 */
static __always_inline bool worker_can_escalate(u32 state)
{
    return state == MORPHEUS_WORKER_STATE_RUNNING;
}

/*
 * Emit a hint to the ring buffer
 */
static __always_inline void emit_hint(u32 worker_id, u64 seq, u32 reason,
                                       u32 tid, u64 deadline_ns)
{
    struct morpheus_hint *hint;
    struct morpheus_stats *stats = get_stats();

    hint = bpf_ringbuf_reserve(&hint_ringbuf, sizeof(*hint), 0);
    if (!hint) {
        if (stats)
            __sync_fetch_and_add(&stats->hints_dropped, 1);
        return;
    }

    hint->seq = seq;
    hint->reason = reason;
    hint->target_tid = tid;
    hint->deadline_ns = deadline_ns;

    bpf_ringbuf_submit(hint, 0);

    if (stats)
        __sync_fetch_and_add(&stats->hints_emitted, 1);
}

/*
 * Delta #3: Execute escalation based on policy
 */
static __always_inline void execute_escalation(struct task_struct *p,
                                                u32 policy,
                                                struct morpheus_stats *stats)
{
    switch (policy) {
    case MORPHEUS_ESCALATION_NONE:
        /* Observer mode - no enforcement */
        break;
    case MORPHEUS_ESCALATION_THREAD_KICK:
        scx_bpf_kick_cpu(scx_bpf_task_cpu(p), SCX_KICK_PREEMPT);
        if (stats)
            __sync_fetch_and_add(&stats->escalations, 1);
        break;
    case MORPHEUS_ESCALATION_CGROUP_THROTTLE:
        /* Cgroup throttling would be implemented here */
        /* For now, fall through to kick */
        scx_bpf_kick_cpu(scx_bpf_task_cpu(p), SCX_KICK_PREEMPT);
        if (stats)
            __sync_fetch_and_add(&stats->escalations, 1);
        break;
    case MORPHEUS_ESCALATION_HYBRID:
        /* Most aggressive: kick + (future) throttle */
        scx_bpf_kick_cpu(scx_bpf_task_cpu(p), SCX_KICK_PREEMPT);
        if (stats)
            __sync_fetch_and_add(&stats->escalations, 1);
        break;
    }
}

/*
 * ============================================================================
 * sched_ext ops
 * ============================================================================
 */

s32 BPF_STRUCT_OPS_SLEEPABLE(morpheus_init)
{
    return scx_bpf_create_dsq(MORPHEUS_DSQ_ID, -1);
}

s32 BPF_STRUCT_OPS(morpheus_init_task, struct task_struct *p,
                   struct scx_init_task_args *args)
{
    struct task_ctx *tctx;
    u32 pid = BPF_CORE_READ(p, pid);
    u32 *worker_id_ptr;

    tctx = bpf_task_storage_get(&task_ctx_map, p,
                                NULL, BPF_LOCAL_STORAGE_GET_F_CREATE);
    if (!tctx)
        return -ENOMEM;

    tctx->last_tick_ns = 0;
    tctx->runtime_ns = 0;

    /* Check if this task is a registered Morpheus worker */
    worker_id_ptr = bpf_map_lookup_elem(&worker_tid_map, &pid);
    if (worker_id_ptr) {
        tctx->worker_id = *worker_id_ptr;
        tctx->is_morpheus_worker = true;
    } else {
        tctx->worker_id = 0;
        tctx->is_morpheus_worker = false;
    }

    return 0;
}

s32 BPF_STRUCT_OPS(morpheus_select_cpu, struct task_struct *p, s32 prev_cpu,
                   u64 wake_flags)
{
    bool is_idle = false;
    s32 cpu;

    cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
    if (is_idle)
        scx_bpf_dispatch(p, SCX_DSQ_LOCAL, slice_ns, 0);

    return cpu;
}

void BPF_STRUCT_OPS(morpheus_enqueue, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = get_task_ctx(p);

    if (tctx)
        tctx->runtime_ns = 0;

    scx_bpf_dispatch(p, MORPHEUS_DSQ_ID, slice_ns, enq_flags);
}

void BPF_STRUCT_OPS(morpheus_dispatch, s32 cpu, struct task_struct *prev)
{
    scx_bpf_consume(MORPHEUS_DSQ_ID);
}

void BPF_STRUCT_OPS(morpheus_running, struct task_struct *p)
{
    struct task_ctx *tctx = get_task_ctx(p);

    if (tctx)
        tctx->last_tick_ns = bpf_ktime_get_ns();
}

void BPF_STRUCT_OPS(morpheus_stopping, struct task_struct *p, bool runnable)
{
    struct task_ctx *tctx = get_task_ctx(p);
    struct morpheus_scb *scb;
    u64 now, delta;

    if (!tctx || !tctx->is_morpheus_worker)
        return;

    now = bpf_ktime_get_ns();
    if (tctx->last_tick_ns > 0) {
        delta = now - tctx->last_tick_ns;
        tctx->runtime_ns += delta;
    }

    scb = get_scb(tctx->worker_id);
    if (scb) {
        u64 budget = __sync_fetch_and_add(&scb->budget_remaining_ns, 0);
        if (budget > delta)
            __sync_lock_test_and_set(&scb->budget_remaining_ns, budget - delta);
        else
            __sync_lock_test_and_set(&scb->budget_remaining_ns, 0);
    }
}

/*
 * Core tick handler - implements all architectural deltas
 */
void BPF_STRUCT_OPS(morpheus_tick, struct task_struct *p)
{
    struct task_ctx *tctx = get_task_ctx(p);
    struct morpheus_scb *scb;
    struct morpheus_stats *stats = get_stats();
    u64 now, delta, preempt_seq, last_ack_seq, deadline;
    u32 worker_state, is_critical, escapable, escalation_policy, tid;

    if (stats)
        __sync_fetch_and_add(&stats->ticks_total, 1);

    if (!tctx || !tctx->is_morpheus_worker)
        return;

    now = bpf_ktime_get_ns();
    if (tctx->last_tick_ns > 0) {
        delta = now - tctx->last_tick_ns;
        tctx->runtime_ns += delta;
    }
    tctx->last_tick_ns = now;

    scb = get_scb(tctx->worker_id);
    if (!scb)
        return;

    /* Delta #2: Check worker lifecycle state */
    worker_state = __sync_fetch_and_add(&scb->worker_state, 0);
    if (!worker_can_receive_hints(worker_state)) {
        if (stats)
            __sync_fetch_and_add(&stats->state_checks_skipped, 1);
        return;
    }

    /* Check if we exceeded the slice */
    if (tctx->runtime_ns > slice_ns) {
        /* Increment preempt_seq to signal yield request */
        preempt_seq = __sync_add_and_fetch(&scb->preempt_seq, 1);

        tid = BPF_CORE_READ(p, pid);

        /* Emit hint via ring buffer */
        deadline = now + grace_period_ns;
        emit_hint(tctx->worker_id, preempt_seq, MORPHEUS_HINT_BUDGET,
                  tid, deadline);

        /* Delta #1: Only escalate if in enforced mode */
        if (scheduler_mode != MORPHEUS_MODE_ENFORCED) {
            return;
        }

        /* === Gated Escalation Check === */
        escapable = __sync_fetch_and_add(&scb->escapable, 0);
        is_critical = __sync_fetch_and_add(&scb->is_in_critical_section, 0);
        last_ack_seq = __sync_fetch_and_add(&scb->last_ack_seq, 0);
        escalation_policy = __sync_fetch_and_add(&scb->escalation_policy, 0);

        /* Delta #2: Check worker state for escalation permission */
        if (!worker_can_escalate(worker_state)) {
            if (stats)
                __sync_fetch_and_add(&stats->escalations_blocked, 1);
            return;
        }

        /*
         * Escalation conditions (ALL must be true):
         * 1. Worker has opted in (escapable == 1)
         * 2. Not in critical section
         * 3. Worker has ignored hints (last_ack_seq < preempt_seq)
         * 4. Runtime exceeds grace period
         * 5. Escalation policy is not NONE
         */
        if (escapable &&
            !is_critical &&
            last_ack_seq < preempt_seq &&
            tctx->runtime_ns > (slice_ns + grace_period_ns) &&
            escalation_policy != MORPHEUS_ESCALATION_NONE) {

            if (debug_mode)
                bpf_printk("morpheus: escalating worker %u (tid=%d, runtime=%llu, policy=%u)",
                           tctx->worker_id, tid, tctx->runtime_ns, escalation_policy);

            /* Delta #3: Execute based on policy */
            execute_escalation(p, escalation_policy, stats);
        } else if (!escapable || is_critical) {
            if (stats)
                __sync_fetch_and_add(&stats->escalations_blocked, 1);
        }
    }
}

void BPF_STRUCT_OPS(morpheus_enable, struct task_struct *p)
{
}

void BPF_STRUCT_OPS(morpheus_exit, struct scx_exit_info *ei)
{
    UEI_RECORD(uei, ei);
}

/*
 * Scheduler operations structure
 */
SCX_OPS_DEFINE(morpheus_ops,
               .select_cpu     = (void *)morpheus_select_cpu,
               .enqueue        = (void *)morpheus_enqueue,
               .dispatch       = (void *)morpheus_dispatch,
               .running        = (void *)morpheus_running,
               .stopping       = (void *)morpheus_stopping,
               .tick           = (void *)morpheus_tick,
               .init_task      = (void *)morpheus_init_task,
               .enable         = (void *)morpheus_enable,
               .init           = (void *)morpheus_init,
               .exit           = (void *)morpheus_exit,
               .name           = "morpheus");
