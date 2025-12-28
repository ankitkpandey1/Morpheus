#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Morpheus-Hybrid Benchmark Runner
#
# Usage: ./run_benchmarks.sh [--quick | --full]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="${PROJECT_ROOT}/BENCHMARKS"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

# Default constants
GRACE_PERIOD_NS=50000  # 50µs
DEFENSIVE_ITERATIONS=64
HINT_RATE_LIMIT=100  # hints/sec per worker

# Create results directory
mkdir -p "$RESULTS_DIR"

echo "=== Morpheus-Hybrid Benchmark Suite ==="
echo "Timestamp: $TIMESTAMP"
echo "Results: $RESULTS_DIR"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Warning: Some benchmarks require root for scheduler loading"
fi

# Build release binaries
echo "Building release binaries..."
cd "$PROJECT_ROOT"
cargo build --release -p morpheus-bench

MODE=${1:---quick}

run_starvation_test() {
    echo ""
    echo "=== Starvation Recovery Test ==="
    local workers=${1:-4}
    local duration=${2:-10}
    
    echo "Workers: $workers, Duration: ${duration}s"
    
    if [ "$EUID" -eq 0 ]; then
        ./target/release/starvation -n "$workers" --duration "$duration" \
            2>&1 | tee "$RESULTS_DIR/starvation_${TIMESTAMP}.log"
    else
        echo "Skipping (requires root)"
    fi
}

run_liar_test() {
    echo ""
    echo "=== Liar Task Test (Adversarial Critical Sections) ==="
    local critical_ms=${1:-500}
    
    echo "Critical section duration: ${critical_ms}ms"
    
    if [ "$EUID" -eq 0 ]; then
        ./target/release/liar --critical-duration-ms "$critical_ms" \
            2>&1 | tee "$RESULTS_DIR/liar_${TIMESTAMP}.log"
    else
        echo "Skipping (requires root)"
    fi
}

run_latency_test() {
    echo ""
    echo "=== Latency Distribution Test ==="
    local workers=${1:-4}
    local duration=${2:-30}
    
    echo "Workers: $workers, Duration: ${duration}s"
    
    if [ "$EUID" -eq 0 ]; then
        ./target/release/latency --duration "$duration" --workers "$workers" --pressure \
            2>&1 | tee "$RESULTS_DIR/latency_${TIMESTAMP}.log"
    else
        echo "Skipping (requires root)"
    fi
}

run_checkpoint_microbench() {
    echo ""
    echo "=== Checkpoint Microbenchmark ==="
    
    cargo bench --bench checkpoint -- --noplot 2>&1 | tee "$RESULTS_DIR/checkpoint_${TIMESTAMP}.log"
}

case "$MODE" in
    --quick)
        echo "Running quick benchmarks..."
        run_checkpoint_microbench
        run_starvation_test 2 5
        ;;
    --full)
        echo "Running full benchmark suite..."
        run_checkpoint_microbench
        run_starvation_test 4 30
        run_liar_test 500
        run_latency_test 4 60
        ;;
    *)
        echo "Usage: $0 [--quick | --full]"
        exit 1
        ;;
esac

echo ""
echo "=== Benchmark Complete ==="
echo "Results saved to: $RESULTS_DIR"
