"""
Morpheus asyncio integration module.

This module provides asyncio-compatible helpers for integrating Morpheus
cooperative scheduling with Python's asyncio event loop.

Usage:
    import asyncio
    from morpheus_asyncio import morpheus_checkpoint, morpheus_critical

    async def heavy_work():
        for i in range(1_000_000):
            # ... computation ...
            if i % 1000 == 0:
                await morpheus_checkpoint()

    async def ffi_work():
        async with morpheus_critical():
            # Protected from kernel escalation
            pass
"""

import asyncio
from contextlib import asynccontextmanager
from typing import Optional

# Import the native morpheus module (built from morpheus-py)
try:
    import morpheus as _morpheus
except ImportError:
    # Stub for when native module isn't available
    class _MorpheusStub:
        def checkpoint(self) -> bool: return False
        def yield_requested(self) -> bool: return False
        def acknowledge_yield(self) -> bool: return False
        def enter_critical_section(self) -> None: pass
        def exit_critical_section(self) -> None: pass
        def is_in_critical_section(self) -> bool: return False
        def pressure_level(self) -> Optional[int]: return None
        def is_defensive_mode(self) -> bool: return False
    _morpheus = _MorpheusStub()


async def morpheus_checkpoint() -> bool:
    """
    Async checkpoint that yields the event loop if kernel requested.
    
    This should be called periodically in CPU-intensive async code.
    It checks if the kernel has requested a yield, and if so, yields
    control back to the event loop.
    
    Returns:
        True if a yield was performed, False otherwise.
        
    Example:
        async def heavy_computation():
            for i in range(1_000_000):
                # ... compute ...
                if i % 1000 == 0:
                    await morpheus_checkpoint()
    """
    if _morpheus.checkpoint():
        # Kernel requested yield - acknowledge and yield event loop
        _morpheus.acknowledge_yield()
        await asyncio.sleep(0)  # Yield to event loop
        return True
    return False


async def force_yield() -> None:
    """
    Force yield to the event loop, acknowledging any kernel request.
    
    Use this when you know you should yield regardless of kernel pressure.
    """
    _morpheus.acknowledge_yield()
    await asyncio.sleep(0)


@asynccontextmanager
async def morpheus_critical():
    """
    Async context manager for critical sections.
    
    While inside a critical section:
    - The kernel will not force-preempt this worker
    - morpheus_checkpoint() will not yield
    
    Critical sections can be nested safely.
    
    Example:
        async def ffi_work():
            async with morpheus_critical():
                # FFI calls, zero-copy operations
                await do_ffi_stuff()
    """
    _morpheus.enter_critical_section()
    try:
        yield
    finally:
        _morpheus.exit_critical_section()


def is_kernel_pressured(threshold: int = 50) -> bool:
    """
    Check if the kernel is reporting high pressure.
    
    Args:
        threshold: Pressure level (0-100) above which to return True.
        
    Returns:
        True if pressure level exceeds threshold.
    """
    level = _morpheus.pressure_level()
    return level is not None and level > threshold


def is_defensive_mode() -> bool:
    """
    Check if defensive mode is active.
    
    Defensive mode is triggered when ring buffer overflows or
    sequence gaps are detected. In this mode, every checkpoint
    will yield.
    """
    return _morpheus.is_defensive_mode()


class AdaptiveCheckpointer:
    """
    Adaptive checkpointing based on kernel pressure.
    
    Adjusts checkpoint frequency based on kernel-reported pressure level.
    Higher pressure = more frequent checkpoints.
    
    Example:
        checker = AdaptiveCheckpointer(min_interval=100, max_interval=10000)
        
        async def work():
            for i in range(1_000_000):
                # ... work ...
                if checker.should_check(i):
                    await morpheus_checkpoint()
    """
    
    def __init__(self, min_interval: int = 100, max_interval: int = 10000):
        """
        Args:
            min_interval: Minimum iterations between checks (high pressure).
            max_interval: Maximum iterations between checks (no pressure).
        """
        self.min_interval = min_interval
        self.max_interval = max_interval
        self._last_check = 0
    
    def should_check(self, iteration: int) -> bool:
        """
        Determine if we should checkpoint at this iteration.
        
        Args:
            iteration: Current loop iteration.
            
        Returns:
            True if checkpoint should be performed.
        """
        # Calculate interval based on pressure
        pressure = _morpheus.pressure_level() or 0
        
        # Linear interpolation: high pressure = low interval
        interval = self.max_interval - (
            (self.max_interval - self.min_interval) * pressure // 100
        )
        
        if iteration - self._last_check >= interval:
            self._last_check = iteration
            return True
        return False


class MorpheusEventLoopPolicy(asyncio.DefaultEventLoopPolicy):
    """
    Event loop policy that integrates Morpheus checkpoints.
    
    This policy wraps the default event loop to automatically insert
    checkpoint checks between event loop iterations.
    
    Usage:
        asyncio.set_event_loop_policy(MorpheusEventLoopPolicy())
        asyncio.run(main())
    """
    
    def new_event_loop(self) -> asyncio.AbstractEventLoop:
        loop = super().new_event_loop()
        return _MorpheusEventLoop(loop)


class _MorpheusEventLoop(asyncio.AbstractEventLoop):
    """
    Wrapper around an event loop that adds Morpheus integration.
    """
    
    def __init__(self, inner: asyncio.AbstractEventLoop):
        self._inner = inner
        self._check_interval = 0.001  # Check every 1ms
        self._last_check = 0.0
    
    def __getattr__(self, name):
        return getattr(self._inner, name)
    
    def _run_once(self):
        """Run one iteration of the event loop with checkpoint."""
        import time
        now = time.monotonic()
        
        if now - self._last_check >= self._check_interval:
            self._last_check = now
            if _morpheus.checkpoint():
                _morpheus.acknowledge_yield()
        
        return self._inner._run_once()


# Convenience function to set up Morpheus-aware event loop
def install_morpheus_loop():
    """
    Install the Morpheus-aware event loop policy.
    
    Call this at the start of your program before creating event loops.
    """
    asyncio.set_event_loop_policy(MorpheusEventLoopPolicy())


__all__ = [
    'morpheus_checkpoint',
    'morpheus_critical', 
    'force_yield',
    'is_kernel_pressured',
    'is_defensive_mode',
    'AdaptiveCheckpointer',
    'MorpheusEventLoopPolicy',
    'install_morpheus_loop',
]
