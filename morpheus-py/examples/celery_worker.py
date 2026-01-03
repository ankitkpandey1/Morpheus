#!/usr/bin/env python3
"""
Celery Worker Integration Example

Demonstrates how to use Morpheus with Celery for CPU-intensive background tasks.

Setup:
    pip install celery redis
    
    # Start Redis
    docker run -d -p 6379:6379 redis
    
    # Start Celery worker with Morpheus
    python -m morpheus.run -m celery -A celery_worker worker --loglevel=info
    
    # Or without morpheus runner:
    celery -A celery_worker worker --loglevel=info
"""

from celery import Celery
import time

# Import morpheus (graceful fallback if not installed)
try:
    from morpheus import checkpoint, critical, is_defensive_mode, pressure_level
    HAS_MORPHEUS = True
except ImportError:
    HAS_MORPHEUS = False
    def checkpoint(): return False
    def critical():
        from contextlib import nullcontext
        return nullcontext()
    def is_defensive_mode(): return False
    def pressure_level(): return None

# Configure Celery
app = Celery(
    'morpheus_celery',
    broker='redis://localhost:6379/0',
    backend='redis://localhost:6379/0'
)

app.conf.update(
    task_serializer='json',
    accept_content=['json'],
    result_serializer='json',
    timezone='UTC',
    enable_utc=True,
)


@app.task(bind=True)
def cpu_intensive_task(self, iterations: int = 100_000):
    """
    CPU-intensive Celery task with Morpheus checkpoints.
    
    This demonstrates cooperative scheduling in long-running background tasks.
    """
    start = time.monotonic()
    total = 0
    yields = 0
    
    for i in range(iterations):
        total += i * i
        
        # Check for kernel yield every 1000 iterations
        if i % 1000 == 0:
            if checkpoint():
                yields += 1
                # Update task state for monitoring
                self.update_state(
                    state='PROGRESS',
                    meta={'current': i, 'total': iterations, 'yields': yields}
                )
    
    elapsed = time.monotonic() - start
    
    return {
        'total': total,
        'iterations': iterations,
        'elapsed_ms': round(elapsed * 1000, 2),
        'kernel_yield_requests': yields,
        'defensive_mode': is_defensive_mode(),
        'pressure_level': pressure_level(),
    }


@app.task
def ffi_task(duration_ms: int = 100):
    """
    Task that performs FFI-sensitive operations.
    
    Uses critical section to prevent kernel preemption during sensitive code.
    """
    with critical():
        # Simulate FFI call - kernel won't force preempt here
        time.sleep(duration_ms / 1000)
    
    return {'status': 'completed', 'protected': True}


@app.task
def batch_process(items: list):
    """
    Process a batch of items with adaptive checkpointing.
    
    Demonstrates pressure-aware processing that yields more frequently
    under high system load.
    """
    results = []
    
    for idx, item in enumerate(items):
        # Process item (simulated)
        result = {'item': item, 'processed': True, 'value': item * 2}
        results.append(result)
        
        # Adaptive checkpointing based on pressure
        pressure = pressure_level() or 0
        checkpoint_interval = max(1, 100 - pressure)  # More frequent under pressure
        
        if idx % checkpoint_interval == 0:
            checkpoint()
    
    return {'processed': len(results), 'results': results}


# Example usage (for testing)
if __name__ == '__main__':
    print("=" * 50)
    print("Celery + Morpheus Example")
    print(f"Morpheus available: {HAS_MORPHEUS}")
    print("=" * 50)
    print()
    print("To test (requires Redis running on localhost:6379):")
    print("  1. Start worker: celery -A celery_worker worker --loglevel=info")
    print("  2. In Python shell:")
    print("       from celery_worker import cpu_intensive_task")
    print("       result = cpu_intensive_task.delay(50000)")
    print("       print(result.get())")
