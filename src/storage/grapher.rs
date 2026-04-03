/// Pre-rendered RRD graph generation using `rrd::ops::graph`.
///
/// For each sensor, generates PNG images for 3 categories × 5 time ranges = 15 graphs.
/// Stored at `<data_dir>/graphs/<sensor_id>/<category>_<range>.png`
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::{Context, Result};
use rrd::ops::graph::{
    self,
    elements::{self, GraphElement, VarName},
    props::{self, ColorTag, GraphProps, Labels, RightYAxis, Size},
};
use rrd::ConsolidationFn;
use tracing::info;

use crate::models::{GraphCategory, GraphRange, TempUnit};

fn c(r: u8, g: u8, b: u8) -> graph::Color {
    graph::Color {
        red: r,
        green: g,
        blue: b,
        alpha: None,
    }
}

fn vn(s: &str) -> VarName {
    VarName::new(s).expect("hard-coded var names are valid")
}

pub struct Grapher {
    data_dir: PathBuf,
    /// Serialise all librrd graph calls
    lock: Mutex<()>,
}

impl Grapher {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lock: Mutex::new(()),
        }
    }

    fn rrd_path(&self, sensor_id: &str) -> PathBuf {
        self.data_dir.join(format!("{sensor_id}.rrd"))
    }

    fn graph_dir(&self, sensor_id: &str) -> PathBuf {
        self.data_dir.join("graphs").join(sensor_id)
    }

    /// Regenerate all graphs for a given sensor.
    pub fn regenerate_all(
        &self,
        sensor_id: &str,
        sensor_name: &str,
        base_url: &str,
        width: u32,
        height: u32,
        temp_unit: TempUnit,
    ) -> Result<()> {
        let graph_dir = self.graph_dir(sensor_id);
        std::fs::create_dir_all(&graph_dir)?;

        let rrd_path = self.rrd_path(sensor_id);
        if !rrd_path.exists() {
            return Ok(());
        }

        for &category in GraphCategory::all() {
            for &range in GraphRange::all() {
                if let Err(e) = self.generate_graph(sensor_id, sensor_name, base_url, category, range, width, height, temp_unit) {
                    tracing::warn!(
                        "Failed to generate {}/{} graph for {sensor_id}: {e}",
                        category.slug(),
                        range.slug()
                    );
                }
            }
        }
        Ok(())
    }

    fn generate_graph(
        &self,
        sensor_id: &str,
        sensor_name: &str,
        base_url: &str,
        category: GraphCategory,
        range: GraphRange,
        width: u32,
        height: u32,
        temp_unit: TempUnit,
    ) -> Result<()> {
        let rrd_path = self.rrd_path(sensor_id);
        let out_path = self
            .graph_dir(sensor_id)
            .join(format!("{}_{}.png", category.slug(), range.slug()));

        info!("Generating graph {}", out_path.display());

        let now = SystemTime::now();
        let start = now
            .checked_sub(range.duration())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        // Background colors
        let mut colors: HashMap<ColorTag, graph::Color> = HashMap::new();
        colors.insert(ColorTag::Back, c(0x1a, 0x1a, 0x2e));
        colors.insert(ColorTag::Canvas, c(0x16, 0x21, 0x3e));
        colors.insert(ColorTag::Font, c(0xe0, 0xe0, 0xe0));
        colors.insert(ColorTag::Axis, c(0x88, 0x88, 0x88));
        colors.insert(ColorTag::Grid, c(0x44, 0x44, 0x44));
        colors.insert(ColorTag::MGrid, c(0x66, 0x66, 0x66));
        colors.insert(ColorTag::Frame, c(0x88, 0x88, 0x88));
        colors.insert(ColorTag::Arrow, c(0xe0, 0xe0, 0xe0));

        let props = GraphProps {
            time_range: props::TimeRange {
                start: Some(start),
                end: Some(now),
                ..Default::default()
            },
            labels: Labels {
                title: Some(format!(
                    "{} ({}) - {} - {}",
                    sensor_name,
                    parse_host(base_url),
                    category.label(),
                    range.label(),
                )),
                vertical_label: Some(match (category, temp_unit) {
                    (GraphCategory::Atmospheric, TempUnit::Fahrenheit) => "Temperature (\u{00b0}F)".to_string(),
                    _ => category.y_label().to_string(),
                }),
            },
            size: Size {
                width: Some(width),
                height: Some(height),
                ..Default::default()
            },
            right_y_axis: right_y_axis_for(category),
            misc: props::Misc {
                colors,
                ..Default::default()
            },
            ..Default::default()
        };

        let elements = build_elements(&rrd_path, category, temp_unit);

        let _guard = self.lock.lock().unwrap();
        let (png_bytes, _meta) = graph::graph(props::ImageFormat::Png, &props, &elements)
            .with_context(|| format!("rrd_graph failed for {sensor_id}/{}/{}", category.slug(), range.slug()))?;

        std::fs::write(&out_path, &png_bytes)
            .with_context(|| format!("Failed to write graph PNG to {}", out_path.display()))?;

        Ok(())
    }

    /// Return the filesystem path to a pre-rendered graph PNG.
    pub fn graph_path(&self, sensor_id: &str, category: GraphCategory, range: GraphRange) -> PathBuf {
        self.graph_dir(sensor_id)
            .join(format!("{}_{}.png", category.slug(), range.slug()))
    }

    /// Parse category slug → GraphCategory
    pub fn parse_category(slug: &str) -> Option<GraphCategory> {
        match slug {
            "pm" => Some(GraphCategory::Particulate),
            "chem" => Some(GraphCategory::Chemical),
            "atm" => Some(GraphCategory::Atmospheric),
            _ => None,
        }
    }

    /// Parse range slug → GraphRange
    pub fn parse_range(slug: &str) -> Option<GraphRange> {
        match slug {
            "1h"  => Some(GraphRange::H1),
            "12h" => Some(GraphRange::H12),
            "24h" => Some(GraphRange::H24),
            "48h" => Some(GraphRange::H48),
            "2w"  => Some(GraphRange::W2),
            "1m"  => Some(GraphRange::M1),
            "1y"  => Some(GraphRange::Y1),
            "5y"  => Some(GraphRange::Y5),
            _ => None,
        }
    }
}

