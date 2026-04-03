use anyhow::{Context, Result};
use reqwest::Client;

use crate::models::SensorReading;

/// Fetches the current reading from an AirGradient sensor.
/// `base_url` is e.g. "http://air01.localdomain"
pub async fn fetch_current(client: &Client, base_url: &str) -> Result<SensorReading> {
    // Normalise: strip any trailing path the user may have included, then re-append the endpoint.
    let base = base_url
        .trim_end_matches('/')
        .trim_end_matches("/measures/current")
        .trim_end_matches('/');
    let url = format!("{base}/measures/current");
    let reading = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .with_context(|| format!("HTTP request failed for {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error response from {url}"))?
        .json::<SensorReading>()
        .await
        .with_context(|| format!("Failed to parse JSON from {url}"))?;
    Ok(reading)
}
