# Contributing to Morpheus-Hybrid

Thank you for your interest in contributing!

## Development Setup

### Requirements

- Rust 1.75+
- clang/LLVM (for BPF)
- Python 3.8+ (for Python bindings)
- Linux 6.12+ (for running the scheduler)

### Building

```bash
# Build all non-BPF crates
cargo build -p morpheus-common -p morpheus-runtime -p morpheus-py -p morpheus-bench

# Build Python bindings
cd morpheus-py && pip install maturin && maturin develop

# Build BPF scheduler (requires kernel headers)
bpftool btf dump file /sys/kernel/btf/vmlinux format c > scx_morpheus/src/bpf/vmlinux.h
cargo build -p scx_morpheus
```

### Testing

```bash
# Run unit tests
cargo test -p morpheus-common -p morpheus-runtime --lib

# Run benchmarks (requires root + kernel 6.12)
sudo ./target/release/starvation
sudo ./target/release/liar
sudo ./target/release/latency
```

### Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- Follow Rust API guidelines

## Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and linting
5. Submit a pull request

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for system design.

## License

By contributing, you agree that your contributions will be licensed under GPL-2.0.
