#!/bin/bash
set -e

# Configuration
DURATION=10
WORKERS_LIST=(1 2 4 8)
MODES=("blocking" "naive" "morpheus")
OUTPUT_DIR="data"

mkdir -p $OUTPUT_DIR

# Ensure python environment
source ../../.venv/bin/activate
export PYTHONPATH=$PYTHONPATH:$(pwd)/../python

GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}=== Starting Morpheus Python Benchmarks ===${NC}"

# Check for scx_morpheus (must be running for morpheus mode)
# Ideally, we start it here, but it requires sudo.
# We assume the user has started it or we use sudo.
# Let's try to start it in the background if not running.
if ! pgrep -x "scx_morpheus" > /dev/null; then
    echo "Starting scx_morpheus scheduler..."
    sudo ../../target/release/scx_morpheus --enforce > /dev/null 2>&1 &
    SCHED_PID=$!
    sleep 2
    echo "Scheduler started (PID $SCHED_PID)"
    
    # Cleanup trap
    trap "sudo kill $SCHED_PID" EXIT
fi

for workers in "${WORKERS_LIST[@]}"; do
    for mode in "${MODES[@]}"; do
        echo -e "\nRunning: Mode=${mode}, Workers=${workers}"
        output_file="${OUTPUT_DIR}/${mode}_w${workers}.json"
        
        python benchmark.py \
            --mode $mode \
            --workers $workers \
            --duration $DURATION \
            --output $output_file
            
        echo "Saved to $output_file"
    done
done

echo -e "\n${GREEN}=== Benchmarks Complete ===${NC}"
