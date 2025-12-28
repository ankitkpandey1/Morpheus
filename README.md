# Morpheus-Hybrid

**Kernel-Guided Cooperative Async Runtime with Opt-In Escalation**

Morpheus-Hybrid enables async runtimes (Rust, Python) to receive yield hints from the Linux kernel scheduler and respond at safe points. The kernel only forces preemption on workers that have explicitly opted in and ignored hints.

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
| **Determinism Modes** | DETERMINISTIC, PRESSURED, DEFENSIVE |
| **Language Adapters** | Abstract API preserving language semantics |

## Requirements

- Linux kernel 6.12+ with `CONFIG_SCHED_CLASS_EXT=y`
- `CAP_BPF` and `CAP_SYS_ADMIN` capabilities
- Rust 1.75+ (for building)
- clang/LLVM (for BPF compilation)

## Quick Start

### Building

```bash
# Install dependencies (Debian/Ubuntu)
sudo apt install -y \
    pkg-config \
    libelf-dev \
    clang \
    llvm \
    linux-headers-$(uname -r) \
    libc6-dev-i386 \
    gcc-multilib \
    libbpf-dev \
    bpftool

# Verify kernel sched_ext support
cat /boot/config-$(uname -r) | grep SCHED_CLASS_EXT
# Should output: CONFIG_SCHED_CLASS_EXT=y

# Build all
cargo build --release

# Build Python module (optional)
cd morpheus-py && maturin build --release
```

### Running the Scheduler

```bash
# Load the sched_ext scheduler (requires root)
# Observer mode (default) - collects metrics, emits hints, no enforcement
sudo ./target/release/scx_morpheus --slice-ms 5 --grace-ms 100 --debug
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
    for i in range(1_000_000):
        # ... work ...
        if i % 1000 == 0:
            morpheus.checkpoint()  # Check for kernel hints

async def ffi_work():
    with morpheus.critical():
        # Protected from forced preemption
        pass
```

## Benchmarks

```bash
# Starvation recovery test
sudo ./target/release/starvation -n 4 --duration 10

# Adversarial critical section test
sudo ./target/release/liar --critical-duration-ms 500

# Latency distribution
sudo ./target/release/latency --duration 30 --workers 4 --pressure
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

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) - Detailed architecture with diagrams
- [benchmark.md](benchmark.md) - Performance data and methodology
- [NON_GOALS.md](NON_GOALS.md) - Architectural guardrails

## License

GPL-2.0-only

SPDX-License-Identifier: GPL-2.0-only
