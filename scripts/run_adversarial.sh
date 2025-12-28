#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Morpheus-Hybrid Adversarial Starvation Test
#
# Tests escalation behavior when workers ignore yield hints

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

WORKERS=${1:-4}
DURATION=${2:-10}

echo "=== Adversarial Starvation Test ==="
echo "Workers: $WORKERS (1 adversarial + $(($WORKERS - 1)) cooperative)"
echo "Duration: ${DURATION}s"
echo ""

if [ "$EUID" -ne 0 ]; then
    echo "Error: This test requires root to load the scheduler"
    exit 1
fi

# Build
cargo build --release -p morpheus-bench -p scx_morpheus

# Start scheduler in background
echo "Loading scx_morpheus scheduler..."
./target/release/scx_morpheus --slice-ms 5 --grace-ms 100 --debug &
SCHED_PID=$!

# Wait for scheduler to attach
sleep 1

# Run starvation test
echo "Running starvation test..."
./target/release/starvation -n "$WORKERS" --duration "$DURATION"

# Cleanup
echo "Stopping scheduler..."
kill $SCHED_PID 2>/dev/null || true
wait $SCHED_PID 2>/dev/null || true

echo "Done."
