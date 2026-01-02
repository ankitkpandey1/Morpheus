#!/bin/bash
set -e

# Cleanup
echo "Cleaning up..."
sudo pkill -9 scx_morpheus || true
sudo rm -rf /sys/fs/bpf/morpheus
sleep 1

# Verify cleanup
if [ -d "/sys/fs/bpf/morpheus" ]; then
    echo "Error: Failed to remove /sys/fs/bpf/morpheus"
    exit 1
fi

# Start Scheduler
echo "Starting Scheduler..."
sudo ../../target/release/scx_morpheus --enforce --pin-maps > scx.log 2>&1 &
SCHED_PID=$!

# Wait for maps to appear
echo "Waiting for maps..."
for i in {1..50}; do
    if sudo test -d "/sys/fs/bpf/morpheus"; then
        echo "Maps directory found."
        break
    fi
    sleep 0.1
done

if ! sudo test -d "/sys/fs/bpf/morpheus"; then
    echo "Error: Maps directory not created after 5s"
    cat scx.log
    exit 1
fi

# Give it a moment to finish pinning files
sleep 1

# Check if maps exist
if ! sudo test -e "/sys/fs/bpf/morpheus/worker_tid_map"; then
    echo "Error: worker_tid_map missing"
    cat scx.log
    exit 1
fi

# Permissions
echo "Setting permissions..."
sudo chmod +x /sys/fs/bpf
sudo chmod -R 777 /sys/fs/bpf/morpheus

# Run Benchmark
echo "Running Benchmark..."
source ../../.venv/bin/activate
export PYTHONPATH=$PYTHONPATH:$(pwd)/../python

# Run only Morpheus mode (locking/naive already done)
# Run one by one to avoid any interference
python benchmark.py --mode morpheus --workers 1 --duration 10 --output data/morpheus_w1.json
python benchmark.py --mode morpheus --workers 2 --duration 10 --output data/morpheus_w2.json
python benchmark.py --mode morpheus --workers 4 --duration 10 --output data/morpheus_w4.json
python benchmark.py --mode morpheus --workers 8 --duration 10 --output data/morpheus_w8.json

echo "Done."
