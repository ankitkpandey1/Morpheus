#!/usr/bin/env python3
"""
Example: Morpheus-Hybrid with asyncio

Demonstrates how to use Morpheus cooperative scheduling with Python asyncio.
"""

import asyncio
import time
from morpheus_asyncio import (
    morpheus_checkpoint,
    morpheus_critical,
    is_kernel_pressured,
    AdaptiveCheckpointer,
)


async def cpu_intensive_task(task_id: int, iterations: int = 100_000):
    """
    Simulates CPU-intensive work with regular checkpoints.
    """
    print(f"[Task {task_id}] Starting CPU work...")
    start = time.monotonic()
    
    result = 0
    for i in range(iterations):
        # Simulate computation
        result += i * i
        
        # Check for kernel yield requests every 1000 iterations
        if i % 1000 == 0:
            yielded = await morpheus_checkpoint()
            if yielded:
                print(f"[Task {task_id}] Yielded at iteration {i}")
    
    elapsed = time.monotonic() - start
    print(f"[Task {task_id}] Completed in {elapsed:.3f}s")
    return result


async def ffi_sensitive_task(task_id: int):
    """
    Demonstrates critical section usage for FFI-sensitive code.
    """
    print(f"[Task {task_id}] Entering critical section...")
    
    async with morpheus_critical():
        # Inside critical section - kernel will NOT force preempt
        print(f"[Task {task_id}] In critical section (kernel won't interrupt)")
        
        # Simulate FFI operation
        await asyncio.sleep(0.1)
        
    print(f"[Task {task_id}] Left critical section")


async def adaptive_checkpoint_example():
    """
    Demonstrates adaptive checkpointing based on kernel pressure.
    """
    print("[Adaptive] Starting with pressure-aware checkpoints...")
    
    checker = AdaptiveCheckpointer(min_interval=100, max_interval=5000)
    
    for i in range(50_000):
        # Computation
        _ = i * i
        
        # Adaptive checkpoint - frequency adjusts to kernel pressure
        if checker.should_check(i):
            await morpheus_checkpoint()
            if is_kernel_pressured():
                print(f"[Adaptive] High pressure at iteration {i}")
    
    print("[Adaptive] Completed")


async def main():
    """
    Run example tasks demonstrating Morpheus integration.
    """
    print("=" * 60)
    print("Morpheus-Hybrid Python asyncio Example")
    print("=" * 60)
    print()
    
    # Run CPU-intensive tasks concurrently
    print("--- Running CPU-intensive tasks ---")
    await asyncio.gather(
        cpu_intensive_task(1, 50_000),
        cpu_intensive_task(2, 50_000),
    )
    print()
    
    # Demonstrate critical sections
    print("--- Running FFI-sensitive tasks ---")
    await asyncio.gather(
        ffi_sensitive_task(1),
        ffi_sensitive_task(2),
    )
    print()
    
    # Demonstrate adaptive checkpointing
    print("--- Running adaptive checkpoint example ---")
    await adaptive_checkpoint_example()
    print()
    
    print("=" * 60)
    print("Example completed successfully!")
    print("=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
