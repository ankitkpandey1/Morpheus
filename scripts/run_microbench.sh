#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Morpheus-Hybrid Checkpoint Microbenchmark
#
# Measures checkpoint overhead

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Checkpoint Microbenchmark ==="
echo "This measures the overhead of the checkpoint!() macro"
echo ""

cd "$PROJECT_ROOT"

# Run criterion benchmark
cargo bench --bench checkpoint -- \
    --save-baseline morpheus \
    --noplot

echo ""
echo "Benchmark complete. Results in target/criterion/"
