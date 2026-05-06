# CLAUDE.md - Victron Monitor

This Rust application receives Victron monitoring data via UDP and exports it as Prometheus metrics.

## Overview

The app listens for JSON data sent by Node-RED (configured to send once per minute) and converts Victron device metrics into Prometheus-compatible format with proper naming conventions and labels.

## Architecture

### Data Flow
1. Node-RED sends JSON data to UDP port (configurable)
2. App receives and parses JSON data
3. Metrics are mapped to Prometheus format with snake_case naming
4. Prometheus scrapes metrics from HTTP endpoint (port 9099 by default)

### Metric Naming Convention

Victron metric names are converted as follows:
- Device types become metric prefixes: `inverter_`, `battery_`, `charger_`
- Measurements become metric names in snake_case
- Units are included as suffixes: `_watts`, `_volts_dc`, `_percent`, `_amps`
- Device specifics and phases become labels

Examples:
- `"MultiPlus-II 48/5000/70-50 - Output power phase 1 (W)"` → `inverter_output_power_watts{device="multiplus_ii_48_5000_70_50", phase="1"}`
- `"CAN-SMARTBMS-BAT - State of charge (%)"` → `battery_state_of_charge_percent{device="can_smartbms_bat"}`
- `"SmartSolar MPPT VE.Can 150/100 - PV Power (W)"` → `charger_pv_power_watts{device="smartsolar_mppt_ve_can_150_100"}`

### Device Type Mapping
- `MultiPlus-II` → `inverter`
- `CAN-SMARTBMS-BAT` → `battery`
- `SmartSolar MPPT` → `charger`

## Configuration

The app uses a configuration file (config.toml) with the following options:
```toml
udp_port = 9099        # Port to listen for UDP data
prometheus_port = 9099 # Port to expose Prometheus metrics
```

## Error Handling

- The app is designed to be infallible where possible
- Uses `anyhow` for error handling
- Relies on systemd to restart on failures
- Invalid/missing data points are ignored without crashing

## Dependencies

Key crates:
- `tokio` - Async runtime
- `serde_json` - JSON parsing
- `prometheus` - Metrics exporter
- `anyhow` - Error handling
- `config` - Configuration management

## Deployment

The app is stateless and can be restarted at any time without data loss. It updates metrics whenever new data arrives, handling unreliable network connections gracefully.