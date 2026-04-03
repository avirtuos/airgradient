/// RRD storage layer using the `rrd` crate (FFI bindings to librrd).
///
/// One RRD file per sensor at `<data_dir>/<sensor_id>.rrd`.
///
/// Data sources (14 GAUGE DS, heartbeat = 120s):
///   pm01, pm02, pm10, pm003_count, pm02_comp,
///   atmp, atmp_comp, rhum, rhum_comp,
///   rco2, tvoc_index, tvoc_raw, nox_index, nox_raw
///
/// Round Robin Archives (base step = 60s):
///   AVERAGE / MIN / MAX  x  4 retention tiers
///
/// Tier | Steps | Rows  | Retention
/// -----|-------|-------|----------
///  1m  |   1   | 2880  | 48 hours
///  5m  |   5   | 4032  | 2 weeks
/// 10m  |  10   | 4320  | 1 month (~30d)
///  1h  |  60   | 43800 | ~5 years
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rrd::ops::{create, fetch, update};
use rrd::ConsolidationFn;
use tracing::info;

use crate::models::{SensorReading, RRD_DS_NAMES};

const STEP_SECS: u64 = 60;
const HEARTBEAT_SECS: u32 = 120;

/// Result of an rrd_fetch call
pub struct FetchResult {
    /// Step size in seconds
    pub step_secs: u64,
    /// Data source names (column names)
    pub ds_names: Vec<String>,
    /// Rows: each row is one Vec<Option<f64>> aligned to ds_names
    pub rows: Vec<(i64, Vec<Option<f64>>)>,
}

pub struct RrdStore {
    data_dir: PathBuf,
    /// Serialise all librrd calls — librrd is not guaranteed thread-safe
    lock: Mutex<()>,
}

impl RrdStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lock: Mutex::new(()),
        }
    }

    fn rrd_path(&self, sensor_id: &str) -> PathBuf {
        self.data_dir.join(format!("{sensor_id}.rrd"))
    }

    /// Ensure the RRD file for `sensor_id` exists, creating it if necessary.
    pub fn ensure_rrd(&self, sensor_id: &str) -> Result<()> {
        let path = self.rrd_path(sensor_id);
        if path.exists() {
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        info!("Creating RRD file: {}", path.display());

        let ds_names = RRD_DS_NAMES.iter().map(|n| create::DataSourceName::new(*n));
        let data_sources: Vec<create::DataSource> = ds_names
            .map(|name| create::DataSource::gauge(&name, HEARTBEAT_SECS, None, None))
            .collect();

        let mut archives: Vec<create::Archive> = Vec::new();
        for &(steps, rows) in &[(1u32, 2880u32), (5, 4032), (10, 4320), (60, 43800)] {
            for &cf in &[ConsolidationFn::Avg, ConsolidationFn::Min, ConsolidationFn::Max] {
                archives.push(
                    create::Archive::new(cf, 0.5, steps, rows)
                        .context("Failed to create RRA definition")?,
                );
            }
        }

        let start = SystemTime::now()
            .checked_sub(Duration::from_secs(10))
            .unwrap_or(UNIX_EPOCH);

        let _guard = self.lock.lock().unwrap();
        create::create(
            &path,
            start,
            Duration::from_secs(STEP_SECS),
            true,  // no_overwrite
            None,  // no template
            &[],   // no sources
            &data_sources,
            &archives,
        )
        .with_context(|| format!("rrd_create failed for {sensor_id}"))?;

        Ok(())
    }

    /// Update the RRD file with a new reading at `timestamp` (Unix seconds).
    pub fn update(&self, sensor_id: &str, timestamp: i64, reading: &SensorReading) -> Result<()> {
        let path = self.rrd_path(sensor_id);

        let values = reading.to_rrd_values();
        let data: Vec<update::Datum> = values
            .iter()
            .map(|&v| {
                if v.is_nan() {
                    update::Datum::Unspecified
                } else {
                    update::Datum::Float(v)
                }
            })
            .collect();

        let ts = UNIX_EPOCH
            .checked_add(Duration::from_secs(timestamp as u64))
            .unwrap_or_else(SystemTime::now);

        let _guard = self.lock.lock().unwrap();
        update::update_all(
            &path,
            update::Options {
                skip_past_updates: true,
            },
            &[(update::BatchTime::Timestamp(ts), data.as_slice())],
        )
        .with_context(|| format!("rrd_update failed for {sensor_id}"))?;

        Ok(())
    }

    /// Fetch data from the RRD and return a `FetchResult`.
    ///
    /// `cf` is "AVERAGE", "MIN", or "MAX".
    /// `start`/`end` are Unix timestamps. `step_secs` is the desired resolution (0 = auto).
    pub fn fetch(
        &self,
        sensor_id: &str,
        cf: &str,
        start_ts: i64,
        end_ts: i64,
        step_secs: u64,
    ) -> Result<FetchResult> {
        let path = self.rrd_path(sensor_id);

        let cf_enum = match cf.to_uppercase().as_str() {
            "MIN" => ConsolidationFn::Min,
            "MAX" => ConsolidationFn::Max,
            _ => ConsolidationFn::Avg,
        };

        let start = UNIX_EPOCH
            .checked_add(Duration::from_secs(start_ts as u64))
            .unwrap_or(UNIX_EPOCH);
        let end = UNIX_EPOCH
            .checked_add(Duration::from_secs(end_ts as u64))
            .unwrap_or_else(SystemTime::now);
        let resolution = Duration::from_secs(step_secs.max(STEP_SECS));

        let _guard = self.lock.lock().unwrap();
        let data =
            fetch::fetch(&path, cf_enum, start, end, resolution)
                .with_context(|| format!("rrd_fetch failed for {sensor_id}"))?;

        let step_secs = data.step().as_secs();
        let ds_names = data.ds_names().to_vec();

        let mut rows: Vec<(i64, Vec<Option<f64>>)> = Vec::new();
        for row in data.rows().iter() {
            let ts = row
                .timestamp()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let values: Vec<Option<f64>> = row
                .as_slice()
                .iter()
                .map(|&v| if v.is_nan() { None } else { Some(v) })
                .collect();
            rows.push((ts, values));
        }

        Ok(FetchResult {
            step_secs,
            ds_names,
            rows,
        })
    }

    /// Delete the RRD file for a sensor.
    pub fn delete(&self, sensor_id: &str) -> Result<()> {
        let path = self.rrd_path(sensor_id);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to delete RRD for {sensor_id}"))?;
        }
        Ok(())
    }
}

// SAFETY: All librrd calls are serialised via the internal Mutex.
unsafe impl Sync for RrdStore {}
