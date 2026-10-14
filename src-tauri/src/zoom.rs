//! Per-site page zoom.
//!
//! A site is the scheme, host, and port of the page address. `https://example.com`
//! and `http://example.com:8080` each keep their own level. The level is one of
//! the fixed steps below, and `1.0` is not stored because that is the default.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const ZOOM_STEPS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoomStatus {
    pub zoom: f64,
    pub percent: u16,
}

pub fn site_key(url: &str) -> Option<String> {
    let parsed = tauri::Url::parse(url).ok()?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let port = parsed.port_or_known_default()?;
    Some(format!("{scheme}://{host}:{port}"))
}

pub fn nearest_step(current: f64) -> f64 {
    if !current.is_finite() || current <= 0.0 {
        return 1.0;
    }
    ZOOM_STEPS
        .iter()
        .copied()
        .min_by(|left, right| {
            let left_distance = (current - left).abs();
            let right_distance = (current - right).abs();
            left_distance
                .partial_cmp(&right_distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(1.0)
}

pub fn step_zoom(current: f64, delta: i32) -> f64 {
    let current = nearest_step(current);
    let index = ZOOM_STEPS
        .iter()
        .position(|step| (*step - current).abs() < 0.001)
        .unwrap_or(7);
    let next = (index as i32 + delta).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
    ZOOM_STEPS[next]
}

pub fn zoom_percent(factor: f64) -> u16 {
    (nearest_step(factor) * 100.0).round().clamp(1.0, 500.0) as u16
}

pub fn zoom_status(factor: f64) -> ZoomStatus {
    let zoom = nearest_step(factor);
    ZoomStatus {
        zoom,
        percent: zoom_percent(zoom),
    }
}

pub struct SiteZoomMemory {
    path: PathBuf,
    levels: Mutex<BTreeMap<String, f64>>,
}

impl SiteZoomMemory {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let levels = load(&path);
        Self {
            path,
            levels: Mutex::new(levels),
        }
    }

    pub fn factor_for_url(&self, url: &str) -> f64 {
        site_key(url).map(|key| self.get(&key)).unwrap_or(1.0)
    }

    pub fn get(&self, site: &str) -> f64 {
        self.lock().get(site).copied().unwrap_or(1.0)
    }

    /// Store a step for `site`. `1.0` removes the site so the default returns.
    pub fn remember(&self, site: &str, factor: f64) {
        let factor = nearest_step(factor);
        let mut levels = self.lock();
        if (factor - 1.0).abs() < 0.001 {
            levels.remove(site);
        } else {
            levels.insert(site.to_string(), factor);
        }
        self.store(&levels);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, f64>> {
        self.levels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn store(&self, levels: &BTreeMap<String, f64>) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(body) = serde_json::to_vec_pretty(levels) {
            let _ = fs::write(&self.path, body);
        }
    }
}

fn load(path: &Path) -> BTreeMap<String, f64> {
    let Ok(bytes) = fs::read(path) else {
        return BTreeMap::new();
    };
    let Ok(raw) = serde_json::from_slice::<BTreeMap<String, f64>>(&bytes) else {
        return BTreeMap::new();
    };
    raw.into_iter()
        .filter_map(|(key, value)| {
            if !value.is_finite() || value <= 0.0 {
                return None;
            }
            Some((key, nearest_step(value)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_file() -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("neurobrowser-zoom-{nanos}-{n}.json"))
    }

    #[test]
    fn a_site_is_scheme_host_and_port() {
        assert_eq!(
            site_key("https://Example.com/docs?q=1#top").as_deref(),
            Some("https://example.com:443")
        );
        assert_eq!(
            site_key("http://example.com./page").as_deref(),
            Some("http://example.com:80")
        );
        assert_eq!(
            site_key("https://example.com:8443/a").as_deref(),
            Some("https://example.com:8443")
        );
        assert_eq!(site_key("about:blank"), None);
        assert_eq!(site_key("file:///tmp/a.html"), None);
        assert_eq!(site_key("not a url"), None);
    }

    #[test]
    fn zoom_steps_move_one_notch_and_stop_at_the_ends() {
        assert_eq!(step_zoom(1.0, 1), 1.1);
        assert_eq!(step_zoom(1.0, -1), 0.9);
        assert_eq!(step_zoom(0.25, -1), 0.25);
        assert_eq!(step_zoom(5.0, 1), 5.0);
        assert_eq!(step_zoom(1.12, 1), 1.25);
        assert_eq!(zoom_percent(0.33), 33);
        assert_eq!(zoom_percent(1.0), 100);
        assert_eq!(zoom_percent(1.1), 110);
    }

    #[test]
    fn each_site_keeps_its_own_zoom_and_reset_is_the_default() {
        let path = temp_file();
        let memory = SiteZoomMemory::open(&path);
        let docs = "https://docs.example:443";
        let news = "https://news.example:443";
        memory.remember(docs, 1.5);
        memory.remember(news, 0.8);
        assert_eq!(memory.get(docs), 1.5);
        assert_eq!(memory.get(news), 0.8);
        assert_eq!(memory.factor_for_url("https://docs.example/guide"), 1.5);
        assert_eq!(memory.factor_for_url("https://other.example/"), 1.0);
        assert_eq!(memory.factor_for_url("about:blank"), 1.0);

        memory.remember(docs, 1.0);
        assert_eq!(memory.get(docs), 1.0);
        assert_eq!(memory.get(news), 0.8);

        let reloaded = SiteZoomMemory::open(&path);
        assert_eq!(reloaded.get(docs), 1.0);
        assert_eq!(reloaded.get(news), 0.8);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_broken_zoom_file_starts_empty() {
        let path = temp_file();
        fs::write(&path, b"{not json").unwrap();
        let memory = SiteZoomMemory::open(&path);
        assert_eq!(memory.get("https://example.com:443"), 1.0);
        let _ = fs::remove_file(path);
    }
}
