# Victron Monitor

A Rust application that receives Victron Energy monitoring data via UDP and exports it as Prometheus metrics.

## Overview

This service acts as a bridge between Victron Energy systems (via Node-RED) and Prometheus monitoring. It listens for JSON data on a UDP port and converts Victron-specific metric names into Prometheus-compatible format with appropriate labels.

## Installation

```bash
cargo build --release
```

## Configuration

Create a `config.toml` file in the working directory:

```toml
udp_port = 9099        # Port to listen for UDP data
prometheus_port = 9099 # Port to expose Prometheus metrics

# Device type mappings
[device_mappings]
"MultiPlus-II" = "inverter"
"CAN-SMARTBMS-BAT" = "battery"
"SmartSolar MPPT" = "charger"

# Unit mappings (from Victron notation to Prometheus suffix)
[unit_mappings]
"W" = "watts"
"A" = "amps"
"V" = "volts"
"V DC" = "volts_dc"
"VAC" = "volts_ac"
"%" = "percent"
```

## Usage

1. Configure Node-RED to send Victron data as JSON to the UDP port
2. Run the application: `./victron-monitor`
3. Access Prometheus metrics at `http://localhost:9099/metrics`

## Metric Format

Victron metrics are converted following these rules:

- Device types become metric prefixes (`inverter_`, `battery_`, `charger_`)
- Measurements are converted to snake_case
- Units are appended as suffixes (`_watts`, `_volts_dc`, etc.)
- Device models and phases become labels

### Examples

| Victron Metric | Prometheus Metric |
|----------------|-------------------|
| `MultiPlus-II 48/5000/70-50 - Output power phase 1 (W)` | `inverter_output_power_watts{device="multiplus_ii_48_5000_70_50", phase="1"}` |
| `CAN-SMARTBMS-BAT - State of charge (%)` | `battery_state_of_charge_percent{device="can_smartbms_bat"}` |
| `SmartSolar MPPT VE.Can 150/100 - PV Power (W)` | `charger_pv_power_watts{device="smartsolar_mppt_ve_can_150_100"}` |

## Testing

Run tests with:

```bash
cargo test
```

The test suite includes parsing verification using the `example.json` file.

## License

This project is part of a personal NixOS configuration repository.