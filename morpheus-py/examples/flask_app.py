#!/usr/bin/env python3
"""
Flask Integration Example

Demonstrates how to use Morpheus with Flask for CPU-intensive API routes.

To run:
    source .venv/bin/activate
    pip install flask
    python morpheus-py/examples/flask_app.py
"""

from flask import Flask, jsonify
import time

# Import morpheus (graceful fallback if not installed)
try:
    from morpheus import checkpoint, critical, is_defensive_mode
    HAS_MORPHEUS = True
except ImportError:
    HAS_MORPHEUS = False
    def checkpoint(): return False
    def critical(): 
        from contextlib import nullcontext
        return nullcontext()
    def is_defensive_mode(): return False

app = Flask(__name__)


@app.route("/")
def root():
    return jsonify({
        "message": "Morpheus + Flask works!",
        "morpheus_available": HAS_MORPHEUS,
    })


@app.route("/compute/<int:iterations>")
def compute(iterations: int):
    """
    CPU-intensive endpoint with Morpheus checkpoints.
    
    The checkpoint() call allows Morpheus to signal if the kernel
    is requesting a yield - useful for cooperative scheduling in
    CPU-bound routes.
    """
    start = time.monotonic()
    total = 0
    yields = 0
    
    for i in range(iterations):
        total += i * i
        
        # Check every 1000 iterations
        if i % 1000 == 0 and checkpoint():
            yields += 1
            # In sync Flask, we can't truly yield, but we record it
    
    elapsed = time.monotonic() - start
    
    return jsonify({
        "total": total,
        "iterations": iterations,
        "elapsed_ms": round(elapsed * 1000, 2),
        "kernel_yield_requests": yields,
        "defensive_mode": is_defensive_mode(),
    })


@app.route("/ffi")
def ffi_simulation():
    """
    Simulates FFI-sensitive code with critical section protection.
    """
    with critical():
        # Inside critical section - kernel won't force preempt
        time.sleep(0.1)  # Simulate FFI call
    
    return jsonify({"status": "FFI completed safely"})


@app.route("/health")
def health():
    return jsonify({"status": "healthy", "morpheus": HAS_MORPHEUS})


if __name__ == "__main__":
    print("=" * 50)
    print("Flask + Morpheus Example")
    print(f"Morpheus available: {HAS_MORPHEUS}")
    print("=" * 50)
    app.run(host="0.0.0.0", port=5000, debug=True)
