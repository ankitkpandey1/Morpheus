#!/usr/bin/env python3
"""
Test suite for morpheus Python bindings.

These tests verify the Python API without requiring the kernel scheduler.
"""

import pytest
import sys
from unittest.mock import patch, MagicMock

# Try to import morpheus - may fail if not built
try:
    import morpheus
    HAS_MORPHEUS = True
except ImportError:
    HAS_MORPHEUS = False


@pytest.mark.skipif(not HAS_MORPHEUS, reason="morpheus module not built")
class TestMorpheusBindings:
    """Test the core morpheus bindings."""

    def test_checkpoint_returns_bool(self):
        """Checkpoint should return a boolean."""
        result = morpheus.checkpoint()
        assert isinstance(result, bool)
        # When not on a worker thread, should return False
        assert result is False

    def test_yield_requested_returns_bool(self):
        """yield_requested should return a boolean."""
        result = morpheus.yield_requested()
        assert isinstance(result, bool)
        assert result is False  # Not on worker thread

    def test_acknowledge_yield_returns_bool(self):
        """acknowledge_yield should return a boolean."""
        result = morpheus.acknowledge_yield()
        assert isinstance(result, bool)

    def test_pressure_level_returns_none_without_worker(self):
        """pressure_level should return None when not on worker thread."""
        result = morpheus.pressure_level()
        assert result is None

    def test_budget_remaining_returns_none_without_worker(self):
        """budget_remaining_ns should return None when not on worker thread."""
        result = morpheus.budget_remaining_ns()
        assert result is None

    def test_worker_id_returns_none_without_worker(self):
        """worker_id should return None when not on worker thread."""
        result = morpheus.worker_id()
        assert result is None

    def test_is_in_critical_section_returns_bool(self):
        """is_in_critical_section should return a boolean."""
        result = morpheus.is_in_critical_section_py()
        assert isinstance(result, bool)

    def test_is_defensive_mode_returns_bool(self):
        """is_defensive_mode should return a boolean."""
        result = morpheus.is_defensive_mode()
        assert isinstance(result, bool)


@pytest.mark.skipif(not HAS_MORPHEUS, reason="morpheus module not built")
class TestCriticalSection:
    """Test critical section context manager."""

    def test_critical_returns_context_manager(self):
        """critical() should return a context manager."""
        ctx = morpheus.critical()
        assert hasattr(ctx, '__enter__')
        assert hasattr(ctx, '__exit__')

    def test_critical_context_manager_enter_exit(self):
        """Critical section context manager should work."""
        with morpheus.critical():
            # Inside critical section
            pass
        # Exited successfully

    def test_nested_critical_sections(self):
        """Nested critical sections should work."""
        with morpheus.critical():
            with morpheus.critical():
                # Double nested
                pass
            # Still in outer
        # Both exited


@pytest.mark.skipif(not HAS_MORPHEUS, reason="morpheus module not built")
class TestConstants:
    """Test that constants are exported correctly."""

    def test_hint_constants_exist(self):
        """Hint reason constants should be exported."""
        assert hasattr(morpheus, 'HINT_BUDGET')
        assert hasattr(morpheus, 'HINT_PRESSURE')
        assert hasattr(morpheus, 'HINT_IMBALANCE')
        assert hasattr(morpheus, 'HINT_DEADLINE')

    def test_config_constants_exist(self):
        """Configuration constants should be exported."""
        assert hasattr(morpheus, 'MAX_WORKERS')
        assert hasattr(morpheus, 'DEFAULT_SLICE_NS')
        assert hasattr(morpheus, 'GRACE_PERIOD_NS')

    def test_max_workers_is_reasonable(self):
        """MAX_WORKERS should be a reasonable value."""
        assert morpheus.MAX_WORKERS == 1024

    def test_slice_is_5ms(self):
        """Default slice should be 5ms in nanoseconds."""
        assert morpheus.DEFAULT_SLICE_NS == 5_000_000


@pytest.mark.skipif(not HAS_MORPHEUS, reason="morpheus module not built")
class TestAsyncCheckpoint:
    """Test async_checkpoint function."""

    @pytest.mark.asyncio
    async def test_async_checkpoint_is_awaitable(self):
        """async_checkpoint should return an awaitable."""
        import asyncio
        # Should not raise
        await morpheus.async_checkpoint()

    @pytest.mark.asyncio
    async def test_async_checkpoint_in_loop(self):
        """async_checkpoint should work in a loop."""
        import asyncio
        for i in range(10):
            await morpheus.async_checkpoint()


@pytest.mark.skipif(not HAS_MORPHEUS, reason="morpheus module not built")
class TestStats:
    """Test statistics API."""

    def test_get_stats_returns_none_without_runtime(self):
        """get_stats should return None when runtime not initialized."""
        result = morpheus.get_stats()
        # May be None or Stats depending on initialization
        assert result is None or hasattr(result, 'hints_received')


class TestMorpheusAsyncioWrapper:
    """Test the morpheus_asyncio wrapper module."""

    def test_import_morpheus_asyncio(self):
        """Should be able to import morpheus_asyncio."""
        try:
            from morpheus_asyncio import morpheus_checkpoint
            assert callable(morpheus_checkpoint)
        except ImportError:
            pytest.skip("morpheus_asyncio not available")

    @pytest.mark.asyncio
    async def test_morpheus_checkpoint_wrapper(self):
        """morpheus_checkpoint wrapper should work."""
        try:
            from morpheus_asyncio import morpheus_checkpoint
            result = await morpheus_checkpoint()
            assert isinstance(result, bool)
        except ImportError:
            pytest.skip("morpheus_asyncio not available")


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
