# airgradient

A self-hosted Rust application that monitors [AirGradient](https://www.airgradient.com/) air quality sensors, stores historical data using RRD (Round Robin Database), and provides a web UI for configuration, real-time monitoring, and historical visualization.

## Features

- **Multi-sensor support** — monitor any number of AirGradient sensors simultaneously
- **Configurable polling** — per-sensor poll interval (default: 60s)
- **RRD-backed storage** — automatic consolidation across 4 retention tiers:
  - 1-minute granularity for the past 48 hours
  - 5-minute granularity for the past 2 weeks
  - 10-minute granularity for the past month
  - 1-hour granularity for up to 5 years
- **Pre-rendered graphs** — RRD-generated PNG graphs always ready to view (no render delay), regenerated periodically
- **Interactive graph explorer** — build custom charts with Chart.js using rrd_fetch data; supports zoom, pan, multi-sensor, multi-metric
- **Web management UI** — add/remove sensors, configure poll rates and graph settings; all saved to `config.toml`
- **Single binary** — all assets embedded at compile time; only runtime dependency is `librrd8`
- **Docker + GitHub Actions** — multi-stage Docker image published to GHCR on every push to `main`

## Quick Start

### With Docker (recommended)

```bash
docker run -d \
  -p 8080:8080 \
  -v /path/to/data:/data \
  ghcr.io/<owner>/airgradient:latest
```

Then open http://localhost:8080, go to **Settings**, and add your first sensor.

### Docker Compose

```yaml
services:
  airgradient:
    image: ghcr.io/avirtuos/airgradient:latest
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
    restart: unless-stopped
```

### Portainer Stack

In Portainer, go to **Stacks → Add stack**, choose **Web editor**, and paste:

```yaml
version: "3.8"
services:
  airgradient:
    image: ghcr.io/avirtuos/airgradient:latest
    container_name: airgradient
    ports:
      - "8080:8080"
    volumes:
      - /opt/airgradient/data:/data
    environment:
      - AIRGRADIENT_CONFIG=/data/config.toml
    restart: unless-stopped
```

> **Note:** Change `/opt/airgradient/data` to any host path you want to persist RRD files and graphs. The directory will be created automatically on first run.

### Build from source

**Prerequisites:** Rust (stable), `librrd-dev`, `pkg-config`

```bash
# Ubuntu/Debian
sudo apt-get install librrd-dev pkg-config

# Build
cargo build --release

# Run
./target/release/airgradient
```

Options:
```
--config <PATH>    Config file path (default: ./config.toml or $AIRGRADIENT_CONFIG)
```

## Configuration

The application creates a default `config.toml` on first run:

```toml
[server]
listen_addr = "0.0.0.0:8080"
data_dir = "./data"          # where RRD files and graphs are stored

[graphs]
regeneration_interval_secs = 300   # how often to regenerate pre-rendered graphs
width = 800                        # graph image width in pixels
height = 400                       # graph image height in pixels

[[sensors]]
id = "living-room"
name = "Living Room"
base_url = "http://air01.localdomain"
poll_interval_secs = 60
enabled = true
```

All settings can also be managed through the web UI at `/settings`. Changes are immediately written back to `config.toml`.

## Sensor API

The application expects each sensor to expose the AirGradient local API:

```
GET <base_url>/measures/current
```

Example response:
```json
{
  "pm02": 0, "rco2": 764, "atmp": 20.59, "rhum": 41.63,
  "tvocIndex": 99, "noxIndex": 1, "wifi": -51,
  "serialno": "3cdc75be4f4c", "firmware": "3.6.2", "model": "I-9PSL"
}
```

## Web UI

| Page | URL | Description |
|------|-----|-------------|
| Dashboard | `/` | Sensor grid with current readings and 48h graph thumbnails |
| Sensor Detail | `/sensors/:id` | Full readings + tabbed historical graphs (48h → 5y) |
| Graph Explorer | `/explorer` | Interactive multi-sensor/multi-metric chart builder |
| Settings | `/settings` | Add/edit/remove sensors, graph configuration |

## Data

All data is stored under `data_dir` (default: `./data`):

```
data/
├── config.toml          (written by the web UI)
├── living-room.rrd      (one RRD file per sensor)
├── bedroom.rrd
└── graphs/
    └── living-room/
        ├── aq_48h.png   (air quality, 48 hours)
        ├── env_2w.png   (temperature + humidity, 2 weeks)
        └── ...          (3 categories × 5 ranges = 15 PNGs per sensor)
```

## Documentation

See [docs/design.md](docs/design.md) for architecture details, RRD schema, API reference, and deployment guide.

## GitHub Actions Setup

The CI/CD workflow publishes to `ghcr.io`. No additional secrets are required — it uses the automatic `GITHUB_TOKEN`.

To enable publishing, ensure the repository's **Settings → Actions → General → Workflow permissions** is set to "Read and write permissions".
