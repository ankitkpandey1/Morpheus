"""
Morpheus-Hybrid Python Package

Kernel-guided cooperative async runtime with opt-in escalation.
"""

from .asyncio import (
    morpheus_checkpoint,
    morpheus_critical,
    force_yield,
    is_kernel_pressured,
    is_defensive_mode,
    AdaptiveCheckpointer,
    MorpheusEventLoopPolicy,
    install_morpheus_loop,
)

# Re-export native module functions if available
try:
    from _morpheus import (
        init_worker,
        checkpoint,
        yield_requested,
        yield_now_async,
        acknowledge_yield,
        pressure_level,
        budget_remaining_ns,
        set_priority,
        is_in_critical_section,
        worker_id,
        critical,
        enter_critical_section,
        exit_critical_section,
        get_stats,
        HINT_BUDGET,
        HINT_PRESSURE,
        HINT_IMBALANCE,
        HINT_DEADLINE,
        MAX_WORKERS,
        DEFAULT_SLICE_NS,
        GRACE_PERIOD_NS,
    )
except ImportError:
    # Native module not available
    pass

__version__ = "0.1.0"
__all__ = [
    # Asyncio integration
    'morpheus_checkpoint',
    'morpheus_critical',
    'force_yield',
    'is_kernel_pressured',
    'is_defensive_mode',
    'AdaptiveCheckpointer',
    'MorpheusEventLoopPolicy',
    'install_morpheus_loop',
    # Native functions (when available)
    'init_worker',
    'checkpoint',
    'yield_requested',
    'async_checkpoint',
    'acknowledge_yield',
    'pressure_level',
    'budget_remaining_ns',
    'set_priority',
    'is_in_critical_section',
    'worker_id',
    'critical',
    'enter_critical_section',
    'exit_critical_section',
    'get_stats',
]

async def async_checkpoint():
    """
    Async checkpoint - await this to yield to the event loop if kernel requests.
    
    This function is optimized to avoid allocation overhead when no yield
    is requested (the common case).
    """
    if checkpoint():
        await yield_now_async()
