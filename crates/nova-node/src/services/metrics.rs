//! Prometheus metrik servisi

use super::{NodeService, ServiceStatus};
use crate::errors::NodeResult;
use dashmap::DashMap;
use std::sync::Arc;

// ─── MetricValue ──────────────────────────────────────────────────────────────

/// Metrik tipi
#[derive(Debug, Clone)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram { sum: f64, count: u64 },
}

impl MetricValue {
    pub fn as_f64(&self) -> f64 {
        match self {
            MetricValue::Counter(v) => *v as f64,
            MetricValue::Gauge(v) => *v,
            MetricValue::Histogram { sum, count } => {
                if *count > 0 { sum / *count as f64 } else { 0.0 }
            }
        }
    }
}

// ─── MetricsService ───────────────────────────────────────────────────────────

/// Prometheus tarzı metrik servisi
pub struct MetricsService {
    status: ServiceStatus,
    metrics: Arc<DashMap<String, MetricValue>>,
    port: u16,
}

impl MetricsService {
    pub fn new(port: u16) -> Self {
        Self {
            status: ServiceStatus::Stopped,
            metrics: Arc::new(DashMap::new()),
            port,
        }
    }

    /// Counter arttır
    pub fn inc_counter(&self, name: impl Into<String>, delta: u64) {
        let key = name.into();
        match self.metrics.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                if let MetricValue::Counter(v) = e.get_mut() {
                    *v += delta;
                }
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(MetricValue::Counter(delta));
            }
        }
    }

    /// Gauge ayarla
    pub fn set_gauge(&self, name: impl Into<String>, value: f64) {
        self.metrics.insert(name.into(), MetricValue::Gauge(value));
    }

    /// Histogram kaydet
    pub fn observe_histogram(&self, name: impl Into<String>, value: f64) {
        let key = name.into();
        match self.metrics.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                if let MetricValue::Histogram { sum, count } = e.get_mut() {
                    *sum += value;
                    *count += 1;
                }
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(MetricValue::Histogram { sum: value, count: 1 });
            }
        }
    }

    /// Metrik değeri oku
    pub fn get(&self, name: &str) -> Option<MetricValue> {
        self.metrics.get(name).map(|v| v.clone())
    }

    /// Tüm metrikleri Prometheus formatında döndür (mock)
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();
        let mut keys: Vec<String> = self.metrics.iter().map(|e| e.key().clone()).collect();
        keys.sort();
        for key in keys {
            if let Some(val) = self.metrics.get(&key) {
                match val.value() {
                    MetricValue::Counter(v) => {
                        output.push_str(&format!("# TYPE {} counter\n{} {}\n", key, key, v));
                    }
                    MetricValue::Gauge(v) => {
                        output.push_str(&format!("# TYPE {} gauge\n{} {}\n", key, key, v));
                    }
                    MetricValue::Histogram { sum, count } => {
                        output.push_str(&format!(
                            "# TYPE {} histogram\n{}_sum {}\n{}_count {}\n",
                            key, key, sum, key, count
                        ));
                    }
                }
            }
        }
        output
    }

    pub fn metric_count(&self) -> usize { self.metrics.len() }
    pub fn port(&self) -> u16 { self.port }
}

impl NodeService for MetricsService {
    fn name(&self) -> &'static str { "MetricsService" }

    fn start(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Starting;
        // Temel metrikleri kaydet
        self.set_gauge("nova_node_up", 1.0);
        self.inc_counter("nova_node_starts_total", 1);
        self.status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Stopping;
        self.set_gauge("nova_node_up", 0.0);
        self.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> &ServiceStatus { &self.status }

    fn health_check(&self) -> bool {
        self.status.is_running()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_lifecycle() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();
        assert!(svc.health_check());
        assert_eq!(svc.port(), 9_615);
        svc.stop().unwrap();
        assert!(!svc.health_check());
    }

    #[test]
    fn test_metrics_counter() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();

        svc.inc_counter("test_counter", 5);
        svc.inc_counter("test_counter", 3);
        let val = svc.get("test_counter").unwrap();
        assert_eq!(val.as_f64(), 8.0);
    }

    #[test]
    fn test_metrics_gauge() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();

        svc.set_gauge("test_gauge", 42.5);
        let val = svc.get("test_gauge").unwrap();
        assert_eq!(val.as_f64(), 42.5);
    }

    #[test]
    fn test_metrics_histogram() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();

        svc.observe_histogram("block_time_ms", 350.0);
        svc.observe_histogram("block_time_ms", 450.0);
        let val = svc.get("block_time_ms").unwrap();
        assert_eq!(val.as_f64(), 400.0); // ortalama
    }

    #[test]
    fn test_metrics_prometheus_export() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();

        svc.inc_counter("blocks_total", 100);
        svc.set_gauge("peer_count", 25.0);

        let output = svc.export_prometheus();
        assert!(output.contains("blocks_total"));
        assert!(output.contains("peer_count"));
        assert!(output.contains("counter"));
        assert!(output.contains("gauge"));
    }

    #[test]
    fn test_metrics_start_sets_node_up() {
        let mut svc = MetricsService::new(9_615);
        svc.start().unwrap();

        let up = svc.get("nova_node_up").unwrap();
        assert_eq!(up.as_f64(), 1.0);
    }
}
