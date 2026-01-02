"""
Morpheus asyncio integration module.

This module provides asyncio-compatible helpers for integrating Morpheus
cooperative scheduling with Python's asyncio event loop.
"""

import asyncio
from contextlib import asynccontextmanager
from typing import Optional, Any
import sys

# Import the native extension
try:
    import _morpheus
except ImportError:
    # Fallback/Stub or re-raise
    # Only use stub if we want soft degradation
    try:
        # Maybe it's installed as legacy 'morpheus'?
        import morpheus as _morpheus
    except ImportError:
        _morpheus = None


class _MorpheusStub:
    def checkpoint(self) -> bool: return False
    def yield_requested(self) -> bool: return False
    def acknowledge_yield(self) -> bool: return False
    def enter_critical_section(self) -> None: pass
    def exit_critical_section(self) -> None: pass
    def is_in_critical_section(self) -> bool: return False
    def pressure_level(self) -> Optional[int]: return None
    def is_defensive_mode(self) -> bool: return False

if _morpheus is None:
    _morpheus = _MorpheusStub()


async def morpheus_checkpoint() -> bool:
    """Async checkpoint that yields the event loop if kernel requested."""
    if _morpheus.checkpoint():
        _morpheus.acknowledge_yield()
        await asyncio.sleep(0)  # Yield to event loop
        return True
    return False


async def force_yield() -> None:
    """Force yield to the event loop, acknowledging any kernel request."""
    _morpheus.acknowledge_yield()
    await asyncio.sleep(0)


@asynccontextmanager
async def morpheus_critical():
    """Async context manager for critical sections."""
    _morpheus.enter_critical_section()
    try:
        yield
    finally:
        _morpheus.exit_critical_section()


def is_kernel_pressured(threshold: int = 50) -> bool:
    """Check if the kernel is reporting high pressure."""
    level = _morpheus.pressure_level()
    return level is not None and level > threshold


def is_defensive_mode() -> bool:
    """Check if defensive mode is active."""
    return _morpheus.is_defensive_mode()


class AdaptiveCheckpointer:
    """Adaptive checkpointing based on kernel pressure."""
    
    def __init__(self, min_interval: int = 100, max_interval: int = 10000):
        self.min_interval = min_interval
        self.max_interval = max_interval
        self._last_check = 0
    
    def should_check(self, iteration: int) -> bool:
        """Determine if we should checkpoint at this iteration."""
        pressure = _morpheus.pressure_level() or 0
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
    This wrapper injects periodic checkpoints into the loop execution.
    """
    
    def __init__(self, inner: asyncio.AbstractEventLoop):
        self._inner = inner
        self._check_handle = None

    def run_forever(self) -> None:
        self._schedule_check()
        try:
            return self._inner.run_forever()
        finally:
            self._cancel_check()

    def run_until_complete(self, future: Any) -> Any:
        self._schedule_check()
        try:
            return self._inner.run_until_complete(future)
        finally:
            self._cancel_check()

    def _schedule_check(self):
        # Schedule the next check to run as soon as possible (next tick)
        self._check_handle = self._inner.call_soon(self._check_and_reschedule)

    def _cancel_check(self):
        if self._check_handle:
            self._check_handle.cancel()
            self._check_handle = None

    def _check_and_reschedule(self):
        # Perform explicit checkpoint (fast FFI call)
        # If kernel requests yield, this call returns true
        # In Sync Checkpoint, we don't 'await asyncio.sleep(0)' because we are IN the loop callback.
        # However, checking allows kernel to update budget/hints.
        
        # NOTE: Sync checkpoint alone updates state but does not release GIL or CPU to OS scheduler 
        # unless implemented to do so. Morpheus checkpoint does NOT yield thread unless configured?
        # Actually `morpheus_checkpoint()` async helper calls `acknowledge_yield` then `await sleep(0)`.
        # Here we are in the loop logic. 
        # We can't await. 
        # But simply calling `checkpoint()` allows us to read hints.
        # If we see a hint, we should probably stop processing callbacks and run I/O?
        # But asyncio structure doesn't allow "stop processing callbacks now".
        
        # However, calling checkpoint() updates the shared memory so kernel knows we are alive.
        # And if we are running callbacks, we are consuming CPU.
        # Just calling it is valuable for keeping state updated.
        _morpheus.checkpoint()
        
        # Reschedule check for next tick
        self._schedule_check()

    def __getattr__(self, name: str) -> Any:
        return getattr(self._inner, name)

    def close(self):
        self._inner.close()


def install_morpheus_loop():
    """Install the Morpheus-aware event loop policy."""
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
