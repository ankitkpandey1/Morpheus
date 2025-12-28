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
 */

#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>
#include "morpheus_shared.h"

char _license[] SEC("license") = "GPL";

/*
 * Configuration - set from userspace before loading
 */
const volatile u64 slice_ns = MORPHEUS_DEFAULT_SLICE_NS;
const volatile u64 grace_period_ns = MORPHEUS_GRACE_PERIOD_NS;
const volatile u32 max_workers = MORPHEUS_MAX_WORKERS;
const volatile bool debug_mode = false;

/*
 * Statistics
 */
struct morpheus_stats {
    u64 hints_emitted;
    u64 hints_dropped;
    u64 escalations;
    u64 escalations_blocked;
    u64 ticks_total;
};

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __type(key, u32);
    __type(value, struct morpheus_stats);
    __uint(max_entries, 1);
} stats_map SEC(".maps");

/*
 * SCB Map - Shared Control Blocks, one per worker
 *
 * Key: worker_id (u32)
 * Value: struct morpheus_scb
 *
 * This map is mmap'd by userspace for fast SCB access.
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
 *
 * Populated by userspace when workers register.
 */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, u32);  /* pid (TID) */
    __type(value, u32); /* worker_id */
    __uint(max_entries, MORPHEUS_MAX_WORKERS);
} worker_tid_map SEC(".maps");

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

/*
 * Helper: get current stats
 */
static __always_inline struct morpheus_stats *get_stats(void)
{
    u32 key = 0;
    return bpf_map_lookup_elem(&stats_map, &key);
}

/*
 * Helper: look up SCB for a worker
 */
static __always_inline struct morpheus_scb *get_scb(u32 worker_id)
{
    if (worker_id >= max_workers)
        return NULL;
    return bpf_map_lookup_elem(&scb_map, &worker_id);
}

/*
 * Helper: look up worker_id for a task
 */
static __always_inline struct task_ctx *get_task_ctx(struct task_struct *p)
{
    return bpf_task_storage_get(&task_ctx_map, p, NULL, 0);
}

/*
 * Helper: emit a hint to the ring buffer
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
 * sched_ext ops: initialization
 */
s32 BPF_STRUCT_OPS_SLEEPABLE(morpheus_init)
{
    return scx_bpf_create_dsq(MORPHEUS_DSQ_ID, -1);
}

/*
 * sched_ext ops: task state initialization
 */
s32 BPF_STRUCT_OPS(morpheus_init_task, struct task_struct *p,
                   struct scx_init_task_args *args)
{
    struct task_ctx *tctx;
    u32 pid = p->pid;
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

/*
 * sched_ext ops: select CPU for a waking task
 */
s32 BPF_STRUCT_OPS(morpheus_select_cpu, struct task_struct *p, s32 prev_cpu,
                   u64 wake_flags)
{
    bool is_idle = false;
    s32 cpu;

    /* Try to find an idle CPU, preferring the previous one */
    cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
    if (is_idle)
        scx_bpf_dispatch(p, SCX_DSQ_LOCAL, slice_ns, 0);

    return cpu;
}

/*
 * sched_ext ops: enqueue a task
 */
void BPF_STRUCT_OPS(morpheus_enqueue, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = get_task_ctx(p);

    /* Reset runtime tracking on enqueue */
    if (tctx)
        tctx->runtime_ns = 0;

    scx_bpf_dispatch(p, MORPHEUS_DSQ_ID, slice_ns, enq_flags);
}

/*
 * sched_ext ops: dispatch next task
 */
void BPF_STRUCT_OPS(morpheus_dispatch, s32 cpu, struct task_struct *prev)
{
    scx_bpf_consume(MORPHEUS_DSQ_ID);
}

/*
 * sched_ext ops: running - called when a task starts running
 */
void BPF_STRUCT_OPS(morpheus_running, struct task_struct *p)
{
    struct task_ctx *tctx = get_task_ctx(p);

    if (tctx)
        tctx->last_tick_ns = bpf_ktime_get_ns();
}

/*
 * sched_ext ops: stopping - called when a task stops running
 */
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

    /* Update SCB budget */
    scb = get_scb(tctx->worker_id);
    if (scb) {
        u64 budget = __sync_load_n(&scb->budget_remaining_ns, __ATOMIC_RELAXED);
        if (budget > delta)
            __sync_store_n(&scb->budget_remaining_ns, budget - delta, __ATOMIC_RELAXED);
        else
            __sync_store_n(&scb->budget_remaining_ns, 0, __ATOMIC_RELAXED);
    }
}

/*
 * sched_ext ops: tick - called on scheduler ticks
 *
 * This is the core scheduling logic for Morpheus. On each tick:
 * 1. Track runtime for Morpheus workers
 * 2. Detect budget overruns
 * 3. Emit yield hints
 * 4. Check escalation conditions
 */
void BPF_STRUCT_OPS(morpheus_tick, struct task_struct *p)
{
    struct task_ctx *tctx = get_task_ctx(p);
    struct morpheus_scb *scb;
    struct morpheus_stats *stats = get_stats();
    u64 now, delta, preempt_seq, last_ack_seq, deadline;
    u32 is_critical, escapable;

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

    /* Check if we exceeded the slice */
    if (tctx->runtime_ns > slice_ns) {
        /* Increment preempt_seq to signal yield request */
        preempt_seq = __sync_add_and_fetch(&scb->preempt_seq, 1);

        /* Emit hint via ring buffer */
        deadline = now + grace_period_ns;
        emit_hint(tctx->worker_id, preempt_seq, MORPHEUS_HINT_BUDGET,
                  p->pid, deadline);

        /* === Gated Escalation Check === */
        escapable = __sync_load_n(&scb->escapable, __ATOMIC_ACQUIRE);
        is_critical = __sync_load_n(&scb->is_in_critical_section, __ATOMIC_ACQUIRE);
        last_ack_seq = __sync_load_n(&scb->last_ack_seq, __ATOMIC_ACQUIRE);

        /*
         * Escalation conditions (ALL must be true):
         * 1. Worker has opted in (escapable == 1)
         * 2. Not in critical section
         * 3. Worker has ignored hints (last_ack_seq < preempt_seq)
         * 4. Runtime exceeds grace period
         */
        if (escapable &&
            !is_critical &&
            last_ack_seq < preempt_seq &&
            tctx->runtime_ns > (slice_ns + grace_period_ns)) {

            if (stats)
                __sync_fetch_and_add(&stats->escalations, 1);

            if (debug_mode)
                bpf_printk("morpheus: escalating worker %u (tid=%d, runtime=%llu)",
                           tctx->worker_id, p->pid, tctx->runtime_ns);

            /* Force preemption */
            scx_bpf_kick_cpu(scx_bpf_task_cpu(p), SCX_KICK_PREEMPT);
        } else if (!escapable || is_critical) {
            if (stats)
                __sync_fetch_and_add(&stats->escalations_blocked, 1);
        }
    }
}

/*
 * sched_ext ops: enable - called when scheduler is enabled
 */
void BPF_STRUCT_OPS(morpheus_enable, struct task_struct *p)
{
}

/*
 * sched_ext ops: exit - called when scheduler exits
 */
void BPF_STRUCT_OPS(morpheus_exit, struct scx_exit_info *ei)
{
    UEI_RECORD(uei, ei);
}

/* Error recording for exit */
UEI_DEFINE(uei);

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
