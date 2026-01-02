"""
Morpheus Runner

Executes a Python script or module with the Morpheus event loop policy installed.

Usage:
    python -m morpheus.run <script_path> [args...]
    python -m morpheus.run -m <module_name> [args...]
"""

import sys
import runpy
import argparse
from morpheus.asyncio import install_morpheus_loop

def main():
    parser = argparse.ArgumentParser(description="Morpheus Runner")
    parser.add_argument('script', help="Script path or module name (with -m)")
    parser.add_argument('-m', '--module', action='store_true', help="Run as module")
    parser.add_argument('args', nargs=argparse.REMAINDER, help="Arguments for the script")
    
    # We parse manually to avoid consuming script args if mixed
    # But for simplicity, we assume standard usage:
    # python -m morpheus.run script.py arg1 arg2
    # python -m morpheus.run -m mymodule arg1
    
    # Actually, argparse is tricky with runpy because sys.argv needs to be set perfectly
    # for the target script.
    
    # Let's inspect sys.argv directly.
    # argv[0] is this script (when executed via runpy as __main__, it might be weird).
    # If run via `python -m morpheus.run`, sys.argv[0] is full path to this file?
    
    # Strategy: Peel off arguments meant for runner, keep the rest for the script.
    
    args = sys.argv[1:]
    target = ""
    is_module = False
    script_args = []
    
    if not args:
        print("Usage: python -m morpheus.run <script> [args]")
        sys.exit(1)
        
    if args[0] == '-m':
        if len(args) < 2:
            print("Error: -m requires a module name")
            sys.exit(1)
        is_module = True
        target = args[1]
        script_args = args[2:]
    else:
        target = args[0]
        script_args = args[1:]
        
    # Install Morpheus Loop Policy
    install_morpheus_loop()
    
    # Fix sys.argv for the target script
    sys.argv = [target] + script_args
    
    # Run
    try:
        if is_module:
            runpy.run_module(target, run_name="__main__", alter_sys=True)
        else:
            runpy.run_path(target, run_name="__main__")
    except Exception as e:
        # Traceback is usually printed by runpy, but ensuring it propagates
        raise e

if __name__ == "__main__":
    main()
