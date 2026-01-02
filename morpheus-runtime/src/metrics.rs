//! Prometheus metrics for Morpheus-Hybrid observability
//!
//! Exports metrics in Prometheus text format at `/metrics` endpoint.
//!
//! ## Metrics Exported
//!
//! - `morpheus_hint_count_total{worker_id, reason}` - Hints received
//! - `morpheus_hint_drops_total` - Hints dropped (ring buffer full)  
//! - `morpheus_escalation_count_total{policy}` - Escalations performed
//! - `morpheus_defensive_mode_total{worker_id}` - Defensive mode activations
//! - `morpheus_last_ack_latency_seconds{worker_id}` - Hint acknowledgment latency

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Metrics collector for Morpheus runtime
pub struct MorpheusMetrics {
    /// Total hints received per worker, per reason
    hint_counts: RwLock<HashMap<(u32, String), AtomicU64>>,

    /// Total hints dropped (ring buffer overflow)
    hint_drops: AtomicU64,

    /// Total escalations per policy
    escalation_counts: RwLock<HashMap<String, AtomicU64>>,

    /// Defensive mode activations per worker
    defensive_mode_counts: RwLock<HashMap<u32, AtomicU64>>,

    /// Acknowledgment latency samples per worker (in nanoseconds)
    ack_latency_samples: RwLock<HashMap<u32, Vec<u64>>>,
}