// SAFETY: All librrd calls are serialised via the internal Mutex.
unsafe impl Sync for Grapher {}

/// Right y-axis configuration for each graph category.
/// Returns None for categories with a single unit on one axis.
fn right_y_axis_for(category: GraphCategory) -> Option<RightYAxis> {
    match category {
        // Right axis: ug/m3; PM values are plotted *10, so scale=0.1 recovers original units.
        GraphCategory::Particulate => Some(RightYAxis {
            scale: 0.1,
            shift: 0,
            label: Some("Concentration (ug/m3)".to_string()),
            formatter: None,
            format: None,
        }),
        // Right axis: Index (1-500); index values are plotted *4 to reach CO2 scale, so scale=0.25.
        GraphCategory::Chemical => Some(RightYAxis {
            scale: 0.25,
            shift: 0,
            label: Some("Index (1-500)".to_string()),
            formatter: None,
            format: None,
        }),
        // Right axis: % humidity; humidity values are plotted /2, so scale=2.0.
        GraphCategory::Atmospheric => Some(RightYAxis {
            scale: 2.0,
            shift: 0,
            label: Some("Humidity (%)".to_string()),
            formatter: None,
            format: None,
        }),
    }
}

/// Create a DEF element using the DS name as the variable name.
fn def(rrd_path: &Path, ds: &str) -> (VarName, GraphElement) {
    let vname = vn(ds);
    let el = elements::Def {
        var_name: vname.clone(),
        rrd: rrd_path.to_path_buf(),
        ds_name: ds.to_string(),
        consolidation_fn: ConsolidationFn::Avg,
        step: None,
        start: None,
        end: None,
        reduce: None,
    }
    .into();
    (vname, el)
}

fn line(v: VarName, r: u8, g: u8, b: u8, legend: &str) -> GraphElement {
    elements::Line {
        width: 2.0,
        value: v,
        color: Some(elements::ColorWithLegend {
            color: c(r, g, b),
            legend: Some(legend.into()),
        }),
        stack: false,
        skip_scale: false,
        dashes: None,
    }
    .into()
}

