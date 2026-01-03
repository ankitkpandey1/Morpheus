#!/usr/bin/env python3
"""
Django Middleware Integration Example

Demonstrates how to integrate Morpheus with Django for request processing.

Add to your Django project:
    MIDDLEWARE = [
        ...
        'morpheus_middleware.MorpheusMiddleware',
    ]
"""

import time

# Import morpheus (graceful fallback if not installed)
try:
    from morpheus import checkpoint, is_defensive_mode, pressure_level
    HAS_MORPHEUS = True
except ImportError:
    HAS_MORPHEUS = False
    def checkpoint(): return False
    def is_defensive_mode(): return False
    def pressure_level(): return None


class MorpheusMiddleware:
    """
    Django middleware that adds Morpheus monitoring to requests.
    
    This middleware:
    1. Records kernel pressure at request start
    2. Checks for yield requests periodically (for long requests)
    3. Adds Morpheus headers to responses
    """
    
    def __init__(self, get_response):
        self.get_response = get_response
    
    def __call__(self, request):
        # Record request start
        request.morpheus_start = time.monotonic()
        request.morpheus_yields = 0
        request.morpheus_pressure = pressure_level()
        
        # Process request
        response = self.get_response(request)
        
        # Add Morpheus headers
        elapsed = time.monotonic() - request.morpheus_start
        response['X-Morpheus-Available'] = str(HAS_MORPHEUS)
        response['X-Morpheus-Elapsed-Ms'] = str(round(elapsed * 1000, 2))
        response['X-Morpheus-Pressure'] = str(request.morpheus_pressure or 'N/A')
        response['X-Morpheus-Defensive'] = str(is_defensive_mode())
        
        return response


def morpheus_checkpoint_decorator(func):
    """
    Decorator for Django views that adds periodic checkpoints.
    
    Usage:
        @morpheus_checkpoint_decorator
        def my_view(request):
            # CPU-intensive work
            ...
    """
    from functools import wraps
    
    @wraps(func)
    def wrapper(request, *args, **kwargs):
        result = func(request, *args, **kwargs)
        # Checkpoint after view execution
        if checkpoint():
            request.morpheus_yields = getattr(request, 'morpheus_yields', 0) + 1
        return result
    
    return wrapper


# Example Django view (not runnable without Django project)
"""
from django.http import JsonResponse

@morpheus_checkpoint_decorator  
def compute_view(request):
    iterations = int(request.GET.get('iterations', 10000))
    
    total = 0
    yields = 0
    for i in range(iterations):
        total += i * i
        if i % 1000 == 0 and checkpoint():
            yields += 1
    
    return JsonResponse({
        'total': total,
        'iterations': iterations,
        'yields': yields,
    })
"""

if __name__ == '__main__':
    print("Django + Morpheus Middleware Example")
    print(f"Morpheus available: {HAS_MORPHEUS}")
    print()
    print("To use: Add MorpheusMiddleware to your Django MIDDLEWARE setting")
