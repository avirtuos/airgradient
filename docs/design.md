# AirGradient Monitor — Design Document

## Overview

AirGradient Monitor is a self-hosted Rust application that polls AirGradient air quality sensors over HTTP, stores historical time-series data using RRD (Round Robin Database), and serves a web UI for configuration, monitoring, and visualization.

The application is a single binary with no runtime dependencies except `librrd8` (the librrd shared library). All static assets — HTML templates, CSS, JavaScript, and vendor libraries — are embedded at compile time.

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   airgradient binary                 │
│                                                     │
│  ┌─────────────┐    ┌──────────────────────────┐   │
│  │ Poll Manager│    │  Axum Web Server          │   │
│  │             │    │  ├─ REST API (/api/...)   │   │
│  │ per-sensor  │    │  ├─ HTML pages (Askama)   │   │
│  │ tokio tasks │    │  └─ Static assets         │   │
│  └──────┬──────┘    └──────────────────────────┘   │
│         │                       │                   │
│         ▼                       ▼                   │
│  ┌──────────────┐    ┌──────────────────────┐      │
│  │  RRD Store   │    │  Grapher             │      │
│  │  (librrd)    │    │  (librrd graph)      │      │
│  │  rrd_update  │    │  rrd_graph → PNG     │      │
│  │  rrd_fetch   │    │  (background task)   │      │
│  └──────┬───────┘    └──────────────────────┘      │
└─────────┼──────────────────────────────────────────┘
          │
          ▼
   ┌──────────────────────────────┐
   │  data/                       │
   │  ├─ config.toml              │
   │  ├─ <sensor_id>.rrd          │
   │  └─ graphs/                  │
   │     └─ <sensor_id>/          │
   │        ├─ aq_48h.png         │
   │        ├─ env_2w.png         │
   │        └─ ...                │
   └──────────────────────────────┘
```

---

## Data Flow

1. **Poll loop** (per sensor, configurable interval): `GET <base_url>/measures/current` → `SensorReading` JSON
2. **In-memory cache** updated immediately (used for "current" readings on dashboard)
3. **RRD update**: `rrd_update` writes the 14 numeric data sources into the sensor's `.rrd` file
4. **Graph regeneration** (background task, configurable interval): `rrd_graph` generates PNG files from RRD data → written to `data/graphs/<sensor_id>/`
5. **Web requests**: PNG graphs served directly from disk; interactive chart data served via `rrd_fetch` → JSON

---

## RRD Storage Design

### One RRD file per sensor

Each sensor is stored at `<data_dir>/<sensor_id>.rrd`.

**Base step**: 60 seconds. All RRD arithmetic happens at 60-second resolution.

### Data Sources (14 GAUGE DS, heartbeat = 120s)

| DS Name      | Metric              | Unit     |
|-------------|---------------------|----------|
| `pm01`       | PM1.0               | ug/m3    |
| `pm02`       | PM2.5               | ug/m3    |
| `pm10`       | PM10                | ug/m3    |
| `pm003_count`| 0.3um particle count| #/dl     |
| `pm02_comp`  | PM2.5 (compensated) | ug/m3    |
| `atmp`       | Temperature         | °C       |
| `atmp_comp`  | Temp (compensated)  | °C       |
| `rhum`       | Relative humidity   | %        |
| `rhum_comp`  | Humidity (comp.)    | %        |
| `rco2`       | CO2                 | ppm      |
| `tvoc_index` | VOC index           | 1–500    |
| `tvoc_raw`   | VOC raw signal      | ticks    |
| `nox_index`  | NOx index           | 1–500    |
| `nox_raw`    | NOx raw signal      | ticks    |

### Round Robin Archives

| Steps | Rows  | Retention | Resolution |
|-------|-------|-----------|------------|
| 1     | 2880  | 48 hours  | 1 minute   |
| 5     | 4032  | 2 weeks   | 5 minutes  |
| 10    | 4320  | 1 month   | 10 minutes |
| 60    | 43800 | ~5 years  | 1 hour     |

Each combination is stored for AVERAGE, MIN, and MAX consolidation functions (12 RRAs total per sensor).

**Storage estimate**: ~5 MB per sensor.

---

## Graph Generation

### Pre-rendered graphs

A background task calls `rrd_graph` every `graphs.regeneration_interval_secs` (default: 300s).

Three categories × five time ranges = **15 graphs per sensor**:

| Category     | Metrics           | Filename pattern         |
|-------------|-------------------|--------------------------|
| Air Quality  | PM2.5, CO2        | `aq_{range}.png`         |
| Environment  | Temperature, Humidity | `env_{range}.png`    |
| VOC / NOx    | VOC Index, NOx Index | `voc_{range}.png`     |

Time ranges: `48h`, `2w`, `1m`, `1y`, `5y`.

Graphs use a dark color scheme (dark navy background). PNG bytes are returned directly from memory by `rrd_graph` and written to disk.

### Interactive graph explorer

The `/explorer` page uses [Chart.js](https://www.chartjs.org/) (locally vendored) to build custom interactive charts. The user selects sensors, metrics, time range, and consolidation function. The frontend fetches data from `/api/sensors/:id/history` (which calls `rrd_fetch`) and renders it as a zoomable, pannable line chart.

---

## API Reference

### REST endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sensors` | List all sensors with status |
| POST | `/api/sensors` | Add a new sensor (creates RRD file, starts polling) |
| GET | `/api/sensors/:id` | Get sensor config |
| PUT | `/api/sensors/:id` | Update sensor (restarts polling task) |
| DELETE | `/api/sensors/:id` | Remove sensor (deletes RRD + graph files) |
| GET | `/api/sensors/:id/current` | Latest in-memory reading |
| GET | `/api/sensors/:id/history` | Fetch historical data from RRD as JSON |
| GET | `/api/sensors/:id/graph/:type/:range` | Serve pre-rendered PNG graph |
| GET | `/api/config` | Get server/graph config |
| PUT | `/api/config` | Update graph config |
| GET | `/api/health` | Health check |

