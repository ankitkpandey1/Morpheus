# Morpheus-Hybrid Operator Guide

**SPDX-License-Identifier: GPL-2.0-only**

## Prerequisites

- Linux kernel 6.12+ with `CONFIG_SCHED_CLASS_EXT=y`
- `CONFIG_DEBUG_INFO_BTF=y` enabled
- Root access or `CAP_BPF` + `CAP_SYS_ADMIN` capabilities

## Installation

### Building

```bash
# Install build dependencies
sudo apt install -y \
    pkg-config libelf-dev clang llvm \
    linux-headers-$(uname -r) \
    libbpf-dev bpftool

# Build release binaries
cargo build --release
```

### Loading the Scheduler

```bash
# Observer mode (default, safest)
sudo ./target/release/scx_morpheus --slice-ms 5 --grace-ms 100

# With debug logging
sudo ./target/release/scx_morpheus --debug
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `--slice-ms` | 5 | Time slice before hint emission |
| `--grace-ms` | 100 | Grace period before escalation |
| `--debug` | false | Enable debug tracing |

## Operating Modes

### Observer Mode (Default)

- Collects metrics and emits hints
- **No enforcement actions**
- Safe for production use

### Enforced Mode

> ⚠️ **Requires explicit operator opt-in**

Enabled by setting scheduler mode in configuration. Allows:
- CPU kicks for unresponsive workers
- Cgroup throttling

## Monitoring

### Prometheus Metrics

The runtime exposes metrics at `/metrics`:

```
morpheus_hint_count_total{worker_id,reason}
morpheus_hint_drops_total
morpheus_escalation_count_total{policy}
morpheus_defensive_mode_total{worker_id}
morpheus_last_ack_latency_seconds{worker_id}
```

### BPF Statistics

```bash
# View per-CPU stats
sudo bpftool map dump name stats_map

# View global pressure
sudo bpftool map dump name global_pressure_map
```

## Uninstallation

### Stop the Scheduler

```bash
# Send SIGINT (Ctrl+C) to gracefully stop
kill -SIGINT $(pidof scx_morpheus)
```

The scheduler will automatically detach and the system will fall back to CFS.

### Verify Detachment

```bash
# Should show no sched_ext scheduler active
cat /sys/kernel/sched_ext/state
```

## Rollback Procedure

If issues occur:

1. **Stop the scheduler** (see above)
2. System automatically falls back to CFS
3. No reboot required

### Emergency Shutdown

```bash
# Force kill if graceful shutdown fails
sudo killall -9 scx_morpheus
```

## Disable Escalation Globally

To run in pure observer mode (no escalation under any circumstances):

1. Ensure scheduler is started without enforced mode
2. All workers will use `EscalationPolicy::None`
3. No CPU kicks or throttling will occur

## Troubleshooting

### Scheduler Won't Load

1. Check kernel config:
   ```bash
   grep SCHED_CLASS_EXT /boot/config-$(uname -r)
   ```
2. Check BTF availability:
   ```bash
   ls /sys/kernel/btf/vmlinux
   ```

### High Hint Drops

- Increase `MORPHEUS_RINGBUF_SIZE` in configuration
- Check worker checkpoint frequency

### Workers Not Receiving Hints

1. Verify worker TID is registered in `worker_tid_map`
2. Check worker state is `RUNNING`
3. Verify scheduler is attached

## Support

For issues, please file a GitHub issue with:
- Kernel version (`uname -r`)
- `dmesg` output related to sched_ext
- `bpftool prog show` output
