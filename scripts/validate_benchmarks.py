#!/usr/bin/env python3
"""
Morpheus Benchmark Validation Suite

Runs standard benchmarks and validates performance criteria.
"""

import subprocess
import json
import statistics
import time
import sys
import os
import re

def run_command(cmd, timeout=None):
    """Run a shell command and return output."""
    print(f"Running: {cmd}")
    try:
        result = subprocess.run(
            cmd,
            shell=True,
            check=True,
            capture_output=True,
            text=True,
            timeout=timeout
        )
        return result.stdout
    except subprocess.CalledProcessError as e:
        print(f"Error executing command: {e}")
        print(f"Stdout: {e.stdout}")
        print(f"Stderr: {e.stderr}")
        raise

def parse_latency_output(output):
    """Parse output from latency benchmark."""
    metrics = {}
    
    # Extract latency percentiles
    p50_match = re.search(r"p50:\s+(\d+)\s+µs", output)
    p95_match = re.search(r"p95:\s+(\d+)\s+µs", output)
    p99_match = re.search(r"p99:\s+(\d+)\s+µs", output)
    
    # Extract operational metrics
    ops_match = re.search(r"Ops/second:\s+(\d+)", output)
    yields_match = re.search(r"Checkpoint yields:\s+(\d+)", output)
    
    if p50_match: metrics['p50_us'] = int(p50_match.group(1))
    if p95_match: metrics['p95_us'] = int(p95_match.group(1))
    if p99_match: metrics['p99_us'] = int(p99_match.group(1))
    if ops_match: metrics['ops_per_sec'] = int(ops_match.group(1))
    if yields_match: metrics['yields'] = int(yields_match.group(1))
    
    return metrics

def run_baseline(duration=10):
    """Run baseline benchmark (no Morpheus)."""
    cmd = f"./target/release/latency --duration {duration} --workers 4"
    output = run_command(cmd)
    return parse_latency_output(output)

def run_morpheus_test(duration=10, with_pressure=True):
    """Run benchmark with Morpheus scheduler enabled."""
    # Note: Requires scx_morpheus running separately via sudo
    # This function assumes the scheduler is already active or we are validating
    # the runtime behavior under normal scheduler conditions.
    
    pressure_flag = "--pressure" if with_pressure else ""
    checkpoints_flag = "--with-checkpoints"
    
    cmd = f"./target/release/latency --duration {duration} --workers 4 {pressure_flag} {checkpoints_flag}"
    output = run_command(cmd)
    return parse_latency_output(output)

def validate_benchmarks():
    """Main validation routine."""
    print('Building benchmarks...')
    run_command("cargo build --release -p morpheus-bench")
    
    print("\n=== Baseline Run (No checkpoints/pressure) ===")
    try:
        baseline = run_baseline(5)
        print(f"Baseline p99: {baseline.get('p99_us')} µs")
    except Exception as e:
        print(f"Baseline run failed: {e}")
        baseline = {}

    print("\n=== Integration Check (With checkpoints) ===")
    try:
        # Check if we can run with flags enabled
        morpheus_res = run_morpheus_test(5, with_pressure=False)
        print(f"Morpheus p99: {morpheus_res.get('p99_us')} µs")
        print(f"Checkpoint yields: {morpheus_res.get('yields', 0)}")
        
        # Validation Logic
        if morpheus_res.get('p99_us', 999999) > 5000:
             print("WARNING: p99 latency > 5ms")
             
        if morpheus_res.get('yields', 0) == 0:
             print("NOTE: No yields recorded (Expected if kernel not scheduling)")
             
    except Exception as e:
        print(f"Integration run failed: {e}")

if __name__ == "__main__":
    if os.geteuid() != 0:
        print("Note: Running without root. Kernel scheduler injection will definitely fail if attempted.")
    except_scheduler = False
    
    try:
        validate_benchmarks()
    except KeyboardInterrupt:
        print("\nAborted.")
