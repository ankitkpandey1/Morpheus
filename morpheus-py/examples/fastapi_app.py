"""
FastAPI Integration Example

To run:
  python -m morpheus.run -m uvicorn examples.fastapi_app:app --loop asyncio --port 8000
"""

from fastapi import FastAPI
import asyncio
import time

app = FastAPI()

@app.get("/")
async def root():
    return {"message": "Morpheus + FastAPI works!"}

@app.get("/work")
async def work():
    """Simulate cooperative async work."""
    total = 0
    for i in range(10000):
        total += i
        if i % 100 == 0:
            # Yield to allow Morpheus checks and I/O
            await asyncio.sleep(0)
    return {"total": total}
