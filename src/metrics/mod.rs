use crate::evm::SerialExecutionResult;
use crate::info;
use crate::scheduler::{AccessListBuilder, ParallelExecutionResult};
use crate::storage::KVStore;
use crate::types::{Block, Metrics};

pub struct MetricsCollector;

impl MetricsCollector {
    pub fn new() -> Self {
        Self
    }

    pub fn collect<S: KVStore>(
        &self,
        block: &Block,
        _serial_result: &SerialExecutionResult<S>,
        serial_time_ms: f64,
        parallel_result: &ParallelExecutionResult,
        parallel_time_ms: f64,
        access_builder: &AccessListBuilder,
    ) -> Metrics {
        let results = &parallel_result.results;
        let waves = &parallel_result.waves;

        let avg_wave_size = if !waves.is_empty() {
            block.transactions.len() as f64 / waves.len() as f64
        } else {
            0.0
        };

        let speedup = if parallel_time_ms > 0.0 {
            serial_time_ms / parallel_time_ms
        } else {
            1.0
        };

        let (precision, recall, fp, fn_count) =
            Self::calculate_preexec_accuracy(&block.transactions, access_builder, results);

        let conflict_rate = Self::calculate_conflict_rate(results);

        let (tx_latency_p50, tx_latency_p95, tx_latency_p99) =
            Self::calculate_latencies(waves, parallel_time_ms);

        let (total_reads, total_writes) = results.iter().fold((0, 0), |(reads, writes), r| {
            (
                reads + r.access_sets.reads.len(),
                writes + r.access_sets.writes.len(),
            )
        });

        let iops = if parallel_time_ms > 0.0 {
            ((total_reads + total_writes) as f64 / parallel_time_ms) * 1000.0
        } else {
            0.0
        };

        Metrics {
            waves: waves.len(),
            avg_wave_size,
            speedup_vs_serial: speedup,
            conflict_rate,
            preexec_precision: precision,
            preexec_recall: recall,
            false_positives: fp,
            false_negatives: fn_count,
            tx_latency_p50,
            tx_latency_p95,
            tx_latency_p99,
            iops,
        }
    }

    fn calculate_latencies(waves: &[Vec<u64>], parallel_time_ms: f64) -> (f64, f64, f64) {
        if waves.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let avg_wave_time = parallel_time_ms / waves.len() as f64;
        let total_txs: usize = waves.iter().map(|w| w.len()).sum();
        let mut latencies = Vec::with_capacity(total_txs);

        for (wave_idx, wave) in waves.iter().enumerate() {
            let latency = (wave_idx + 1) as f64 * avg_wave_time;
            latencies.extend(std::iter::repeat_n(latency, wave.len()));
        }

        latencies.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p50_idx = (0.5 * latencies.len() as f64) as usize;
        let p95_idx = (0.95 * latencies.len() as f64) as usize;
        let p99_idx = (0.99 * latencies.len() as f64) as usize;

        let p50 = latencies
            .get(p50_idx.min(latencies.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p95 = latencies
            .get(p95_idx.min(latencies.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p99 = latencies
            .get(p99_idx.min(latencies.len() - 1))
            .copied()
            .unwrap_or(0.0);

        (p50, p95, p99)
    }

    fn calculate_conflict_rate(results: &[crate::types::ExecutionResult]) -> f64 {
        if results.len() <= 1 {
            return 0.0;
        }

        let n = results.len();
        let total_pairs = n * (n - 1) / 2;
        let mut conflicts = 0;

        for i in 0..n {
            for j in (i + 1)..n {
                if results[i]
                    .access_sets
                    .has_conflict_with(&results[j].access_sets)
                {
                    conflicts += 1;
                }
            }
        }

        conflicts as f64 / total_pairs as f64
    }

    fn calculate_preexec_accuracy(
        transactions: &[crate::types::Transaction],
        access_builder: &AccessListBuilder,
        actual_results: &[crate::types::ExecutionResult],
    ) -> (f64, f64, usize, usize) {
        use ahash::AHashMap;

        let actual_map: AHashMap<u64, &crate::types::ExecutionResult> =
            actual_results.iter().map(|r| (r.tx_id, r)).collect();

        let mut tp = 0;
        let mut fp = 0;
        let mut fn_count = 0;

        for i in 0..transactions.len() {
            for j in (i + 1)..transactions.len() {
                if let (Some(est_i), Some(est_j)) = (
                    access_builder.get_estimated(transactions[i].id),
                    access_builder.get_estimated(transactions[j].id),
                ) {
                    if let (Some(act_i), Some(act_j)) = (
                        actual_map.get(&transactions[i].id),
                        actual_map.get(&transactions[j].id),
                    ) {
                        match (
                            est_i.has_conflict_with(est_j),
                            act_i.access_sets.has_conflict_with(&act_j.access_sets),
                        ) {
                            (true, true) => tp += 1,
                            (true, false) => fp += 1,
                            (false, true) => fn_count += 1,
                            _ => {}
                        }
                    }
                }
            }
        }

        let precision = if tp + fp > 0 {
            tp as f64 / (tp + fp) as f64
        } else {
            1.0
        };

        let recall = if tp + fn_count > 0 {
            tp as f64 / (tp + fn_count) as f64
        } else {
            1.0
        };

        (precision, recall, fp, fn_count)
    }

    pub fn export_json(&self, metrics: &Metrics, path: &str) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(metrics)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn print_metrics(&self, metrics: &Metrics) {
        info!("\nMetrics Summary:");
        info!("  Speedup: {:.2}x", metrics.speedup_vs_serial);
        info!("  Waves: {}", metrics.waves);
        info!("  Avg Wave Size: {:.2}", metrics.avg_wave_size);
        info!("  Conflict Rate: {:.3}%", metrics.conflict_rate * 100.0);
        info!("  Preexec Precision: {:.3}", metrics.preexec_precision);
        info!("  Preexec Recall: {:.3}", metrics.preexec_recall);
        info!("  IOPS: {:.2}", metrics.iops);
        info!(
            "  Latency P50/P95/P99: {:.2}/{:.2}/{:.2} ms",
            metrics.tx_latency_p50, metrics.tx_latency_p95, metrics.tx_latency_p99
        );
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
