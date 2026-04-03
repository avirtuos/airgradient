use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::AppConfig;
use crate::models::{SensorConfig, SensorStatus};
use crate::sensor::client::fetch_current;
use crate::storage::rrd::RrdStore;

/// Manages per-sensor polling tasks.
pub struct PollManager {
    config: Arc<RwLock<AppConfig>>,
    rrd_store: Arc<RrdStore>,
    statuses: Arc<RwLock<HashMap<String, SensorStatus>>>,
    /// Maps sensor_id → task abort handle
    tasks: tokio::sync::Mutex<HashMap<String, JoinHandle<()>>>,
}

impl PollManager {
    pub fn new(
        config: Arc<RwLock<AppConfig>>,
        rrd_store: Arc<RrdStore>,
        statuses: Arc<RwLock<HashMap<String, SensorStatus>>>,
    ) -> Self {
        Self {
            config,
            rrd_store,
            statuses,
            tasks: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Start polling tasks for all enabled sensors in config.
    pub async fn start_all(&self) {
        let sensors: Vec<SensorConfig> = {
            let cfg = self.config.read().await;
            cfg.sensors.clone()
        };
        for sensor in sensors {
            if sensor.enabled {
                self.start_sensor(&sensor).await;
            }
        }
    }

    /// Start a polling task for a single sensor. Replaces an existing task if present.
    pub async fn start_sensor(&self, sensor: &SensorConfig) {
        self.stop_sensor(&sensor.id).await;

        let sensor = sensor.clone();
        let rrd_store = Arc::clone(&self.rrd_store);
        let statuses = Arc::clone(&self.statuses);

        // Ensure RRD exists before polling
        if let Err(e) = rrd_store.ensure_rrd(&sensor.id) {
            warn!("RRD creation failed for {}: {e}", sensor.id);
        }

        // Extract fields so the async block owns plain values (no sensor struct capture issues)
        let sensor_id = sensor.id.clone();
        let sensor_name = sensor.name.clone();
        let sensor_base_url = sensor.base_url.clone();
        let sensor_interval = sensor.poll_interval_secs;

        let handle = tokio::spawn(async move {
            let client = Client::new();
            info!("Started polling {sensor_base_url} every {sensor_interval}s");
            loop {
                let now = chrono::Utc::now();
                match fetch_current(&client, &sensor_base_url).await {
                    Ok(reading) => {
                        // Update in-memory status
                        {
                            let mut map = statuses.write().await;
                            let entry =
                                map.entry(sensor_id.clone())
                                    .or_insert_with(|| SensorStatus {
                                        id: sensor_id.clone(),
                                        name: sensor_name.clone(),
                                        base_url: sensor_base_url.clone(),
                                        poll_interval_secs: sensor_interval,
                                        enabled: true,
                                        last_seen: None,
                                        last_error: None,
                                        latest_reading: None,
                                    });
                            entry.last_seen = Some(now.timestamp());
                            entry.last_error = None;
                            entry.latest_reading = Some(reading.clone());
                        }

                        // Write to RRD (blocking call, use spawn_blocking)
                        let sid = sensor_id.clone();
                        let rrd = Arc::clone(&rrd_store);
                        let ts = now.timestamp();
                        match tokio::task::spawn_blocking(move || rrd.update(&sid, ts, &reading))
                            .await
                        {
                            Ok(Err(e)) => error!("RRD update failed for {sensor_id}: {e}"),
                            Err(e) => error!("spawn_blocking panicked for {sensor_id}: {e}"),
                            Ok(Ok(())) => {}
                        }
                    }
                    Err(e) => {
                        warn!("Poll failed for {sensor_base_url}: {e}");
                        let mut map = statuses.write().await;
                        let entry = map
                            .entry(sensor_id.clone())
                            .or_insert_with(|| SensorStatus {
                                id: sensor_id.clone(),
                                name: sensor_name.clone(),
                                base_url: sensor_base_url.clone(),
                                poll_interval_secs: sensor_interval,
                                enabled: true,
                                last_seen: None,
                                last_error: None,
                                latest_reading: None,
                            });
                        entry.last_error = Some(e.to_string());
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(sensor_interval)).await;
            }
        });

        let mut tasks = self.tasks.lock().await;
        tasks.insert(sensor.id.clone(), handle);
    }

    /// Stop the polling task for a sensor.
    pub async fn stop_sensor(&self, sensor_id: &str) {
        let mut tasks = self.tasks.lock().await;
        if let Some(handle) = tasks.remove(sensor_id) {
            handle.abort();
        }
    }

    /// Restart a sensor's polling task (e.g. after config change).
    pub async fn restart_sensor(&self, sensor: &SensorConfig) {
        if sensor.enabled {
            self.start_sensor(sensor).await;
        } else {
            self.stop_sensor(&sensor.id).await;
        }
    }
}