impl Default for MorpheusMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl MorpheusMetrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            hint_counts: RwLock::new(HashMap::new()),
            hint_drops: AtomicU64::new(0),
            escalation_counts: RwLock::new(HashMap::new()),
            defensive_mode_counts: RwLock::new(HashMap::new()),
            ack_latency_samples: RwLock::new(HashMap::new()),
        }
    }

    /// Record a hint received
    pub fn record_hint(&self, worker_id: u32, reason: &str) {
        let mut counts = self.hint_counts.write().unwrap();
        let key = (worker_id, reason.to_string());
        counts
            .entry(key)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a hint drop
    pub fn record_hint_drop(&self) {
        self.hint_drops.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an escalation
    pub fn record_escalation(&self, policy: &str) {
        let mut counts = self.escalation_counts.write().unwrap();
        counts
            .entry(policy.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record defensive mode activation
    pub fn record_defensive_mode(&self, worker_id: u32) {
        let mut counts = self.defensive_mode_counts.write().unwrap();
        counts
            .entry(worker_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record acknowledgment latency sample
    pub fn record_ack_latency(&self, worker_id: u32, latency_ns: u64) {
        let mut samples = self.ack_latency_samples.write().unwrap();
        let worker_samples = samples.entry(worker_id).or_default();

        // Keep last 1000 samples per worker
        if worker_samples.len() >= 1000 {
            worker_samples.remove(0);
        }
        worker_samples.push(latency_ns);
    }

    /// Render metrics in Prometheus text format
    pub fn render(&self) -> String {
        let mut output = String::new();

        // Hint counts
        output.push_str(
            "# HELP morpheus_hint_count_total Total hints received by worker and reason\n",
        );
        output.push_str("# TYPE morpheus_hint_count_total counter\n");
        {
            let counts = self.hint_counts.read().unwrap();
            for ((worker_id, reason), count) in counts.iter() {
                output.push_str(&format!(
                    "morpheus_hint_count_total{{worker_id=\"{}\",reason=\"{}\"}} {}\n",
                    worker_id,
                    reason,
                    count.load(Ordering::Relaxed)
                ));
            }
        }

        // Hint drops
        output.push_str(
            "# HELP morpheus_hint_drops_total Total hints dropped due to ring buffer overflow\n",
        );
        output.push_str("# TYPE morpheus_hint_drops_total counter\n");
        output.push_str(&format!(
            "morpheus_hint_drops_total {}\n",
            self.hint_drops.load(Ordering::Relaxed)
        ));

        // Escalation counts
        output.push_str("# HELP morpheus_escalation_count_total Total escalations by policy\n");
        output.push_str("# TYPE morpheus_escalation_count_total counter\n");
        {
            let counts = self.escalation_counts.read().unwrap();
            for (policy, count) in counts.iter() {
                output.push_str(&format!(
                    "morpheus_escalation_count_total{{policy=\"{}\"}} {}\n",
                    policy,
                    count.load(Ordering::Relaxed)
                ));
            }
        }

        // Defensive mode counts
        output.push_str(
            "# HELP morpheus_defensive_mode_total Defensive mode activations by worker\n",
        );
        output.push_str("# TYPE morpheus_defensive_mode_total counter\n");
        {
            let counts = self.defensive_mode_counts.read().unwrap();
            for (worker_id, count) in counts.iter() {
                output.push_str(&format!(
                    "morpheus_defensive_mode_total{{worker_id=\"{}\"}} {}\n",
                    worker_id,
                    count.load(Ordering::Relaxed)
                ));
            }
        }

        // Ack latency histogram
        output.push_str(
            "# HELP morpheus_last_ack_latency_seconds Hint acknowledgment latency in seconds\n",
        );
        output.push_str("# TYPE morpheus_last_ack_latency_seconds histogram\n");
        {
            let samples = self.ack_latency_samples.read().unwrap();
            for (worker_id, worker_samples) in samples.iter() {
                if worker_samples.is_empty() {
                    continue;
                }

                // Calculate histogram buckets (in seconds)
                let buckets = [0.00001, 0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01];
                let mut bucket_counts = vec![0u64; buckets.len()];
                let mut sum_ns: u64 = 0;

                for &sample_ns in worker_samples.iter() {
                    sum_ns = sum_ns.saturating_add(sample_ns);
                    let sample_s = sample_ns as f64 / 1_000_000_000.0;
                    for (i, &bucket) in buckets.iter().enumerate() {
                        if sample_s <= bucket {
                            bucket_counts[i] += 1;
                        }
                    }
                }

                for (i, &bucket) in buckets.iter().enumerate() {
                    output.push_str(&format!(
                        "morpheus_last_ack_latency_seconds_bucket{{worker_id=\"{}\",le=\"{}\"}} {}\n",
                        worker_id, bucket, bucket_counts[i]
                    ));
                }
                output.push_str(&format!(
                    "morpheus_last_ack_latency_seconds_bucket{{worker_id=\"{}\",le=\"+Inf\"}} {}\n",
                    worker_id,
                    worker_samples.len()
                ));
                output.push_str(&format!(
                    "morpheus_last_ack_latency_seconds_sum{{worker_id=\"{}\"}} {}\n",
                    worker_id,
                    sum_ns as f64 / 1_000_000_000.0
                ));
                output.push_str(&format!(
                    "morpheus_last_ack_latency_seconds_count{{worker_id=\"{}\"}} {}\n",
                    worker_id,
                    worker_samples.len()
                ));
            }
        }

        output
    }
}

/// Global metrics instance
static METRICS: std::sync::OnceLock<MorpheusMetrics> = std::sync::OnceLock::new();

/// Get the global metrics instance
pub fn metrics() -> &'static MorpheusMetrics {
    METRICS.get_or_init(MorpheusMetrics::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let m = MorpheusMetrics::new();

        m.record_hint(0, "budget");
        m.record_hint(0, "budget");
        m.record_hint(1, "pressure");
        m.record_hint_drop();
        m.record_escalation("thread_kick");
        m.record_defensive_mode(0);
        m.record_ack_latency(0, 50_000); // 50µs

        let output = m.render();
        assert!(output.contains("morpheus_hint_count_total"));
        assert!(output.contains("morpheus_hint_drops_total"));
        assert!(output.contains("morpheus_escalation_count_total"));
        assert!(output.contains("morpheus_defensive_mode_total"));
        assert!(output.contains("morpheus_last_ack_latency_seconds"));
    }
}
