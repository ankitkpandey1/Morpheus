# Morpheus-Hybrid Non-Goals

**SPDX-License-Identifier: GPL-2.0-only**

This document explicitly defines features that are **OUT OF SCOPE** for
Morpheus-Hybrid. These are architectural guardrails to prevent feature creep.

## Non-Goal #1: Per-Task Kernel Scheduling

**What it means**: The kernel ONLY operates on worker threads, never on
individual async tasks, coroutines, or green threads.

**Why not**:
- Tasks are a userspace abstraction invisible to the kernel
- Context switch overhead would be prohibitive
- Language runtimes already handle task scheduling efficiently

**Enforcement**:
- BPF maps are keyed by worker_id, not task_id
- No task state is stored in kernel structures

---

## Non-Goal #2: Bytecode-Level Preemption

**What it means**: The kernel cannot interrupt code at arbitrary points.
Safe points are determined entirely by the language runtime.

**Why not**:
- Stack invariants would be violated
- FFI calls would be corrupted
- GC invariants would break
- Zero-copy regions would fault

**Enforcement**:
- Critical sections protect invariant-sensitive code
- Runtime controls checkpoint placement
- Kernel only emits hints, never forces without consent

---

## Non-Goal #3: Kernel-Managed Budgets

**What it means**: Time budgets are **advisory only**. The kernel does not
enforce strict time limits on worker execution.

**Why not**:
- Hard preemption conflicts with cooperative model
- Critical sections would be violated
- No safe way to preempt mid-computation

**Enforcement**:
- `budget_remaining_ns` is informational
- Escalation requires explicit worker opt-in (escapable flag)
- Grace period always observed before escalation

---

## Non-Goal #4: Cross-Runtime Task Migration

**What it means**: Tasks cannot be migrated between different Morpheus
runtimes (e.g., Rust to Python).

**Why not**:
- Incompatible memory models
- Different ownership semantics
- No shared task representation

---

## Non-Goal #5: Kernel-Side Task Queuing

**What it means**: The kernel does not maintain task queues. All task
scheduling happens in userspace.

**Why not**:
- BPF programs cannot manage unbounded queues
- Task state is opaque to kernel
- Would require kernel memory allocation

---

## Architectural Invariants

These invariants MUST be preserved:

1. **Kernel operates on threads, not tasks**
2. **Hints are advisory, escalation requires consent**
3. **Critical sections are inviolable**
4. **Language semantics are preserved**
5. **No kernel memory allocation for tasks**

---

## How to Enforce

When reviewing changes, reject any that:

- Add task-level state to BPF maps
- Introduce kernel-side task queues
- Allow preemption during critical sections
- Assume specific language semantics in BPF code
- Enforce hard budget limits without escalation consent
