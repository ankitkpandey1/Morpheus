/* SPDX-License-Identifier: GPL-2.0 */
/*
 * morpheus_shared.h - Shared types for Morpheus-Hybrid kernel↔runtime protocol
 *
 * This header defines the binary contract between the sched_ext BPF scheduler
 * and userspace runtimes (Rust, Python, etc.). All types are language-neutral
 * and operate at the worker-thread level, not at task/coroutine granularity.
 *
 * Memory Protocol:
 * - SCBs live in a BPF_MAP_TYPE_ARRAY, not in userspace memory
 * - Userspace accesses SCBs via mmap or bpf_map_update_elem
 * - Kernel directly reads/writes map values, never dereferences userspace ptrs
 */

#ifndef __MORPHEUS_SHARED_H
#define __MORPHEUS_SHARED_H

#ifdef __KERNEL__
#include <linux/types.h>
#else
#include <stdint.h>
typedef uint8_t  __u8;
typedef uint16_t __u16;
typedef uint32_t __u32;
typedef uint64_t __u64;
typedef int32_t  __s32;
typedef int64_t  __s64;
#endif

/*
 * Shared Control Block (SCB) - One per worker thread
 *
 * Memory layout is critical: 64-byte aligned for cache efficiency.
 * All fields are atomically accessed; no locks required.
 *
 * Split into two cache lines:
 *   - Line 1 (bytes 0-63): Kernel → Runtime fields
 *   - Line 2 (bytes 64-127): Runtime → Kernel fields
 */
struct morpheus_scb {
    /* === Cache Line 1: Kernel → Runtime === */

    /*
     * Monotonically increasing sequence number. Kernel increments this
     * when it wants the runtime to yield. Runtime compares against
     * last_ack_seq to detect pending yield requests.
     */
    __u64 preempt_seq;

    /*
     * Remaining time budget in nanoseconds. Advisory only; kernel
     * updates this on each tick. Runtime may use for soft budgeting.
     */
    __u64 budget_remaining_ns;

    /*
     * System pressure level (0-100). Kernel sets this based on
     * runqueue depth, CPU utilization, and memory pressure.
     * 0 = no pressure, 100 = critical.
     */
    __u32 kernel_pressure_level;

    __u32 _pad0;
    __u64 _reserved0[4];

    /* === Cache Line 2: Runtime → Kernel === */

    /*
     * Set to 1 when runtime is in a critical section (FFI, zero-copy,
     * GIL-held, or invariant-sensitive code). Kernel MUST NOT escalate
     * while this is set.
     */
    __u32 is_in_critical_section;

    /*
     * Set to 1 if this worker has opted in to forced escalation.
     * Default: 0 for Python (GIL safety), 1 for Rust.
     * Kernel will NEVER force-preempt workers with escapable=0.
     */
    __u32 escapable;

    /*
     * Last acknowledged preempt_seq. Runtime sets this after yielding.
     * Kernel uses (preempt_seq - last_ack_seq) to detect unresponsive
     * workers.
     */
    __u64 last_ack_seq;

    /*
     * Advisory priority (0-1000). Higher = more important.
     * Kernel may use for hint frequency or escalation grace periods.
     */
    __u32 runtime_priority;

    __u32 _pad1;
    __u64 _reserved1[3];

} __attribute__((aligned(64)));

/* Compile-time size check */
_Static_assert(sizeof(struct morpheus_scb) == 128,
               "morpheus_scb must be exactly 128 bytes (2 cache lines)");

/*
 * Hint reasons - why the kernel is requesting a yield
 */
#define MORPHEUS_HINT_BUDGET    1  /* Worker exceeded time slice */
#define MORPHEUS_HINT_PRESSURE  2  /* System under CPU pressure */
#define MORPHEUS_HINT_IMBALANCE 3  /* Runqueue imbalance detected */
#define MORPHEUS_HINT_DEADLINE  4  /* Hard deadline approaching */

/*
 * Hint message - sent via ring buffer (edge-triggered events)
 *
 * Hints are advisory. A well-behaved runtime should respond by yielding
 * at the next safe point. The kernel rate-limits hint emission.
 */
struct morpheus_hint {
    /* Matches the preempt_seq that triggered this hint */
    __u64 seq;

    /* One of MORPHEUS_HINT_* constants */
    __u32 reason;

    /* Thread ID of the target worker */
    __u32 target_tid;

    /* Deadline in nanoseconds (monotonic). Kernel may escalate after this. */
    __u64 deadline_ns;
};

/*
 * Configuration constants
 */
#define MORPHEUS_MAX_WORKERS      1024
#define MORPHEUS_DEFAULT_SLICE_NS (5 * 1000 * 1000)  /* 5ms default slice */
#define MORPHEUS_GRACE_PERIOD_NS  (100 * 1000 * 1000) /* 100ms before escalation */
#define MORPHEUS_RINGBUF_SIZE     (256 * 1024)       /* 256KB ring buffer */

/*
 * Map names (for bpf_obj_get)
 */
#define MORPHEUS_SCB_MAP_NAME     "scb_map"
#define MORPHEUS_HINT_RINGBUF_NAME "hint_ringbuf"
#define MORPHEUS_WORKER_MAP_NAME  "worker_tid_map"

#endif /* __MORPHEUS_SHARED_H */
