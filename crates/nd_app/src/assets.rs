//! Data files that drive the generators, hot-reloaded by polling their
//! modification time once a second. Missing files are written with defaults so
//! they are easy to find and edit.

use bevy::prelude::*;
use nd_procgen::{BuildingRules, Palette};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const PALETTE_PATH: &str = "assets/palette.ron";
pub const RULES_PATH: &str = "assets/rules/buildings.ron";

#[derive(Resource)]
pub struct GameAssets {
    pub palette: Palette,
    pub rules: BuildingRules,
    palette_mtime: Option<SystemTime>,
    rules_mtime: Option<SystemTime>,
    poll_timer: f32,
    /// Set for one frame when the palette or rules changed.
    pub palette_changed: bool,
    pub rules_changed: bool,
    pub last_error: Option<String>,
}

pub struct GameAssetsPlugin;

impl Plugin for GameAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GameAssets::load()).add_systems(Update, poll_assets);
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn load_or_write<T: serde::Serialize + serde::de::DeserializeOwned + Default>(
    path: &str,
) -> (T, Option<SystemTime>, Option<String>) {
    let p = PathBuf::from(path);
    match std::fs::read_to_string(&p) {
        Ok(text) => match ron::from_str::<T>(&text) {
            Ok(v) => (v, mtime(&p), None),
            Err(e) => (T::default(), mtime(&p), Some(format!("{path}: {e}"))),
        },
        Err(_) => {
            let v = T::default();
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = ron::ser::to_string_pretty(&v, ron::ser::PrettyConfig::default()) {
                let _ = std::fs::write(&p, text);
            }
            (v, mtime(&p), None)
        }
    }
}

impl GameAssets {
    pub fn load() -> Self {
        let (palette, palette_mtime, e1) = load_or_write::<Palette>(PALETTE_PATH);
        let (rules, rules_mtime, e2) = load_or_write::<BuildingRules>(RULES_PATH);
        Self {
            palette,
            rules,
            palette_mtime,
            rules_mtime,
            poll_timer: 0.0,
            palette_changed: false,
            rules_changed: false,
            last_error: e1.or(e2),
        }
    }
}

fn poll_assets(time: Res<Time>, mut assets: ResMut<GameAssets>) {
    assets.palette_changed = false;
    assets.rules_changed = false;
    assets.poll_timer += time.delta_secs();
    if assets.poll_timer < 1.0 {
        return;
    }
    assets.poll_timer = 0.0;

    let pm = mtime(Path::new(PALETTE_PATH));
    if pm.is_some() && pm != assets.palette_mtime {
        assets.palette_mtime = pm;
        match std::fs::read_to_string(PALETTE_PATH).map_err(|e| e.to_string()).and_then(|t| ron::from_str::<Palette>(&t).map_err(|e| e.to_string())) {
            Ok(p) => {
                assets.palette = p;
                assets.palette_changed = true;
                assets.last_error = None;
                info!("palette reloaded");
            }
            Err(e) => assets.last_error = Some(format!("palette: {e}")),
        }
    }
    let rm = mtime(Path::new(RULES_PATH));
    if rm.is_some() && rm != assets.rules_mtime {
        assets.rules_mtime = rm;
        match std::fs::read_to_string(RULES_PATH).map_err(|e| e.to_string()).and_then(|t| ron::from_str::<BuildingRules>(&t).map_err(|e| e.to_string())) {
            Ok(r) => {
                assets.rules = r;
                assets.rules_changed = true;
                assets.last_error = None;
                info!("building rules reloaded");
            }
            Err(e) => assets.last_error = Some(format!("rules: {e}")),
        }
    }
}
