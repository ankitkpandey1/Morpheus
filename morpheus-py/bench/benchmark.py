import asyncio
import time
import argparse
import statistics
import json
import sys
import os
from dataclasses import dataclass, asdict
from typing import List, Literal

# Try to import morpheus, handling failure gracefully for baseline tests
try:
    import morpheus
    MORPHEUS_AVAILABLE = True
except ImportError:
    MORPHEUS_AVAILABLE = False

Mode = Literal["blocking", "naive", "morpheus"]

@dataclass
class BenchmarkResult:
    mode: str
    workers: int
    duration: float
    latencies_us: List[float]
    total_work_units: int
    throughput: float

class Workload:
    def __init__(self, mode: Mode, workers: int, load_intensity: int):
        self.mode = mode
        self.workers = workers
        self.load_intensity = load_intensity
        self.running = True
        self.work_units = 0
        self.latencies = []

    async def run(self, duration: int):
        self.start_time = time.time()
        self.duration = duration
        
        # Start workers
        worker_tasks = [asyncio.create_task(self.worker_loop(i, duration)) for i in range(self.workers)]
        
        # Start latency probe
        probe_task = asyncio.create_task(self.latency_probe())
        
        # We await the workers. In blocking mode, they block, but will exit after duration.
        await asyncio.gather(*worker_tasks)
        
        self.running = False
        # probe might be stuck waiting for sleep(0.01) if workers were blocking
        # But now workers are done, so probe should wake up and see exit condition.
        await probe_task
        
        throughput = self.work_units / duration
        return BenchmarkResult(
            mode=self.mode,
            workers=self.workers,
            duration=duration,
            latencies_us=self.latencies,
            total_work_units=self.work_units,
            throughput=throughput
        )

    async def worker_loop(self, worker_id: int, duration: int):
        """Simulates CPU-bound work with different yielding strategies."""
        
        # Initialize Morpheus worker if needed
        if self.mode == "morpheus" and MORPHEUS_AVAILABLE:
            try:
                morpheus.init_worker(worker_id=worker_id, escapable=False)
            except Exception as e:
                print(f"Worker init failed: {e}")
                raise e

        end_time = time.time() + duration
        
        while time.time() < end_time:
            # Simulate CPU work (busy loop)
            # load_intensity determines how many iterations before a potential yield
            # For 50ms work with Python loop overhead, maybe 1M iters?
            # Let's keep it simpler: small chunks of work.
            
            # Perform a chunk of CPU work (approx 1ms)
            x = 0
            for i in range(10000): 
                x += i
            
            self.work_units += 1

            # Yielding strategy
            if self.mode == "blocking":
                # Never yield voluntarily
                pass
                
            elif self.mode == "naive":
                # Yield every iteration (too aggressive) or every N
                if self.work_units % 10 == 0:
                    await asyncio.sleep(0)
                    
            elif self.mode == "morpheus":
                if MORPHEUS_AVAILABLE:
                    # Checkpoint: yields ONLY if kernel requested it
                    await morpheus.async_checkpoint()
                else:
                    # Fallback if morpheus missing (shouldn't happen in test)
                    await asyncio.sleep(0)

    async def latency_probe(self):
        """Measures event loop responsiveness."""
        while self.running:
            start = time.perf_counter()
            # Yield to event loop. If loop is blocked, this will take a long time.
            await asyncio.sleep(0)
            end = time.perf_counter()
            
            latency_us = (end - start) * 1_000_000
            self.latencies.append(latency_us)
            
            # Sampling rate: ~100Hz (sleep is min 0)
            # However, if loop is blocked, we sample less frequently, which is fine.
            # We want to measure the delay.
            await asyncio.sleep(0.01)

async def main():
    parser = argparse.ArgumentParser(description="Morpheus Python Benchmark")
    parser.add_argument("--mode", type=str, choices=["blocking", "naive", "morpheus"], required=True)
    parser.add_argument("--workers", type=int, default=1, help="Number of CPU-bound workers")
    parser.add_argument("--duration", type=int, default=10, help="Duration in seconds")
    parser.add_argument("--output", type=str, default="result.json", help="Output JSON file")
    
    args = parser.parse_args()
    
    # Check morpheus requirements
    if args.mode == "morpheus" and not MORPHEUS_AVAILABLE:
        print("Error: Morpheus mode requested but module not found.")
        sys.exit(1)

    print(f"Starting {args.mode} benchmark with {args.workers} workers for {args.duration}s...")
    
    workload = Workload(args.mode, args.workers, load_intensity=10000)
    result = await workload.run(args.duration)
    
    print(f"done.")
    print(f"Throughput: {result.throughput:.2f} units/s")
    if result.latencies_us:
        p50 = statistics.median(result.latencies_us)
        p99 = statistics.quantiles(result.latencies_us, n=100)[98]
        print(f"Latency P50: {p50:.2f} us")
        print(f"Latency P99: {p99:.2f} us")
        print(f"Max Latency: {max(result.latencies_us):.2f} us")
    else:
        print("No latency samples collected!")

    with open(args.output, "w") as f:
        json.dump(asdict(result), f, indent=2)

if __name__ == "__main__":
    asyncio.run(main())
