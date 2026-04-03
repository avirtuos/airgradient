pub mod api;
pub mod pages;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::models::SensorStatus;
use crate::sensor::poller::PollManager;
use crate::storage::grapher::Grapher;
use crate::storage::rrd::RrdStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: Arc<PathBuf>,
    pub rrd_store: Arc<RrdStore>,
    pub grapher: Arc<Grapher>,
    pub statuses: Arc<RwLock<HashMap<String, SensorStatus>>>,
    pub poll_manager: Arc<PollManager>,
}

pub fn build_router(state: AppState) -> Router {
    use axum::routing::{get, post};

    Router::new()
        // HTML pages
        .route("/", get(pages::dashboard))
        .route("/sensors/{id}", get(pages::sensor_detail))
        .route("/explorer", get(pages::explorer))
        .route("/settings", get(pages::settings))
        // htmx partials
        .route("/partials/sensor-cards", get(pages::partial_sensor_cards))
        // REST API
        .route("/api/sensors", get(api::list_sensors).post(api::add_sensor))
        .route(
            "/api/sensors/{id}",
            get(api::get_sensor)
                .put(api::update_sensor)
                .delete(api::delete_sensor),
        )
        .route("/api/sensors/{id}/current", get(api::current_reading))
        .route("/api/sensors/{id}/history", get(api::history))
        .route(
            "/api/sensors/{id}/graph/{category}/{range}",
            get(api::serve_graph),
        )
        .route("/api/config", get(api::get_config).put(api::update_config))
        .route("/api/admin/reset-rrds", post(api::reset_rrds))
        .route("/api/admin/regenerate-graphs", post(api::regenerate_graphs))
        .route("/api/health", get(api::health))
        // Static assets (embedded)
        .route("/static/{*path}", get(api::static_asset))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