/// Append VDEF + GPRINT elements to show Last/Avg/Min/Max in the legend for a metric.
/// `stat_src` is the DEF or CDEF variable name to compute stats from (unscaled, original units).
/// `line_end` is appended to the final GPRINT format (typically `"\\l"` to terminate the row).
fn stat_els(stat_src: &str, line_end: &str) -> Vec<GraphElement> {
    let last_vn = vn(&format!("{}sl", stat_src));
    let avg_vn  = vn(&format!("{}sa", stat_src));
    let min_vn  = vn(&format!("{}sn", stat_src));
    let max_vn  = vn(&format!("{}sx", stat_src));
    vec![
        elements::VDef { var_name: last_vn.clone(), rpn: format!("{},LAST",    stat_src) }.into(),
        elements::VDef { var_name: avg_vn.clone(),  rpn: format!("{},AVERAGE", stat_src) }.into(),
        elements::VDef { var_name: min_vn.clone(),  rpn: format!("{},MINIMUM", stat_src) }.into(),
        elements::VDef { var_name: max_vn.clone(),  rpn: format!("{},MAXIMUM", stat_src) }.into(),
        elements::GPrint { var_name: last_vn, format: "Last\\: %6.1lf ".to_string() }.into(),
        elements::GPrint { var_name: avg_vn,  format: "Avg\\: %6.1lf ".to_string() }.into(),
        elements::GPrint { var_name: min_vn,  format: "Min\\: %6.1lf ".to_string() }.into(),
        elements::GPrint { var_name: max_vn,  format: format!("Max\\: %6.1lf{}", line_end) }.into(),
    ]
}

