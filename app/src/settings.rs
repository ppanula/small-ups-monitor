//! Tiny key=value settings file in %APPDATA%\Small UPS Monitor\settings.ini.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const DATA_DIR: &str = "Small UPS Monitor";
/// Folder used by pre-1.0 builds (then called "upsmon").
const LEGACY_DATA_DIR: &str = "upsmon";

pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(DATA_DIR))
}

/// Move settings and the event log from the pre-1.0 folder, once.
pub fn migrate_legacy_dir() {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return;
    };
    let (old, new) = (appdata.join(LEGACY_DATA_DIR), appdata.join(DATA_DIR));
    if old.is_dir() && !new.exists() {
        let _ = fs::rename(old, new);
    }
}

/// Key for the window that has no UPS yet (also where the old single-window
/// position is migrated to).
pub const DEFAULT_POS: &str = "default";

pub struct Settings {
    /// Window position per UPS id.
    pub positions: BTreeMap<String, (i32, i32)>,
    pub topmost: bool,
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            positions: BTreeMap::new(),
            topmost: false,
            notify: true,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("settings.ini"))
    }

    pub fn load() -> Self {
        let mut s = Self::default();
        let Some(text) = Self::path().and_then(|p| fs::read_to_string(p).ok()) else {
            return s;
        };
        let (mut legacy_x, mut legacy_y) = (None, None);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "x" => legacy_x = v.parse().ok(),
                "y" => legacy_y = v.parse().ok(),
                "topmost" => s.topmost = v == "1",
                "notify" => s.notify = v != "0",
                _ => {
                    if let Some(id) = k.strip_prefix("pos.")
                        && let Some((x, y)) = v.split_once(',')
                        && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
                    {
                        s.positions.insert(id.to_string(), (x, y));
                    }
                }
            }
        }
        if let (Some(x), Some(y)) = (legacy_x, legacy_y) {
            s.positions.entry(DEFAULT_POS.to_string()).or_insert((x, y));
        }
        s
    }

    pub fn save(&self) {
        let (Some(dir), Some(path)) = (data_dir(), Self::path()) else {
            return;
        };
        let _ = fs::create_dir_all(dir);
        let mut out = format!("topmost={}\nnotify={}\n", u8::from(self.topmost), u8::from(self.notify));
        for (id, (x, y)) in &self.positions {
            out += &format!("pos.{id}={x},{y}\n");
        }
        let _ = fs::write(path, out);
    }
}
