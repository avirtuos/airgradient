use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};

use crate::models::{GraphCategory, GraphRange, SensorReading, SensorStatus};
use crate::web::AppState;

// ── Template structs ──────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "index.html")]
struct DashboardTemplate {
    sensors: Vec<SensorCard>,
    use_fahrenheit: bool,
}

#[derive(Template)]
#[template(path = "sensor_detail.html")]
struct SensorDetailTemplate {
    sensor: SensorStatus,
    categories: Vec<GraphCategory>,
    ranges: Vec<GraphRange>,
    use_fahrenheit: bool,
}

#[derive(Template)]
#[template(path = "explorer.html")]
struct ExplorerTemplate {
    sensor_ids: Vec<(String, String)>, // (id, name)
    ds_names: Vec<String>,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    sensors: Vec<SensorStatus>,
    regen_interval: u64,
    graph_width: u32,
    graph_height: u32,
    temp_unit_is_fahrenheit: bool,
}

#[derive(Template)]
#[template(path = "partials/sensor_cards.html")]
struct SensorCardsPartial {
    sensors: Vec<SensorCard>,
    use_fahrenheit: bool,
}

// A view model for a sensor card on the dashboard
pub struct SensorCard {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    pub reading: Option<SensorReading>,
}

// ── Page handlers ─────────────────────────────────────────────────────────────

pub async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let use_fahrenheit = state.config.read().await.graphs.temp_unit.is_fahrenheit();
    let cards = build_cards(&state).await;
    render(DashboardTemplate { sensors: cards, use_fahrenheit })
}

pub async fn partial_sensor_cards(State(state): State<AppState>) -> impl IntoResponse {
    let use_fahrenheit = state.config.read().await.graphs.temp_unit.is_fahrenheit();
    let cards = build_cards(&state).await;
    render(SensorCardsPartial { sensors: cards, use_fahrenheit })
}

pub async fn sensor_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let statuses = state.statuses.read().await;
    let cfg = state.config.read().await;

    let sensor_cfg = match cfg.sensors.iter().find(|s| s.id == id) {
        Some(s) => s,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let status = statuses.get(&id).cloned().unwrap_or_else(|| SensorStatus {
        id: sensor_cfg.id.clone(),
        name: sensor_cfg.name.clone(),
        base_url: sensor_cfg.base_url.clone(),
        poll_interval_secs: sensor_cfg.poll_interval_secs,
        enabled: sensor_cfg.enabled,
        last_seen: None,
        last_error: None,
        latest_reading: None,
    });

    let use_fahrenheit = cfg.graphs.temp_unit.is_fahrenheit();
    render(SensorDetailTemplate {
        sensor: status,
        categories: GraphCategory::all().to_vec(),
        ranges: GraphRange::all().to_vec(),
        use_fahrenheit,
    })
    .into_response()
}

pub async fn explorer(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    let sensor_ids: Vec<(String, String)> = cfg
        .sensors
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();

    render(ExplorerTemplate {
        sensor_ids,
        ds_names: crate::models::RRD_DS_NAMES.iter().map(|s| s.to_string()).collect(),
    })
}

pub async fn settings(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    let statuses = state.statuses.read().await;

    let sensors: Vec<SensorStatus> = cfg
        .sensors
        .iter()
        .map(|s| {
            statuses.get(&s.id).cloned().unwrap_or_else(|| SensorStatus {
                id: s.id.clone(),
                name: s.name.clone(),
                base_url: s.base_url.clone(),
                poll_interval_secs: s.poll_interval_secs,
                enabled: s.enabled,
                last_seen: None,
                last_error: None,
                latest_reading: None,
            })
        })
        .collect();

    render(SettingsTemplate {
        sensors,
        regen_interval: cfg.graphs.regeneration_interval_secs,
        graph_width: cfg.graphs.width,
        graph_height: cfg.graphs.height,
        temp_unit_is_fahrenheit: cfg.graphs.temp_unit.is_fahrenheit(),
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn build_cards(state: &AppState) -> Vec<SensorCard> {
    let cfg = state.config.read().await;
    let statuses = state.statuses.read().await;
    cfg.sensors
        .iter()
        .map(|s| {
            let status = statuses.get(&s.id);
            SensorCard {
                id: s.id.clone(),
                name: s.name.clone(),
                base_url: s.base_url.clone(),
                last_seen: status.and_then(|st| st.last_seen),
                last_error: status.and_then(|st| st.last_error.clone()),
                reading: status.and_then(|st| st.latest_reading.clone()),
            }
        })
        .collect()
}

fn render<T: Template>(tmpl: T) -> Html<String> {
    match tmpl.render() {
        Ok(html) => Html(html),
        Err(e) => Html(format!("<pre>Template error: {e}</pre>")),
    }
}