fn build_elements(rrd_path: &Path, category: GraphCategory, temp_unit: TempUnit) -> Vec<GraphElement> {
    match category {
        // Left (#/dl): pm003_count, pm005_count, pm01_count
        // Right (ug/m3): pm01, pm02, pm10  — scaled ×10 so they share the count axis visually
        // Left (#/dl): pm003_count, pm005_count, pm01_count
        // Right (ug/m3): pm01, pm02, pm10 — scaled ×10 so they share the count axis visually.
        // right_y_axis scale=0.1 recovers original ug/m3 values on the right labels.
        GraphCategory::Particulate => {
            let (pm003, d_pm003) = def(rrd_path, "pm003_count");
            let (pm005, d_pm005) = def(rrd_path, "pm005_count");
            let (pm01c, d_pm01c) = def(rrd_path, "pm01_count");
            let (_,     d_pm01)  = def(rrd_path, "pm01");
            let (_,     d_pm02)  = def(rrd_path, "pm02");
            let (_,     d_pm10)  = def(rrd_path, "pm10");
            let (pm01s, c_pm01s) = cdef("pm01s", "pm01", "10,*");
            let (pm02s, c_pm02s) = cdef("pm02s", "pm02", "10,*");
            let (pm10s, c_pm10s) = cdef("pm10s", "pm10", "10,*");

            let mut els = vec![
                d_pm003, d_pm005, d_pm01c, d_pm01, d_pm02, d_pm10,
                c_pm01s, c_pm02s, c_pm10s,
            ];
            // Counts (left axis)
            els.push(line(pm003, 0x00, 0xe5, 0xff, "0.3um #/dl (L)  "));
            els.extend(stat_els("pm003_count", "\\l"));
            els.push(line(pm005, 0x76, 0xff, 0x03, "0.5um #/dl (L)  "));
            els.extend(stat_els("pm005_count", "\\l"));
            els.push(line(pm01c, 0xe0, 0x40, 0xfb, "1.0um #/dl (L)  "));
            els.extend(stat_els("pm01_count", "\\l"));
            // PM concentrations (right axis)
            els.push(line(pm01s, 0xff, 0xea, 0x00, "PM1.0 ug/m3 (R) "));
            els.extend(stat_els("pm01", "\\l"));
            els.push(line(pm02s, 0xff, 0x6d, 0x00, "PM2.5 ug/m3 (R) "));
            els.extend(stat_els("pm02", "\\l"));
            els.push(line(pm10s, 0xdd, 0x00, 0x31, "PM10  ug/m3 (R) "));
            els.extend(stat_els("pm10", "\\l"));
            els
        }

        // Left (ppm): rco2; plus tvoc_index scaled ×4 to approach CO2 ppm range.
        // Right (index 1-500): tvoc_index and nox_index — right_y_axis scale=0.25.
        GraphCategory::Chemical => {
            let (rco2, d_rco2) = def(rrd_path, "rco2");
            let (_,    d_tvoc) = def(rrd_path, "tvoc_index");
            let (_,    d_nox)  = def(rrd_path, "nox_index");
            let (tvocs, c_tvocs) = cdef("tvocs", "tvoc_index", "4,*");
            let (noxs,  c_noxs)  = cdef("noxs",  "nox_index",  "4,*");

            let mut els = vec![d_rco2, d_tvoc, d_nox, c_tvocs, c_noxs];
            els.push(line(rco2,  0xf4, 0xa2, 0x61, "CO\u{2082} ppm (L)   "));
            els.extend(stat_els("rco2", "\\l"));
            els.push(line(tvocs, 0xff, 0xea, 0x00, "VOC Index (R)   "));
            els.extend(stat_els("tvoc_index", "\\l"));
            els.push(line(noxs,  0x3a, 0x0c, 0xa3, "NOx Index (R)   "));
            els.extend(stat_els("nox_index", "\\l"));
            els
        }

        // Left (temp): atmp, atmp_compensated — in °C or °F depending on temp_unit.
        // Right (%): rhum, rhum_compensated — scaled ÷2 to map 0-100% onto the temp axis range;
        // right_y_axis scale=2.0 restores correct percentage labels on the right side.
        GraphCategory::Atmospheric => {
            let (atmp,      d_atmp)      = def(rrd_path, "atmp");
            let (atmp_comp, d_atmp_comp) = def(rrd_path, "atmp_comp");
            let (_,         d_rhum)      = def(rrd_path, "rhum");
            let (_,         d_rhum_comp) = def(rrd_path, "rhum_comp");
            let (rhums,      c_rhums)      = cdef("rhums",     "rhum",      "2,/");
            let (rhum_comps, c_rhum_comps) = cdef("rhumcomps", "rhum_comp", "2,/");

            let mut els = vec![d_atmp, d_atmp_comp, d_rhum, d_rhum_comp, c_rhums, c_rhum_comps];

            // When Fahrenheit, convert via CDEF: F = C * 9 / 5 + 32
            let (t1, t1_stat, t2, t2_stat, unit_sym) = if temp_unit.is_fahrenheit() {
                let (t1f, c_t1f) = cdef("atmpf",     "atmp",      "9,*,5,/,32,+");
                let (t2f, c_t2f) = cdef("atmpcompf", "atmp_comp", "9,*,5,/,32,+");
                els.push(c_t1f);
                els.push(c_t2f);
                (t1f, "atmpf", t2f, "atmpcompf", "\u{00b0}F")
            } else {
                (atmp, "atmp", atmp_comp, "atmp_comp", "\u{00b0}C")
            };

            els.push(line(t1, 0xef, 0x23, 0x3c, &format!("Temp {} (L)      ", unit_sym)));
            els.extend(stat_els(t1_stat, "\\l"));
            els.push(line(t2, 0xff, 0x70, 0x70, &format!("Temp Comp {} (L) ", unit_sym)));
            els.extend(stat_els(t2_stat, "\\l"));
            els.push(line(rhums,      0x4c, 0xc9, 0xf0, "Humidity % (R)      "));
            els.extend(stat_els("rhum", "\\l"));
            els.push(line(rhum_comps, 0x00, 0x7a, 0xa5, "Humidity Comp % (R) "));
            els.extend(stat_els("rhum_comp", "\\l"));
            els
        }
    }
}

/// Extract the first DNS label from a base URL.
/// e.g. "http://air01.localdomain/measures/current" → "air01"
fn parse_host(base_url: &str) -> &str {
    let s = base_url.find("://").map_or(base_url, |i| &base_url[i + 3..]);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.split(':').next().unwrap_or(s); // strip port
    s.split('.').next().unwrap_or(s)
}

/// Build a CDEF element: `out = src_name,rpn_suffix` (RPN expression).
fn cdef(out: &str, src_name: &str, rpn_suffix: &str) -> (VarName, GraphElement) {
    let vname = vn(out);
    let el = elements::CDef {
        var_name: vname.clone(),
        rpn: format!("{},{}", src_name, rpn_suffix),
    }
    .into();
    (vname, el)
}
