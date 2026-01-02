import json
import glob
import os
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from typing import List, Dict

DATA_DIR = "data"
PLOT_DIR = "plots"

def load_data():
    results = []
    for file in glob.glob(f"{DATA_DIR}/*.json"):
        with open(file, "r") as f:
            data = json.load(f)
            results.append(data)
    return results

def plot_latency_cdf(df: pd.DataFrame):
    plt.figure(figsize=(10, 6))
    
    modes = df['mode'].unique()
    for mode in modes:
        subset = df[df['mode'] == mode]
        workers = subset['workers'].iloc[0] # Assuming comparing same workers for now or aggregating
        
        # Flatten all latencies for this mode
        latencies = []
        for l_list in subset['latencies_us']:
            latencies.extend(l_list)
            
        if not latencies:
            continue
            
        sorted_lat = np.sort(latencies)
        p = 1. * np.arange(len(sorted_lat)) / (len(sorted_lat) - 1)
        
        plt.plot(sorted_lat, p, label=f"{mode}")

    plt.title("Latency CDF (Cumulative Distribution Function)")
    plt.xlabel("Latency (microseconds)")
    plt.ylabel("Probability")
    plt.grid(True)
    plt.legend()
    plt.semilogx() # Log scale for latency often helps
    plt.savefig(f"{PLOT_DIR}/latency_cdf.png")
    plt.close()

def plot_tail_latency_vs_load(df: pd.DataFrame):
    # P99 vs Workers
    plt.figure(figsize=(10, 6))
    
    modes = df['mode'].unique()
    for mode in modes:
        subset = df[df['mode'] == mode].sort_values('workers')
        workers = subset['workers']
        p99s = []
        for l_list in subset['latencies_us']:
            if l_list:
                p99s.append(np.percentile(l_list, 99))
            else:
                p99s.append(0)
        
        plt.plot(workers, p99s, marker='o', label=mode)
        
    plt.title("Tail Latency (P99) vs Load")
    plt.xlabel("Number of Workers")
    plt.ylabel("P99 Latency (us)")
    plt.grid(True)
    plt.legend()
    plt.savefig(f"{PLOT_DIR}/p99_vs_load.png")
    plt.close()

def plot_throughput_vs_load(df: pd.DataFrame):
    plt.figure(figsize=(10, 6))
    
    modes = df['mode'].unique()
    for mode in modes:
        subset = df[df['mode'] == mode].sort_values('workers')
        workers = subset['workers']
        throughput = subset['throughput']
        
        plt.plot(workers, throughput, marker='o', label=mode)
        
    plt.title("Throughput vs Load")
    plt.xlabel("Number of Workers")
    plt.ylabel("Work Units / Second")
    plt.grid(True)
    plt.legend()
    plt.savefig(f"{PLOT_DIR}/throughput_vs_load.png")
    plt.close()

def main():
    os.makedirs(PLOT_DIR, exist_ok=True)
    data = load_data()
    if not data:
        print("No data found!")
        return

    df = pd.DataFrame(data)
    
    # Generate Plots
    plot_latency_cdf(df)
    plot_tail_latency_vs_load(df)
    plot_throughput_vs_load(df)
    
    print(f"Plots saved to {PLOT_DIR}/")

if __name__ == "__main__":
    main()