### History query parameters

| Parameter | Values | Default | Description |
|-----------|--------|---------|-------------|
| `from` | Unix timestamp | `to - 48h` | Start time |
| `to` | Unix timestamp | now | End time |
| `resolution` | `auto`, `1m`, `5m`, `10m`, `1h` | `auto` | Desired step |
| `cf` | `AVERAGE`, `MIN`, `MAX` | `AVERAGE` | Consolidation function |

Auto-resolution selection:
- Range ≤ 48h → 1-minute steps
- Range ≤ 2 weeks → 5-minute steps
- Range ≤ 1 month → 10-minute steps
- Range > 1 month → 1-hour steps

---

## Frontend Architecture

All frontend assets are embedded in the binary at compile time via `rust-embed`. No file system access is needed for static assets.

| Page | URL | Description |
|------|-----|-------------|
| Dashboard | `/` | Sensor cards with current readings and 48h graph thumbnails. Auto-refreshes every 30s via htmx. |
| Sensor Detail | `/sensors/:id` | Full current readings + tabbed pre-rendered graphs by category and range. |
| Graph Explorer | `/explorer` | Interactive Chart.js chart builder with rrd_fetch data. |
| Settings | `/settings` | Sensor management and graph config. |

**Vendor libraries** (all locally hosted, no CDN):
- `htmx.min.js` — live partial reloads on the dashboard
- `chart.umd.min.js` — interactive charts in the Explorer
- `chartjs-plugin-zoom.min.js` — zoom/pan in Chart.js
- `hammer.min.js` — touch support for zoom plugin
- `pico.min.css` — classless CSS base theme

---

## Configuration

File: `config.toml` (location: `./config.toml`, overridable via `--config` flag or `AIRGRADIENT_CONFIG` env var)

```toml
[server]
listen_addr = "0.0.0.0:8080"
data_dir = "./data"

[graphs]
regeneration_interval_secs = 300
width = 800
height = 400

[[sensors]]
id = "living-room"
name = "Living Room"
base_url = "http://air01.localdomain"
poll_interval_secs = 60
enabled = true
```

The web UI reads and writes this file on every sensor/config change. The config is the authoritative source; the web UI never diverges from it.

---

## Deployment

### Docker

```bash
docker run -d \
  -p 8080:8080 \
  -v /path/to/data:/data \
  ghcr.io/<owner>/airgradient:latest
```

The `/data` volume persists:
- `config.toml` — application configuration
- `*.rrd` — RRD data files (one per sensor)
- `graphs/` — pre-rendered PNG graphs

### Docker Compose example

```yaml
services:
  airgradient:
    image: ghcr.io/<owner>/airgradient:latest
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
    restart: unless-stopped
```

### GitHub Actions CI/CD

On every push to `main`:
1. `cargo fmt --check` + `cargo clippy` + `cargo test`
2. Docker multi-stage build (installs `librrd-dev` in build stage, `librrd8` in runtime stage)
3. Push to `ghcr.io/<owner>/airgradient` tagged as `latest` and `<sha>`

No secrets needed beyond the automatic `GITHUB_TOKEN` for GHCR.
