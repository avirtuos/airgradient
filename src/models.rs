use serde::{Deserialize, Serialize};

/// Raw reading from the AirGradient sensor API (GET /measures/current)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensorReading {
    // Particulate matter (ug/m3)
    pub pm01: Option<f64>,
    pub pm02: Option<f64>,
    pub pm10: Option<f64>,
    #[serde(rename = "pm01Standard")]
    pub pm01_standard: Option<f64>,
    #[serde(rename = "pm02Standard")]
    pub pm02_standard: Option<f64>,
    #[serde(rename = "pm10Standard")]
    pub pm10_standard: Option<f64>,
    // Particle counts (#/dl)
    #[serde(rename = "pm003Count")]
    pub pm003_count: Option<f64>,
    #[serde(rename = "pm005Count")]
    pub pm005_count: Option<f64>,
    #[serde(rename = "pm01Count")]
    pub pm01_count: Option<f64>,
    #[serde(rename = "pm02Count")]
    pub pm02_count: Option<f64>,
    #[serde(rename = "pm50Count")]
    pub pm50_count: Option<f64>,
    #[serde(rename = "pm10Count")]
    pub pm10_count: Option<f64>,
    // Humidity-compensated PM (ug/m3)
    #[serde(rename = "pm02Compensated")]
    pub pm02_compensated: Option<f64>,
    // Temperature (Celsius)
    pub atmp: Option<f64>,
    #[serde(rename = "atmpCompensated")]
    pub atmp_compensated: Option<f64>,
    // Relative humidity (%)
    pub rhum: Option<f64>,
    #[serde(rename = "rhumCompensated")]
    pub rhum_compensated: Option<f64>,
    // CO2 (ppm)
    pub rco2: Option<f64>,
    // VOC index (1-500)
    #[serde(rename = "tvocIndex")]
    pub tvoc_index: Option<f64>,
    #[serde(rename = "tvocRaw")]
    pub tvoc_raw: Option<f64>,
    // NOx index (1-500)
    #[serde(rename = "noxIndex")]
    pub nox_index: Option<f64>,
    #[serde(rename = "noxRaw")]
    pub nox_raw: Option<f64>,
    // Device metadata
    pub boot: Option<u64>,
    #[serde(rename = "bootCount")]
    pub boot_count: Option<u64>,
    pub wifi: Option<i32>,
    #[serde(rename = "ledMode")]
    pub led_mode: Option<String>,
    pub serialno: Option<String>,
    pub firmware: Option<String>,
    pub model: Option<String>,
}

impl SensorReading {
    /// Returns all numeric DS values in the order used by the RRD schema.
    /// Unknown/missing values are encoded as NaN for RRD.
    pub fn to_rrd_values(&self) -> [f64; 23] {
        [
            // PM concentrations (ug/m3)
            self.pm01.unwrap_or(f64::NAN),
            self.pm02.unwrap_or(f64::NAN),
            self.pm10.unwrap_or(f64::NAN),
            // Standard PM concentrations (ug/m3)
            self.pm01_standard.unwrap_or(f64::NAN),
            self.pm02_standard.unwrap_or(f64::NAN),
            self.pm10_standard.unwrap_or(f64::NAN),
            // Particle counts (#/dl)
            self.pm003_count.unwrap_or(f64::NAN),
            self.pm005_count.unwrap_or(f64::NAN),
            self.pm01_count.unwrap_or(f64::NAN),
            self.pm02_count.unwrap_or(f64::NAN),
            self.pm50_count.unwrap_or(f64::NAN),
            self.pm10_count.unwrap_or(f64::NAN),
            // Compensated PM (ug/m3)
            self.pm02_compensated.unwrap_or(f64::NAN),
            // Temperature (°C)
            self.atmp.unwrap_or(f64::NAN),
            self.atmp_compensated.unwrap_or(f64::NAN),
            // Relative humidity (%)
            self.rhum.unwrap_or(f64::NAN),
            self.rhum_compensated.unwrap_or(f64::NAN),
            // CO2 (ppm)
            self.rco2.unwrap_or(f64::NAN),
            // VOC
            self.tvoc_index.unwrap_or(f64::NAN),
            self.tvoc_raw.unwrap_or(f64::NAN),
            // NOx
            self.nox_index.unwrap_or(f64::NAN),
            self.nox_raw.unwrap_or(f64::NAN),
            // WiFi RSSI (dBm)
            self.wifi.map(|v| v as f64).unwrap_or(f64::NAN),
        ]
    }
}

/// Names for the 23 RRD data sources (must match to_rrd_values order).
/// RRD DS names are limited to 19 characters and [A-Za-z0-9_-] only.
pub const RRD_DS_NAMES: [&str; 23] = [
    // PM concentrations
    "pm01",
    "pm02",
    "pm10",
    // Standard PM concentrations
    "pm01_std",
    "pm02_std",
    "pm10_std",
    // Particle counts
    "pm003_count",
    "pm005_count",
    "pm01_count",
    "pm02_count",
    "pm50_count",
    "pm10_count",
    // Compensated PM
    "pm02_comp",
    // Temperature
    "atmp",
    "atmp_comp",
    // Humidity
    "rhum",
    "rhum_comp",
    // CO2
    "rco2",
    // VOC
    "tvoc_index",
    "tvoc_raw",
    // NOx
    "nox_index",
    "nox_raw",
    // WiFi RSSI
    "wifi",
];

