# Cgroup Throttling Design

**Status**: Deferred to Phase 4 (Requires kernel changes)

## Overview

Morpheus currently uses `SCHED_KICK` to preempt workers that ignore yield hints. For stricter enforcement, we plan to integrate with Linux control groups (cgroup v2) to throttle misbehaving workers using the CPU controller.

## Proposed Architecture

### 1. BPF Implementation (scx_morpheus)

We need `BPF_ITER` support to walk cgroups and identify the cgroup associated with a misbehaving worker.

```c
// scx_morpheus.bpf.c

case MORPHEUS_ESCALATION_CGROUP_THROTTLE:
    struct task_struct *p = ctx->task;
    struct cgroup *cgrp = task_cgroup(p, cpu_cgrp_id);
    
    // Decrease quota for this cgroup
    bpf_cgroup_set_quota(cgrp, THROW_QUOTA_PCT); 
    break;
```

**Challenges**:
- `task_cgroup` requires GPL-compatible BPF helpers.
- Modifying cgroup limits from BPF is restricted.
- We might need a userspace agent to perform the actual write to `/sys/fs/cgroup/...`.

### 2. Userspace Agent Approach

Instead of direct BPF modification, `scx_morpheus` can emit an event:

```c
struct escalation_event {
    u32 worker_id;
    u32 cgroup_id;
    u32 severity;
};
// Emit to ring buffer
```

The userspace agent (`scx_morpheus` binary):
1.  Reads escalation events.
2.  Maps `cgroup_id` to file path.
3.  Writes to `cpu.max`.

### 3. Integration Plan

1.  **Kernel Patch**: verify if `scx_bpf_set_cgroup_weight` or similar APIs become available in sched_ext.
2.  **Userspace Fallback**: Implement the ring-buffer based userspace throttling agent.
3.  **Policy**: Define "penalty box" logic (throttle for X seconds, then restore).

## Next Steps

1.  Prototype userspace throttling agent.
2.  Benchmark latency impact of userspace cgroup modification.
