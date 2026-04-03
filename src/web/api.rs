use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::models::{GraphConfigRequest, SensorConfig, SensorRequest, TempUnit};
use crate::storage::grapher::Grapher;
use crate::web::AppState;

// ── Static asset embedding ────────────────────────────────────────────────────

#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

pub async fn static_asset(Path(path): Path<String>) -> impl IntoResponse {
    match StaticAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

// ── Health ────────────────────────────────────────────────────────────────────

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let statuses = state.statuses.read().await;
    let sensors: Vec<Value> = statuses
        .values()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "last_seen": s.last_seen,
                "last_error": s.last_error,
                "enabled": s.enabled,
            })
        })
        .collect();
    Json(json!({ "status": "ok", "sensors": sensors }))
}

// ── Sensor CRUD ───────────────────────────────────────────────────────────────

pub async fn list_sensors(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.read().await;
    let statuses = state.statuses.read().await;
    let sensors: Vec<Value> = cfg
        .sensors
        .iter()
        .map(|s| {
            let status = statuses.get(&s.id);
            json!({
                "id": s.id,
                "name": s.name,
                "base_url": s.base_url,
                "poll_interval_secs": s.poll_interval_secs,
                "enabled": s.enabled,
                "last_seen": status.and_then(|st| st.last_seen),
                "last_error": status.and_then(|st| st.last_error.as_deref()),
            })
        })
        .collect();
    Json(json!({ "sensors": sensors }))
}

pub async fn get_sensor(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let cfg = state.config.read().await;
    let sensor = cfg
        .sensors
        .iter()
        .find(|s| s.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "id": sensor.id,
        "name": sensor.name,
        "base_url": sensor.base_url,
        "poll_interval_secs": sensor.poll_interval_secs,
        "enabled": sensor.enabled,
    })))
}

pub async fn add_sensor(
    State(state): State<AppState>,
    Json(req): Json<SensorRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = slugify(&req.name);

    {
        let cfg = state.config.read().await;
        if cfg.sensors.iter().any(|s| s.id == id) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({ "error": "sensor with this id already exists" })),
            ));
        }
    }

    let new_sensor = SensorConfig {
        id: id.clone(),
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        poll_interval_secs: req.poll_interval_secs.unwrap_or(60),
        enabled: req.enabled.unwrap_or(true),
    };

    {
        let mut cfg = state.config.write().await;
        cfg.sensors.push(new_sensor.clone());
        if let Err(e) = cfg.save(&state.config_path) {
            tracing::error!("Failed to save config: {e}");
        }
    }

    // Create RRD and start polling
    let rrd = Arc::clone(&state.rrd_store);
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = rrd.ensure_rrd(&id2) {
            tracing::warn!("Failed to create RRD for {id2}: {e}");
        }
    })
    .await
    .ok();
    state.poll_manager.start_sensor(&new_sensor).await;

    Ok(Json(json!({ "id": new_sensor.id, "status": "created" })))
}

pub async fn update_sensor(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SensorRequest>,
) -> Result<Json<Value>, StatusCode> {
    let updated = {
        let mut cfg = state.config.write().await;
        let sensor = cfg
            .sensors
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        sensor.name = req.name;
        sensor.base_url = req.base_url;
        if let Some(interval) = req.poll_interval_secs {
            sensor.poll_interval_secs = interval;
        }
        if let Some(enabled) = req.enabled {
            sensor.enabled = enabled;
        }
        let updated = sensor.clone();
        if let Err(e) = cfg.save(&state.config_path) {
            tracing::error!("Failed to save config: {e}");
        }
        updated
    };

    state.poll_manager.restart_sensor(&updated).await;
    Ok(Json(json!({ "id": updated.id, "status": "updated" })))
}

pub async fn delete_sensor(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    {
        let mut cfg = state.config.write().await;
        let pos = cfg
            .sensors
            .iter()
            .position(|s| s.id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        cfg.sensors.remove(pos);
        if let Err(e) = cfg.save(&state.config_path) {
            tracing::error!("Failed to save config: {e}");
        }
    }

    state.poll_manager.stop_sensor(&id).await;

    let rrd = Arc::clone(&state.rrd_store);
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = rrd.delete(&id2) {
            tracing::warn!("Failed to delete RRD for {id2}: {e}");
        }
    })
    .await
    .ok();

    Ok(Json(json!({ "id": id, "status": "deleted" })))
}

// ── Current reading ───────────────────────────────────────────────────────────

pub async fn current_reading(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let statuses = state.statuses.read().await;
    let status = statuses.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "sensor_id": id,
        "last_seen": status.last_seen,
        "reading": status.latest_reading,
    })))
}

// ── History (rrd_fetch → JSON) ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub resolution: Option<String>,
    pub cf: Option<String>,
}