/// Sensor configuration entry (stored in config.toml and passed to web UI)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorConfig {
    /// Unique identifier (slug, e.g. "living-room")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Base URL of the sensor (e.g. "http://air01.localdomain")
    pub base_url: String,
    /// How often to poll the sensor in seconds (default 60)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// Whether polling is active
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_poll_interval() -> u64 {
    60
}

fn default_true() -> bool {
    true
}

/// Temperature unit preference for UI display and graph labels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    pub fn is_fahrenheit(self) -> bool {
        self == TempUnit::Fahrenheit
    }
}

/// Graph configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    /// How often to regenerate pre-rendered graphs in seconds
    #[serde(default = "default_graph_regen_interval")]
    pub regeneration_interval_secs: u64,
    /// Graph image width in pixels
    #[serde(default = "default_graph_width")]
    pub width: u32,
    /// Graph image height in pixels
    #[serde(default = "default_graph_height")]
    pub height: u32,
    /// Whether temperatures are shown in Celsius or Fahrenheit
    #[serde(default)]
    pub temp_unit: TempUnit,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            regeneration_interval_secs: default_graph_regen_interval(),
            width: default_graph_width(),
            height: default_graph_height(),
            temp_unit: TempUnit::default(),
        }
    }
}

fn default_graph_regen_interval() -> u64 {
    300
}

fn default_graph_width() -> u32 {
    800
}

fn default_graph_height() -> u32 {
    100
}

/// Graph category for pre-rendered graphs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphCategory {
    Particulate,
    Chemical,
    Atmospheric,
}

impl GraphCategory {
    pub fn slug(&self) -> &'static str {
        match self {
            GraphCategory::Particulate => "pm",
            GraphCategory::Chemical => "chem",
            GraphCategory::Atmospheric => "atm",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            GraphCategory::Particulate => "Particulate Matter",
            GraphCategory::Chemical => "Chemical",
            GraphCategory::Atmospheric => "Atmospheric",
        }
    }

    pub fn all() -> &'static [GraphCategory] {
        &[
            GraphCategory::Particulate,
            GraphCategory::Chemical,
            GraphCategory::Atmospheric,
        ]
    }

    pub fn y_label(&self) -> &'static str {
        match self {
            GraphCategory::Particulate => "Counts (#/dl)",
            GraphCategory::Chemical => "CO\u{2082} (ppm)",
            GraphCategory::Atmospheric => "Temperature (\u{00b0}C)",
        }
    }
}

/// Time range for pre-rendered graphs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphRange {
    H1,
    H12,
    H24,
    H48,
    W2,
    M1,
    Y1,
    Y5,
}

impl GraphRange {
    pub fn slug(&self) -> &'static str {
        match self {
            GraphRange::H1 => "1h",
            GraphRange::H12 => "12h",
            GraphRange::H24 => "24h",
            GraphRange::H48 => "48h",
            GraphRange::W2 => "2w",
            GraphRange::M1 => "1m",
            GraphRange::Y1 => "1y",
            GraphRange::Y5 => "5y",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            GraphRange::H1 => "1 Hour",
            GraphRange::H12 => "12 Hours",
            GraphRange::H24 => "24 Hours",
            GraphRange::H48 => "48 Hours",
            GraphRange::W2 => "2 Weeks",
            GraphRange::M1 => "1 Month",
            GraphRange::Y1 => "1 Year",
            GraphRange::Y5 => "5 Years",
        }
    }

    /// Duration back from now that this range covers
    pub fn duration(&self) -> std::time::Duration {
        match self {
            GraphRange::H1 => std::time::Duration::from_secs(3600),
            GraphRange::H12 => std::time::Duration::from_secs(12 * 3600),
            GraphRange::H24 => std::time::Duration::from_secs(24 * 3600),
            GraphRange::H48 => std::time::Duration::from_secs(48 * 3600),
            GraphRange::W2 => std::time::Duration::from_secs(14 * 86400),
            GraphRange::M1 => std::time::Duration::from_secs(30 * 86400),
            GraphRange::Y1 => std::time::Duration::from_secs(365 * 86400),
            GraphRange::Y5 => std::time::Duration::from_secs(5 * 365 * 86400),
        }
    }

    pub fn all() -> &'static [GraphRange] {
        &[
            GraphRange::H1,
            GraphRange::H12,
            GraphRange::H24,
            GraphRange::H48,
            GraphRange::W2,
            GraphRange::M1,
            GraphRange::Y1,
            GraphRange::Y5,
        ]
    }
}

/// Status of a sensor as seen by the poller
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorStatus {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub poll_interval_secs: u64,
    pub enabled: bool,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    pub latest_reading: Option<SensorReading>,
}

/// Request body for creating/updating a sensor via API
#[derive(Debug, Deserialize)]
pub struct SensorRequest {
    pub name: String,
    pub base_url: String,
    pub poll_interval_secs: Option<u64>,
    pub enabled: Option<bool>,
}

/// Request body for updating graph config via API
#[derive(Debug, Deserialize)]
pub struct GraphConfigRequest {
    pub regeneration_interval_secs: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub temp_unit: Option<TempUnit>,
}
