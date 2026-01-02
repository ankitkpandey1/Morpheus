# Morpheus-Hybrid

**Kernel-Guided Cooperative Async Runtime with Opt-In Escalation**

## Why Morpheus? (The Problem)

Modern async runtimes (Tokio, asyncio) and the Linux kernel operate in silos.
- **The Runtime** knows *when* it's safe to yield (e.g., not holding a lock/GIL) but doesn't know *if* the system is overloaded.
- **The Kernel** knows *if* the system is overloaded (runqueue pressure) but doesn't know *when* it's safe to preempt without causing priority inversion or lock contention.

This disconnection leads to:
1.  **Tail Latency Spikes**: The kernel preempts a worker holding a lock/GIL, stalling all other threads.
2.  **Throughput Loss**: Runtimes yield too aggressively (wasting CPU) or too lazily (starving others).
3.  **Non-Determinism**: Performance varies wildly under load.

## The Solution

Morpheus-Hybrid bridges this gap using **sched_ext** (Linux 6.12+):
1.  **Kernel-Guided**: The kernel monitors pressure and *hints* to the runtime when to yield ("Please yield soon").
2.  **Runtime-Controlled**: The runtime yields only at safe "checkpoints" (await points), preventing lock-holding preemption.
3.  **Opt-In Enforcement**: If a worker ignores hints for too long, the kernel *escalates* (force-preempts or throttles) but only if the worker has explicitly signaled it is "escapable" (safe to preempt).

**Result**: Deterministic tail latency under high load, with the safety of cooperative scheduling and the robustness of preemptive kernels.

## Features

- **Cooperative by default**: Kernel requests yields, runtime chooses when
- **Language-neutral**: Works with Rust async and Python asyncio
- **Zero-copy communication**: mmap'd SCBs, ring buffer hints
- **Safe**: Critical sections prevent forced preemption
- **Observable**: Metrics for hints, escalations, drops
- **Pluggable policies**: Observer-only mode or full enforcement

## Architecture Highlights

| Component | Description |
|-----------|-------------|
| **Scheduler Mode** | Observer-only (default, safest) or Enforced (opt-in) |
| **Worker States** | INIT → REGISTERED → RUNNING → QUIESCING → DEAD |
| **Escalation Policies** | NONE, THREAD_KICK, CGROUP_THROTTLE, HYBRID |
| **Runtime Modes** | DETERMINISTIC, PRESSURED, DEFENSIVE |
| **Yield Cause Ledger** | Tracks yield reasons (Voluntary, Hint, Budget, etc.) |
| **Language Adapters** | Abstract API preserving language semantics |

## Requirements

- Linux kernel 6.12+ with `CONFIG_SCHED_CLASS_EXT=y`
- `CAP_BPF` and `CAP_SYS_ADMIN` capabilities
- Rust 1.75+ (for building)
- clang/LLVM (for BPF compilation)

## Quick Start

### Building

```bash
# Install system dependencies (Debian/Ubuntu)
sudo apt install -y pkg-config libelf-dev clang llvm linux-headers-$(uname -r) \
    libc6-dev-i386 gcc-multilib libbpf-dev bpftool

# 1. Build Rust components
cargo build --release

# 2. Set up Python environment
python3 -m venv .venv
source .venv/bin/activate
pip install maturin patchelf

# 3. Build and install Python bindings
cd morpheus-py
maturin develop --release
```

### Running the Scheduler

```bash
# Load the sched_ext scheduler (requires root)
# Observer mode (default) - collects metrics, emits hints, no enforcement
sudo ./target/release/scx_morpheus --slice-ms 5 --grace-ms 100 --debug

# Enforcement mode (Opt-in) - enables cgroup throttling and CPU kicks
sudo ./target/release/scx_morpheus --enforce
```

### Rust Usage

```rust
use morpheus_runtime::{checkpoint, critical_section, Builder};

#[tokio::main]
async fn main() {
    // Heavy computation with checkpoints
    for i in 0..1_000_000 {
        // ... work ...
        if i % 1000 == 0 {
            checkpoint!(); // Yield if kernel requested
        }
    }

    // Protect FFI/zero-copy operations
    {
        let _guard = critical_section();
        unsafe {
            // Kernel won't force-preempt here
        }
    }
}
```

### Python Usage

```python
import morpheus
import asyncio

async def heavy_computation():
    # Register the worker thread with the scheduler
    morpheus.init_worker()
    
    for i in range(1_000_000):
        # ... work ...
        if i % 1000 == 0:
            morpheus.checkpoint()  # Check for kernel hints

async def ffi_work():
    with morpheus.critical():
        # Protected from forced preemption
        pass

# Run the FastAPI example
# python -m morpheus.run -m uvicorn examples.fastapi_app:app --loop asyncio --port 8000
```

## Benchmarks

See [benchmark.md](benchmark.md) for comprehensive comparative methodology and results.

**Key findings:**
- Sub-nanosecond checkpoint overhead (~751 ps)
- Stable 2 µs operation latency across configurations
- Critical section protection adds only 0.50% overhead
- Starvation recovery validation (p99 > 13 ms without Morpheus)

| Workers | Blocking (Throughput) | Naive (Throughput) | Morpheus (Throughput) |
| :--- | :--- | :--- | :--- |
| **8** | 33,431 | 3,616 | **44,236** |

*Morpheus demonstrates 12x higher throughput than Naive yielding at saturation by avoiding unnecessary context switches, and 30% higher than Blocking by maintaining cooperative responsiveness without preemption interrupt overhead.*

**New in v0.1.0:**
*   **Adaptive Slicing**: Dynamically adjusts time slice (2ms-10ms) based on runqueue depth.
*   **Hot Path Optimization**: Python-side atomic checks reduce non-yielding checkpoint overhead to <5%.

```bash
# Starvation recovery test
sudo ./target/release/starvation -n 4 --duration 10

# Adversarial critical section test  
sudo ./target/release/liar --critical-duration-ms 500

# Latency distribution
sudo ./target/release/latency --duration 30 --workers 4 --pressure

# Criterion microbenchmark
cd morpheus-bench && cargo bench
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `--slice-ms` | 5 | Time slice before yield hint |
| `--grace-ms` | 100 | Grace period before escalation |
| `--debug` | false | Enable debug logging |

## Safety

- Python workers default to `escapable=false` (GIL safety)
- Critical sections prevent all forced preemption
- Escalation is failure recovery, not scheduling policy
- Scheduler auto-falls back to CFS on any BPF error

## Non-Goals

See [NON_GOALS.md](NON_GOALS.md) for features explicitly out of scope:
- Per-task kernel scheduling
- Bytecode-level preemption
- Kernel-managed budgets

## Testing

```bash
# Run integration tests
cargo test -p morpheus-runtime
cargo test -p morpheus-common

# Run all tests
cargo test --workspace
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) - Detailed architecture with diagrams
- [benchmark.md](benchmark.md) - Comparative benchmark methodology & results
- [NON_GOALS.md](NON_GOALS.md) - Architectural guardrails
- [OPERATOR.md](OPERATOR.md) - Operator deployment guide

## License

GPL-2.0-only

SPDX-License-Identifier: GPL-2.0-only