pub async fn history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Validate sensor exists
    {
        let cfg = state.config.read().await;
        if !cfg.sensors.iter().any(|s| s.id == id) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "sensor not found" })),
            ));
        }
    }

    let now = chrono::Utc::now().timestamp();
    let end = q.to.unwrap_or(now);
    let start = q.from.unwrap_or(end - 48 * 3600);
    let cf = q.cf.as_deref().unwrap_or("AVERAGE").to_uppercase();

    // Auto-select resolution from range
    let range_secs = end - start;
    let step_secs: u64 = match q.resolution.as_deref().unwrap_or("auto") {
        "1m" => 60,
        "5m" => 300,
        "10m" => 600,
        "1h" => 3600,
        _ => {
            if range_secs <= 48 * 3600 {
                60
            } else if range_secs <= 14 * 86400 {
                300
            } else if range_secs <= 30 * 86400 {
                600
            } else {
                3600
            }
        }
    };

    let rrd = Arc::clone(&state.rrd_store);
    let id2 = id.clone();
    let cf2 = cf.clone();
    let result = tokio::task::spawn_blocking(move || rrd.fetch(&id2, &cf2, start, end, step_secs))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    let datapoints: Vec<Value> = result
        .rows
        .iter()
        .map(|(ts, values)| {
            let mut obj = serde_json::Map::new();
            obj.insert("t".into(), json!(ts));
            for (i, name) in result.ds_names.iter().enumerate() {
                match values.get(i).and_then(|v| *v) {
                    Some(f) => obj.insert(name.clone(), json!(f)),
                    None => obj.insert(name.clone(), Value::Null),
                };
            }
            Value::Object(obj)
        })
        .collect();

    Ok(Json(json!({
        "sensor_id": id,
        "cf": cf,
        "step": result.step_secs,
        "ds_names": result.ds_names,
        "datapoints": datapoints,
    })))
}

// ── Pre-rendered graph serving ────────────────────────────────────────────────

pub async fn serve_graph(
    State(state): State<AppState>,
    Path((sensor_id, category_slug, range_slug)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let category = match Grapher::parse_category(&category_slug) {
        Some(c) => c,
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap()
        }
    };
    let range = match Grapher::parse_range(&range_slug) {
        Some(r) => r,
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap()
        }
    };

    let path = state.grapher.graph_path(&sensor_id, category, range);
    match std::fs::read(&path) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap(),
    }
}

// ── App config ────────────────────────────────────────────────────────────────

pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.read().await;
    let temp_unit = match cfg.graphs.temp_unit {
        TempUnit::Celsius => "celsius",
        TempUnit::Fahrenheit => "fahrenheit",
    };
    Json(json!({
        "server": {
            "listen_addr": cfg.server.listen_addr,
            "data_dir": cfg.server.data_dir,
        },
        "graphs": {
            "regeneration_interval_secs": cfg.graphs.regeneration_interval_secs,
            "width": cfg.graphs.width,
            "height": cfg.graphs.height,
            "temp_unit": temp_unit,
        }
    }))
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<GraphConfigRequest>,
) -> Json<Value> {
    {
        let mut cfg = state.config.write().await;
        if let Some(v) = req.regeneration_interval_secs {
            cfg.graphs.regeneration_interval_secs = v;
        }
        if let Some(v) = req.width {
            cfg.graphs.width = v;
        }
        if let Some(v) = req.height {
            cfg.graphs.height = v;
        }
        if let Some(v) = req.temp_unit {
            cfg.graphs.temp_unit = v;
        }
        if let Err(e) = cfg.save(&state.config_path) {
            tracing::error!("Failed to save config: {e}");
        }
    }
    Json(json!({ "status": "updated" }))
}

// ── Admin actions ─────────────────────────────────────────────────────────────

/// Delete and recreate all RRD files for all configured sensors.
/// Existing data is permanently lost. Polling tasks are temporarily
/// stopped and restarted so no update is written to a half-created file.
pub async fn reset_rrds(State(state): State<AppState>) -> Json<Value> {
    let sensors = {
        let cfg = state.config.read().await;
        cfg.sensors.clone()
    };

    for sensor in &sensors {
        state.poll_manager.stop_sensor(&sensor.id).await;
    }

    let rrd = Arc::clone(&state.rrd_store);
    let sensor_ids: Vec<String> = sensors.iter().map(|s| s.id.clone()).collect();
    let result = tokio::task::spawn_blocking(move || {
        let mut errors: Vec<String> = Vec::new();
        for id in &sensor_ids {
            if let Err(e) = rrd.delete(id) {
                tracing::warn!("Failed to delete RRD for {id}: {e}");
            }
            if let Err(e) = rrd.ensure_rrd(id) {
                errors.push(format!("{id}: {e}"));
            }
        }
        errors
    })
    .await
    .unwrap_or_default();

    for sensor in sensors.iter().filter(|s| s.enabled) {
        state.poll_manager.start_sensor(sensor).await;
    }

    if result.is_empty() {
        Json(json!({ "status": "ok", "message": "All RRD files recreated" }))
    } else {
        Json(json!({ "status": "partial", "errors": result }))
    }
}

/// Trigger immediate graph regeneration for all sensors.
pub async fn regenerate_graphs(State(state): State<AppState>) -> Json<Value> {
    let (sensors, width, height, temp_unit) = {
        let cfg = state.config.read().await;
        let sensors: Vec<(String, String, String)> = cfg
            .sensors
            .iter()
            .map(|s| (s.id.clone(), s.name.clone(), s.base_url.clone()))
            .collect();
        (
            sensors,
            cfg.graphs.width,
            cfg.graphs.height,
            cfg.graphs.temp_unit,
        )
    };

    let grapher = Arc::clone(&state.grapher);
    let result = tokio::task::spawn_blocking(move || {
        let mut errors: Vec<String> = Vec::new();
        for (id, name, base_url) in &sensors {
            if let Err(e) = grapher.regenerate_all(id, name, base_url, width, height, temp_unit) {
                errors.push(format!("{id}: {e}"));
            }
        }
        errors
    })
    .await
    .unwrap_or_default();

    if result.is_empty() {
        Json(json!({ "status": "ok", "message": "Graphs regenerated" }))
    } else {
        Json(json!({ "status": "partial", "errors": result }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
