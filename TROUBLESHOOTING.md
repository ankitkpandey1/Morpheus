# Troubleshooting Guide

Common issues and solutions for Morpheus-Hybrid.

---

## Quick Diagnostics

```bash
# Check if sched_ext is supported
cat /boot/config-$(uname -r) | grep SCHED_CLASS_EXT

# Verify scheduler is running
sudo bpftool prog list | grep morpheus

# Check BPF map status
sudo bpftool map list | grep -E "(scb_map|worker_tid)"
```

---

## Common Issues

### 1. "Failed to load BPF program"

**Symptoms:**
```
Error: Failed to load BPF program: Operation not permitted
```

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Missing capabilities | Run with `sudo` or add `CAP_BPF`, `CAP_SYS_ADMIN` |
| Kernel too old | Requires Linux 6.12+ with `CONFIG_SCHED_CLASS_EXT=y` |
| Missing BTF | Enable `CONFIG_DEBUG_INFO_BTF=y` in kernel config |
| libbpf version mismatch | Ensure libbpf-dev matches kernel version |

**Verify kernel config:**
```bash
zcat /proc/config.gz | grep -E "(SCHED_CLASS_EXT|DEBUG_INFO_BTF)"
```

---

### 2. "Worker not receiving hints"

**Symptoms:**
- `checkpoint()` always returns `false`
- Stats show `hints=0`

**Causes & Solutions:**

1. **Worker not registered**: Ensure `register_worker(tid, worker_id)` is called
   ```rust
   maps.register_worker(get_tid(), worker_id)?;
   ```

2. **Maps not pinned**: Run scheduler with `--pin-maps`
   ```bash
   sudo ./scx_morpheus --pin-maps
   ```

3. **Wrong worker state**: Check SCB `worker_state` is `RUNNING`

---

### 3. "High hint drop rate"

**Symptoms:**
- Stats show `hints_dropped > 0`
- Runtime enters defensive mode

**Causes & Solutions:**

| Cause | Solution |
|-------|----------|
| Ring buffer overflow | Increase `MORPHEUS_RINGBUF_SIZE` |
| Slow checkpoint polling | Reduce checkpoint interval |
| Too many workers | Limit concurrent workers |

**Monitor drops:**
```bash
sudo bpftool map dump name stats_map | grep hints
```

---

### 4. "Escalations keep happening"

**Symptoms:**
- `escalations` counter keeps increasing
- Workers being force-preempted

**Causes:**
1. Workers ignoring hints (missing `checkpoint!()` calls)
2. Long critical sections blocking yields
3. `grace_ms` too short

**Solutions:**
```bash
# Increase grace period
sudo ./scx_morpheus --grace-ms 500

# Or switch to observer mode
sudo ./scx_morpheus --debug  # Default is observer-only
```

---

### 5. Python module not loading

**Symptoms:**
```python
ImportError: No module named 'morpheus'
```

**Solutions:**

1. **Build the module:**
   ```bash
   cd morpheus-py && maturin develop
   ```

2. **Python version mismatch:**
   ```bash
   # Check PyO3 compatibility
   python3 --version  # Must be <= 3.13 for PyO3 0.22.x
   ```

3. **Set ABI3 compatibility (Python 3.14+):**
   ```bash
   export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
   maturin develop
   ```

---

## Debug Mode

Enable debug logging for detailed diagnostics:

```bash
# Scheduler debug mode
sudo ./scx_morpheus --debug --stats-interval 1

# Rust runtime tracing
RUST_LOG=morpheus_runtime=debug cargo run

# Python verbose
python -c "import morpheus; print(morpheus.get_stats())"
```

---

## Collecting Diagnostics

When reporting issues, include:

```bash
# System info
uname -a
cat /etc/os-release

# Kernel config
zcat /proc/config.gz | grep -E "(BPF|SCHED)" 

# BPF status
sudo bpftool prog list
sudo bpftool map list

# Morpheus stats (if running)
sudo bpftool map dump name stats_map
```

---

## Getting Help

- GitHub Issues: [github.com/ankitkpandey1/Morpheus/issues](https://github.com/ankitkpandey1/Morpheus/issues)
- Check existing issues before reporting
- Include diagnostic output in bug reports
