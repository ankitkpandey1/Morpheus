# Performance Tuning Guide

Optimize Morpheus-Hybrid for your workload.

---

## Key Parameters

| Parameter | Default | Range | Description |
|-----------|---------|-------|-------------|
| `--slice-ms` | 5 | 1-100 | Time before yield hint |
| `--grace-ms` | 100 | 10-5000 | Grace period before escalation |
| `--stats-interval` | 5 | 0-60 | Stats output interval (0=disabled) |

---

## Workload Profiles

### Latency-Sensitive (Web servers, APIs)

```bash
sudo ./scx_morpheus --slice-ms 2 --grace-ms 50
```

- Short slice → frequent hints
- Short grace → fast recovery from runaway

**Checkpoint frequency:**
```rust
// Every 100-500 iterations
if i % 100 == 0 { checkpoint!(); }
```

---

### Throughput-Focused (Batch processing, ML)

```bash
sudo ./scx_morpheus --slice-ms 20 --grace-ms 500
```

- Longer slice → less overhead
- Longer grace → batched work uninterrupted

**Checkpoint frequency:**
```rust
// Every 5000-10000 iterations
if i % 5000 == 0 { checkpoint!(); }
```

---

### Mixed Workloads

```bash
sudo ./scx_morpheus --slice-ms 5 --grace-ms 200
```

Use priority to differentiate:
```rust
// High-priority workers
scb.set_priority(900);

// Background workers
scb.set_priority(100);
```

---

## Checkpoint Optimization

### Overhead Comparison

| Strategy | Overhead | Responsiveness |
|----------|----------|----------------|
| Every iteration | ~750ps × N | Maximum |
| Every 100 | ~7.5µs per 100 | High |
| Every 1000 | ~750ns per 1000 | Medium |
| Every 10000 | ~75ns per 10000 | Low |

### Adaptive Checkpointing

Check kernel pressure to adjust:

```rust
let pressure = scb.pressure_level();
let interval = if pressure > 80 {
    100  // High pressure - check often
} else if pressure > 50 {
    500  // Medium pressure
} else {
    2000 // Low pressure - less checking
};

if i % interval == 0 { checkpoint!(); }
```

---

## Critical Section Guidelines

### Do

```rust
// Short, necessary critical sections
let _guard = critical_section();
unsafe { ffi_call() };
// Guard dropped immediately
```

### Don't

```rust
// DON'T hold critical section across I/O
let _guard = critical_section();
expensive_network_call().await;  // BAD!
```

### Measuring Impact

```bash
# Run critical section benchmark
cargo run --release -p morpheus-bench -- critical
```

Target: <1% overhead for critical section protection.

---

## Memory Optimization

### SCB Map Size

Default: 1024 workers × 128 bytes = 128KB

For fewer workers:
```c
// In morpheus_shared.h
#define MORPHEUS_MAX_WORKERS 256  // 32KB
```

### Ring Buffer Size

Default: 256KB

Adjust if seeing drops:
```c
#define MORPHEUS_RINGBUF_SIZE (512 * 1024)  // 512KB
```

---

## Monitoring

### Key Metrics to Watch

| Metric | Warning Threshold | Action |
|--------|-------------------|--------|
| `hints_dropped` | >1% of hints | Increase ring buffer |
| `escalations` | >0.1% of ticks | Add checkpoints |
| `defensive_triggers` | >10/min | Check worker issues |

### Prometheus Export

```rust
use morpheus_runtime::metrics;

// Start metrics server
metrics().render()  // Returns Prometheus text format
```

---

## Benchmarking

### Baseline (no Morpheus)

```bash
cd morpheus-bench
cargo run --release -- baseline --duration 10
```

### With Morpheus

```bash
cargo run --release -- latency --duration 10 --workers 4
```

### Compare

```bash
cargo run --release -- starvation -n 4 --duration 10
```

---

## Platform-Specific

### AMD Ryzen

Good cache behavior. Use default settings.

### Intel Xeon

May benefit from larger slices on high core count:
```bash
sudo ./scx_morpheus --slice-ms 10
```

### ARM64

Test with your workload. Cache line size differences may affect SCB performance.

---

## Quick Reference

```bash
# Low-latency mode
sudo ./scx_morpheus --slice-ms 2 --grace-ms 50

# High-throughput mode  
sudo ./scx_morpheus --slice-ms 20 --grace-ms 500

# Debug mode
sudo ./scx_morpheus --debug --stats-interval 1

# Observer only (safe, no enforcement)
sudo ./scx_morpheus  # Default
```
