# network-monitor

A lightweight terminal dashboard that keeps an eye on a NIC and a handful of connectivity probes. It polls an interface for link state and error counters, pings a set of hosts, and performs DNS lookups, rendering the status as a coloured history bar so you can glance at a terminal and see if packets are flowing.

## Configuration

`network-monitor` expects a TOML configuration file. You can point to one explicitly with `--config <path>`, or it will look for one of:

- `~/.config/network-monitor/config.toml`
- `./network-monitor.toml`
- `./config.toml`
- `./tools/network-monitor/config.toml`

A starter configuration lives in [`tools/network-monitor/config.example.toml`](config.example.toml). Copy it to one of the recognised locations and adjust the host addresses for your environment.

Key options:

- `interval_ms`: refresh interval in milliseconds (default `2000`).
- `history_length`: how many samples to keep per target (controls the width of the Unicode history bar).
- `interface`: NIC name. If omitted, the tool picks the first non-loopback interface reporting `operstate=up`.
- `targets`: list of probes. Two kinds are supported today:
  - `icmp`: runs `ping -c 1` against `address`. Optional `timeout_ms` controls the deadline.
  - `dns`: performs a lookup for `query`. Optional `resolver` (IP or `[ip]:port`) overrides the system resolver, `record_type` selects the RR type (defaults to `A`).
