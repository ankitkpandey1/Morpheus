# Morpheus Python Bindings

Python bindings for the Morpheus-Hybrid kernel-guided cooperative async runtime.

## Installation

### From Source (requires Rust)

```bash
cd morpheus-py
pip install maturin
maturin develop
```

### Using the Module

```python
import asyncio
from morpheus_hybrid import (
    morpheus_checkpoint,
    morpheus_critical,
    is_kernel_pressured,
)

async def heavy_work():
    for i in range(1_000_000):
        # ... computation ...
        if i % 1000 == 0:
            await morpheus_checkpoint()

async def ffi_work():
    async with morpheus_critical():
        # Protected from kernel escalation
        pass
```

## API Reference

### Async Functions

- `morpheus_checkpoint()` - Check for kernel yield request and yield if needed
- `morpheus_critical()` - Async context manager for critical sections
- `force_yield()` - Force yield to event loop

### Sync Functions

- `checkpoint()` - Synchronous check (returns bool)
- `enter_critical_section()` / `exit_critical_section()` - Manual critical section
- `pressure_level()` - Get kernel pressure (0-100)
- `is_defensive_mode()` - Check if in defensive mode

### Utilities

- `AdaptiveCheckpointer` - Adjust checkpoint frequency based on pressure
- `MorpheusEventLoopPolicy` - Event loop policy with automatic checkpoints

## Requirements

- Linux 6.12+ with sched_ext enabled
- `scx_morpheus` scheduler loaded
- Python 3.8+

## CLI Runner (Zero-Config)

You can run existing asyncio scripts without code changes using the `morpheus.run` module.

```bash
# Run a script
python -m morpheus.run my_script.py

# Run a module
python -m morpheus.run -m my_module
```

## Framework Integrations

### FastAPI / Uvicorn

Morpheus works seamlessly with FastAPI. You must use the `--loop asyncio` flag with Uvicorn to ensure it uses the Morpheus-compatible event loop policy (instead of `uvloop`).

```bash
# Run using the Morpheus runner
python -m morpheus.run -m uvicorn examples.fastapi_app:app --loop asyncio --port 8000
```

### Celery

For Celery, since workers are spawned as separate processes, you must install the Morpheus loop policy inside the worker process initialization.

```python
from celery import signals
from morpheus.asyncio import install_morpheus_loop

@signals.worker_process_init.connect
def setup_morpheus(**kwargs):
    install_morpheus_loop()
```
