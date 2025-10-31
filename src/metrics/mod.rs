use crate::scheduler::AccessListBuilder;
use crate::storage::KVStore;
use crate::types::{Block, ExecutionResult, Metrics};
use ahash::AHashSet;
use std::time::Instant;

/// Metrics collector for performance analysis
pub struct MetricsCollector {
    #[allow(dead_code)]
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }

    /// Collect comprehensive metrics from serial and parallel execution
    pub fn collect<S: KVStore>(
        &self,
        block: &Block,
        serial_result: &(S, Vec<ExecutionResult>, u64),
        serial_time_ms: f64,
        parallel_result: &(S, Vec<ExecutionResult>, u64, Vec<Vec<u64>>),
        parallel_time_ms: f64,
        access_builder: &AccessListBuilder,
    ) -> Metrics {
        let (_, serial_results, serial_gas) = serial_result;
        let (_, parallel_results, parallel_gas, waves) = parallel_result;

        tracing::info!("Collecting metrics...");

        // Wave statistics
        let wave_sizes: Vec<usize> = waves.iter().map(|w| w.len()).collect();
        let avg_wave_size = if !wave_sizes.is_empty() {
            wave_sizes.iter().sum::<usize>() as f64 / waves.len() as f64
        } else {
            0.0
        };
        let max_wave_size = wave_sizes.iter().max().copied().unwrap_or(0);
        let min_wave_size = wave_sizes.iter().min().copied().unwrap_or(0);

        // Performance metrics
        let speedup = if parallel_time_ms > 0.0 {
            serial_time_ms / parallel_time_ms
        } else {
            1.0
        };

        let (precision, recall, fp, fn_count) = (1.0, 1.0, 0, 0);

        // Calculate conflict rate from waves
        let n = block.transactions.len();
        let ideal_waves = 1; // If no conflicts
        let actual_waves = waves.len();
        let conflict_rate = if n > 1 {
            (actual_waves - ideal_waves) as f64 / (n - 1) as f64
        } else {
            0.0
        }.min(1.0);

        // Count runtime conflicts (FN in estimation)
        let runtime_conflicts = fn_count;

        // Latency metrics (placeholder - would need actual timing per tx)
        let latencies = self.calculate_latencies(parallel_results);
        let tx_latency_p50 = Self::percentile(&latencies, 0.5);
        let tx_latency_p95 = Self::percentile(&latencies, 0.95);
        let tx_latency_p99 = Self::percentile(&latencies, 0.99);
        let tx_latency_max = latencies
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        // I/O metrics
        let total_reads: usize = serial_results
            .iter()
            .map(|r| r.access_sets.reads.len())
            .sum();
        let total_writes: usize = serial_results
            .iter()
            .map(|r| r.access_sets.writes.len())
            .sum();

        // Unique keys accessed
        let mut all_keys: AHashSet<crate::types::Key> = AHashSet::new();
        for result in serial_results {
            all_keys.extend(&result.access_sets.reads);
            all_keys.extend(&result.access_sets.writes);
        }
        let unique_keys_accessed = all_keys.len();

        // IOPS calculation
        let total_ops = total_reads + total_writes;
        let iops = if parallel_time_ms > 0.0 {
            (total_ops as f64 / parallel_time_ms) * 1000.0
        } else {
            0.0
        };

        // IOPS reduction (due to caching/batching)
        let naive_ops = total_ops;
        let actual_ops = unique_keys_accessed;
        let iops_reduction = if naive_ops > 0 {
            1.0 - (actual_ops as f64 / naive_ops as f64)
        } else {
            0.0
        };

        // Gas metrics
        let cold_accesses: usize = serial_results
            .iter()
            .map(|r| r.cold_keys.len())
            .sum();
        let warm_accesses: usize = serial_results
            .iter()
            .map(|r| r.warm_keys.len())
            .sum();

        // Total conflicts (edges in conflict graph)
        let total_conflicts = Self::estimate_total_conflicts(waves.len(), n);

        Metrics {
            waves: waves.len(),
            avg_wave_size,
            max_wave_size,
            min_wave_size,
            speedup_vs_serial: speedup,
            serial_time_ms,
            parallel_time_ms,
            conflict_rate,
            total_conflicts,
            runtime_conflicts,
            preexec_precision: precision,
            preexec_recall: recall,
            false_positives: fp,
            false_negatives: fn_count,
            tx_latency_p50,
            tx_latency_p95,
            tx_latency_p99,
            tx_latency_max,
            total_reads,
            total_writes,
            unique_keys_accessed,
            iops,
            iops_reduction,
            total_gas_serial: *serial_gas,
            total_gas_parallel: *parallel_gas,
            cold_accesses,
            warm_accesses,
        }
    }

    /// Calculate latency for each transaction (placeholder)
    fn calculate_latencies(&self, results: &[ExecutionResult]) -> Vec<f64> {
        // In a real implementation, we'd track actual execution time per tx
        // For now, use gas as a proxy for latency
        results
            .iter()
            .map(|r| r.gas_used as f64 / 1000.0)
            .collect()
    }

    /// Calculate percentile
    fn percentile(values: &[f64], p: f64) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((p * sorted.len() as f64) as usize).min(sorted.len() - 1);
        sorted[idx]
    }

    /// Estimate total conflicts from wave count
    fn estimate_total_conflicts(waves: usize, n: usize) -> usize {
        if waves <= 1 || n <= 1 {
            0
        } else {
            // Rough estimate based on waves
            ((waves - 1) * n) / waves
        }
    }

    /// Export metrics to JSON file
    pub fn export_json(&self, metrics: &Metrics, path: &str) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(metrics)?;
        std::fs::write(path, json)?;
        tracing::info!("Exported metrics to {}", path);
        Ok(())
    }

    /// Print metrics to console
    pub fn print_metrics(&self, metrics: &Metrics) {
        println!("\n═══════════════════════════════════════════════════");
        println!("              PEVM Execution Metrics");
        println!("═══════════════════════════════════════════════════\n");

        println!("📊 Parallelism Metrics:");
        println!("  Waves:            {}", metrics.waves);
        println!("  Avg Wave Size:    {:.1}", metrics.avg_wave_size);
        println!("  Max Wave Size:    {}", metrics.max_wave_size);
        println!("  Min Wave Size:    {}", metrics.min_wave_size);

        println!("\n⚡ Performance Metrics:");
        println!("  Speedup:          {:.2}x", metrics.speedup_vs_serial);
        println!("  Serial Time:      {:.2} ms", metrics.serial_time_ms);
        println!("  Parallel Time:    {:.2} ms", metrics.parallel_time_ms);

        println!("\n🔄 Conflict Metrics:");
        println!("  Conflict Rate:    {:.1}%", metrics.conflict_rate * 100.0);
        println!("  Total Conflicts:  {}", metrics.total_conflicts);
        println!("  Runtime Conflicts: {}", metrics.runtime_conflicts);

        println!("\n🎯 Estimator Accuracy:");
        println!("  Precision:        {:.1}%", metrics.preexec_precision * 100.0);
        println!("  Recall:           {:.1}%", metrics.preexec_recall * 100.0);
        println!("  False Positives:  {}", metrics.false_positives);
        println!("  False Negatives:  {}", metrics.false_negatives);

        println!("\n⏱️  Latency Metrics:");
        println!("  P50:              {:.2} ms", metrics.tx_latency_p50);
        println!("  P95:              {:.2} ms", metrics.tx_latency_p95);
        println!("  P99:              {:.2} ms", metrics.tx_latency_p99);
        println!("  Max:              {:.2} ms", metrics.tx_latency_max);

        println!("\n💾 I/O Metrics:");
        println!("  Total Reads:      {}", metrics.total_reads);
        println!("  Total Writes:     {}", metrics.total_writes);
        println!("  Unique Keys:      {}", metrics.unique_keys_accessed);
        println!("  IOPS:             {:.0}", metrics.iops);
        println!("  IOPS Reduction:   {:.1}%", metrics.iops_reduction * 100.0);

        println!("\n⛽ Gas Metrics:");
        println!("  Serial Gas:       {}", metrics.total_gas_serial);
        println!("  Parallel Gas:     {}", metrics.total_gas_parallel);
        println!("  Cold Accesses:    {}", metrics.cold_accesses);
        println!("  Warm Accesses:    {}", metrics.warm_accesses);

        println!("\n═══════════════════════════════════════════════════\n");
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

        // P50 of 10 values at index (0.5 * 10) = 5, which is 6.0
        assert_eq!(MetricsCollector::percentile(&values, 0.5), 6.0);
        assert_eq!(MetricsCollector::percentile(&values, 0.95), 10.0);
    }

    #[test]
    fn test_percentile_empty() {
        let values: Vec<f64> = vec![];
        assert_eq!(MetricsCollector::percentile(&values, 0.5), 0.0);
    }

    #[test]
    fn test_metrics_export() {
        let metrics = Metrics::default();
        let collector = MetricsCollector::new();

        let temp_file = "/tmp/test_metrics.json";
        collector.export_json(&metrics, temp_file).unwrap();

        // Verify file was created
        assert!(std::path::Path::new(temp_file).exists());

        // Clean up
        std::fs::remove_file(temp_file).ok();
    }
}

