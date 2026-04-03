mod config;
mod models;
mod sensor;
mod storage;
mod web;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::RwLock;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::config::{resolve_config_path, AppConfig};
use crate::models::SensorStatus;
use crate::sensor::poller::PollManager;
use crate::storage::grapher::Grapher;
use crate::storage::rrd::RrdStore;
use crate::web::AppState;

#[derive(Parser)]
#[command(name = "airgradient", about = "AirGradient sensor monitor")]
struct Cli {
    /// Path to config file (default: ./config.toml or $AIRGRADIENT_CONFIG)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("airgradient=info".parse()?))
        .init();

    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config);
    info!("Loading config from {}", config_path.display());

    let config = AppConfig::load(&config_path)?;

    // Ensure data directory exists
    std::fs::create_dir_all(&config.server.data_dir)?;

    let data_dir = config.server.data_dir.clone();
    let config = Arc::new(RwLock::new(config));
    let config_path = Arc::new(config_path);

    // Sensor status map: sensor_id -> SensorStatus
    let statuses: Arc<RwLock<HashMap<String, SensorStatus>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // RRD store — synchronous, all ops serialised internally
    let rrd_store = Arc::new(RrdStore::new(data_dir.clone()));

    // Ensure RRD files exist for all configured sensors
    {
        let cfg = config.read().await;
        for sensor in &cfg.sensors {
            let rrd = Arc::clone(&rrd_store);
            let id = sensor.id.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = rrd.ensure_rrd(&id) {
                    tracing::warn!("Failed to create RRD for {id}: {e}");
                }
            })
            .await?;
        }
    }

    // Grapher for pre-rendered PNG generation
    let grapher = Arc::new(Grapher::new(data_dir));

    // Poll manager — starts a tokio task per enabled sensor
    let poll_manager = Arc::new(PollManager::new(
        Arc::clone(&config),
        Arc::clone(&rrd_store),
        Arc::clone(&statuses),
    ));
    poll_manager.start_all().await;

    // Graph regen background task
    {
        let grapher = Arc::clone(&grapher);
        let config_bg = Arc::clone(&config);
        tokio::spawn(async move {
            loop {
                let (interval, width, height, temp_unit) = {
                    let cfg = config_bg.read().await;
                    (
                        cfg.graphs.regeneration_interval_secs,
                        cfg.graphs.width,
                        cfg.graphs.height,
                        cfg.graphs.temp_unit,
                    )
                };
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;

                let sensors: Vec<(String, String, String)> = {
                    let cfg = config_bg.read().await;
                    cfg.sensors.iter().map(|s| (s.id.clone(), s.name.clone(), s.base_url.clone())).collect()
                };
                for (sensor_id, sensor_name, base_url) in sensors {
                    let g = Arc::clone(&grapher);
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = g.regenerate_all(&sensor_id, &sensor_name, &base_url, width, height, temp_unit) {
                            tracing::warn!("Graph regen failed for {sensor_id}: {e}");
                        }
                    })
                    .await
                    .ok();
                }
            }
        });
    }

    let app_state = AppState {
        config: Arc::clone(&config),
        config_path: Arc::clone(&config_path),
        rrd_store: Arc::clone(&rrd_store),
        grapher: Arc::clone(&grapher),
        statuses: Arc::clone(&statuses),
        poll_manager: Arc::clone(&poll_manager),
    };

    let listen_addr = {
        let cfg = config.read().await;
        cfg.server.listen_addr.clone()
    };

    let router = web::build_router(app_state);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    info!("Listening on http://{listen_addr}");
    axum::serve(listener, router).await?;

    Ok(())
}
