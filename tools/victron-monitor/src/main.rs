use anyhow::{anyhow, bail, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use clap::Parser;
use config::Config;
use prometheus::{GaugeVec, Opts, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::{Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

#[derive(Parser)]
#[command(name = "victron-monitor")]
#[command(about = "Victron monitoring data UDP receiver and Prometheus exporter")]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct AppConfig {
    udp_port: u16,
    prometheus_port: u16,
    device_mappings: HashMap<String, String>,
    unit_mappings: HashMap<String, String>,
}

const MAX_SERIES: usize = 1024;
const MAX_METRIC_KEY_BYTES: usize = 256;

#[derive(Default)]
struct Metrics {
    registry: Registry,
    gauges: HashMap<String, GaugeVec>,
    series: HashSet<(String, Vec<String>)>,
}

impl Metrics {
    fn update(&mut self, data: HashMap<String, Value>, config: &AppConfig) -> Result<()> {
        for (key, value) in data {
            let value = match value {
                Value::Bool(value) => f64::from(value),
                Value::Number(value) => value.as_f64().ok_or(anyhow!("Invalid number"))?,
                _ => bail!("Expected numeric or boolean measurement"),
            };
            let Some((name, labels)) = parse_metric_name(&key, config) else {
                continue;
            };
            let mut labels: Vec<_> = labels.into_iter().collect();
            labels.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            let label_values: Vec<_> = labels.iter().map(|(_, value)| value.clone()).collect();
            let series = (name.clone(), label_values);
            if !self.series.contains(&series) && self.series.len() >= MAX_SERIES {
                bail!("Metric series limit reached");
            }
            // Construct and validate before registering; malformed input must
            // neither panic nor grow the registry on a failed update.
            if let Some(gauge) = self.gauges.get(&name) {
                let values: Vec<_> = labels.iter().map(|(_, value)| value.as_str()).collect();
                gauge.get_metric_with_label_values(&values)?.set(value);
            } else {
                let names: Vec<_> = labels.iter().map(|(key, _)| key.as_str()).collect();
                let gauge = GaugeVec::new(Opts::new(&name, "Victron monitoring metric"), &names)?;
                let values: Vec<_> = labels.iter().map(|(_, value)| value.as_str()).collect();
                gauge.get_metric_with_label_values(&values)?.set(value);
                self.registry.register(Box::new(gauge.clone()))?;
                self.gauges.insert(name, gauge);
            }
            self.series.insert(series);
        }
        Ok(())
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "victron_monitor=info".into()),
        )
        .init();

    // Load configuration
    let config = Config::builder()
        .add_source(config::File::with_name(&args.config))
        .build()?;

    let app_config: AppConfig = config.try_deserialize()?;
    info!(
        "Configuration loaded: UDP port {}, Prometheus port {}",
        app_config.udp_port, app_config.prometheus_port
    );

    let config = Arc::new(app_config);

    let metrics = Arc::new(RwLock::new(Metrics::default()));

    // Start Prometheus HTTP server
    let http_addr = SocketAddr::from(([127, 0, 0, 1], config.prometheus_port));

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(metrics.clone());

    info!("Starting Prometheus metrics server on {}", http_addr);

    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    tokio::select! {
        result = udp_listener(config, metrics) => result?,
        result = axum::serve(listener, app) => result?,
    }

    Ok(())
}

async fn udp_listener(config: Arc<AppConfig>, metrics: Arc<RwLock<Metrics>>) -> Result<()> {
    let addr = SocketAddr::from((Ipv6Addr::UNSPECIFIED, config.udp_port));
    let socket = UdpSocket::bind(addr).await?;
    info!("UDP listener started on {}", addr);

    let mut buf = vec![0u8; 65536];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                debug!("Received {} bytes from {}", len, addr);

                if let Ok(data) = serde_json::from_slice::<HashMap<String, Value>>(&buf[..len]) {
                    if let Err(e) = metrics.write().await.update(data, &config) {
                        debug!("Rejected metrics: {}", e);
                    }
                } else {
                    debug!("Rejected invalid JSON from {}", addr);
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
}

fn parse_metric_name(
    raw_name: &str,
    config: &AppConfig,
) -> Option<(String, HashMap<String, String>)> {
    if raw_name.len() > MAX_METRIC_KEY_BYTES {
        return None;
    }
    // Split by " - " to separate device from measurement
    let parts: Vec<&str> = raw_name.split(" - ").collect();
    if parts.len() != 2 {
        return None;
    }

    let device_part = parts[0];
    let measurement_part = parts[1];

    // Find device type
    let mut device_type = None;
    let device_model = device_part.to_lowercase().replace([' ', '-', '/'], "_");

    for (pattern, mapped_type) in &config.device_mappings {
        if device_part.contains(pattern) {
            device_type = Some(mapped_type.clone());
            break;
        }
    }

    let device_type = device_type?;

    // Parse measurement and unit
    let (measurement, unit) = if let Some(idx) = measurement_part.rfind(" (") {
        let measurement = &measurement_part[..idx];
        let unit_part = measurement_part.get(idx + 2..)?.strip_suffix(')')?;
        (measurement, Some(unit_part))
    } else {
        (measurement_part, None)
    };

    // Convert measurement to snake_case
    let mut metric_name = measurement
        .to_lowercase()
        .replace(' ', "_")
        .replace([';', ','], "");
    if metric_name.is_empty()
        || !metric_name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
    {
        return None;
    }

    // Build full metric name
    let mut full_metric_name = format!("{}_{}", device_type, metric_name);

    // Add unit suffix
    if let Some(unit) = unit {
        if let Some(mapped_unit) = config.unit_mappings.get(unit) {
            full_metric_name.push('_');
            full_metric_name.push_str(mapped_unit);
        }
    }

    // Extract labels
    let mut labels = HashMap::new();
    labels.insert("device".to_string(), device_model);

    // Check for phase number
    if let Some(phase_match) = metric_name.find("phase_") {
        if let Some(phase_num) = metric_name.chars().nth(phase_match + 6) {
            if phase_num.is_ascii_digit() {
                labels.insert("phase".to_string(), phase_num.to_string());
                // Remove phase from metric name
                metric_name = metric_name.replace(&format!("_phase_{}", phase_num), "");
                full_metric_name = format!("{}_{}", device_type, metric_name);
                if let Some(unit) = unit {
                    if let Some(mapped_unit) = config.unit_mappings.get(unit) {
                        full_metric_name.push('_');
                        full_metric_name.push_str(mapped_unit);
                    }
                }
            }
        }
    }

    Some((full_metric_name, labels))
}

async fn metrics_handler(
    State(metrics): State<Arc<RwLock<Metrics>>>,
) -> Result<String, StatusCode> {
    let families = metrics.read().await.registry.gather();
    TextEncoder::new()
        .encode_to_string(&families)
        .map_err(|error| {
            error!(%error, "Cannot encode metrics");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AppConfig {
        let mut device_mappings = HashMap::new();
        device_mappings.insert("MultiPlus-II".to_string(), "inverter".to_string());
        device_mappings.insert("CAN-SMARTBMS-BAT".to_string(), "battery".to_string());
        device_mappings.insert("SmartSolar MPPT".to_string(), "charger".to_string());

        let mut unit_mappings = HashMap::new();
        unit_mappings.insert("W".to_string(), "watts".to_string());
        unit_mappings.insert("A".to_string(), "amps".to_string());
        unit_mappings.insert("V".to_string(), "volts".to_string());
        unit_mappings.insert("V DC".to_string(), "volts_dc".to_string());
        unit_mappings.insert("VAC".to_string(), "volts_ac".to_string());
        unit_mappings.insert("%".to_string(), "percent".to_string());

        AppConfig {
            udp_port: 9099,
            prometheus_port: 9099,
            device_mappings,
            unit_mappings,
        }
    }

    #[test]
    fn test_parse_example_json() {
        let json_content = include_str!("../example.json");

        let data: HashMap<String, f64> =
            serde_json::from_str(json_content).expect("Failed to parse example.json");

        let config = test_config();

        // Test each metric in the example
        let expected_metrics = vec![
            (
                "MultiPlus-II 48/5000/70-50 - Output power phase 1 (W)",
                "inverter_output_power_watts",
                vec![("device", "multiplus_ii_48_5000_70_50"), ("phase", "1")],
            ),
            (
                "CAN-SMARTBMS-BAT - Battery current (A)",
                "battery_battery_current_amps",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - System; maximum cell voltage (V DC)",
                "battery_system_maximum_cell_voltage_volts_dc",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - System; minimum cell voltage (V DC)",
                "battery_system_minimum_cell_voltage_volts_dc",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - Number of online modules",
                "battery_number_of_online_modules",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - Min discharge voltage (V DC)",
                "battery_min_discharge_voltage_volts_dc",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - State of health (%)",
                "battery_state_of_health_percent",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "CAN-SMARTBMS-BAT - State of charge (%)",
                "battery_state_of_charge_percent",
                vec![("device", "can_smartbms_bat")],
            ),
            (
                "SmartSolar MPPT VE.Can 150/100 - PV voltage",
                "charger_pv_voltage",
                vec![("device", "smartsolar_mppt_ve.can_150_100")],
            ),
            (
                "SmartSolar MPPT VE.Can 150/100 - PV Power (W)",
                "charger_pv_power_watts",
                vec![("device", "smartsolar_mppt_ve.can_150_100")],
            ),
            (
                "MultiPlus-II 48/5000/70-50 - Input voltage phase 1 (VAC)",
                "inverter_input_voltage_volts_ac",
                vec![("device", "multiplus_ii_48_5000_70_50"), ("phase", "1")],
            ),
        ];

        for (raw_name, expected_metric, expected_labels) in expected_metrics {
            let result = parse_metric_name(raw_name, &config);
            assert!(result.is_some(), "Failed to parse metric: {}", raw_name);

            let (metric_name, labels) = result.unwrap();
            assert_eq!(
                metric_name, expected_metric,
                "Metric name mismatch for {}",
                raw_name
            );

            for (label_key, label_value) in expected_labels {
                assert_eq!(
                    labels.get(label_key).map(|s| s.as_str()),
                    Some(label_value),
                    "Label mismatch for {} in metric {}",
                    label_key,
                    raw_name
                );
            }
        }

        // Verify all metrics from example.json can be parsed
        for key in data.keys() {
            let result = parse_metric_name(key, &config);
            assert!(
                result.is_some(),
                "Failed to parse metric from example.json: {}",
                key
            );
        }
    }

    #[test]
    fn test_phase_extraction() {
        let config = test_config();

        let (metric_name, labels) = parse_metric_name(
            "MultiPlus-II 48/5000/70-50 - Output power phase 1 (W)",
            &config,
        )
        .unwrap();

        assert_eq!(metric_name, "inverter_output_power_watts");
        assert_eq!(labels.get("phase"), Some(&"1".to_string()));
        assert_eq!(
            labels.get("device"),
            Some(&"multiplus_ii_48_5000_70_50".to_string())
        );
    }
    #[test]
    fn malformed_measurements_do_not_panic() {
        let config = test_config();
        for raw in [
            "MultiPlus-II - Power (",
            "MultiPlus-II - Power (é",
            "MultiPlus-II - !invalid",
            "unknown - Power (W)",
            "MultiPlus-II - phase_é",
            "MultiPlus-II - ",
        ] {
            assert!(parse_metric_name(raw, &config).is_none(), "{raw}");
        }
        assert!(parse_metric_name(&"a".repeat(MAX_METRIC_KEY_BYTES + 1), &config).is_none());
    }

    #[test]
    fn series_limit_bounds_metrics_and_labels() {
        let config = test_config();
        let mut metrics = Metrics::default();
        for index in 0..MAX_SERIES {
            metrics
                .update(
                    HashMap::from([(
                        format!("MultiPlus-II {index} - Power (W)"),
                        Value::from(index),
                    )]),
                    &config,
                )
                .unwrap();
        }
        assert!(metrics
            .update(
                HashMap::from([("MultiPlus-II extra - Power (W)".to_string(), Value::from(1)),]),
                &config
            )
            .is_err());
        assert!(metrics
            .update(
                HashMap::from([("MultiPlus-II - New metric (W)".to_string(), Value::from(1)),]),
                &config
            )
            .is_err());
        metrics
            .update(
                HashMap::from([("MultiPlus-II 0 - Power (W)".to_string(), Value::from(2))]),
                &config,
            )
            .unwrap();
        assert_eq!(metrics.series.len(), MAX_SERIES);
        assert_eq!(metrics.gauges.len(), 1);
    }

    #[test]
    fn conflicting_labels_return_an_error() {
        let config = test_config();
        let mut metrics = Metrics::default();
        metrics
            .update(
                HashMap::from([("MultiPlus-II - Power (W)".to_string(), Value::from(1))]),
                &config,
            )
            .unwrap();
        assert!(metrics
            .update(
                HashMap::from([(
                    "MultiPlus-II - Power phase 1 (W)".to_string(),
                    Value::from(2)
                ),]),
                &config
            )
            .is_err());
        assert_eq!(metrics.series.len(), 1);
    }
}
