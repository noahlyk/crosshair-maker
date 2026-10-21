use std::path::PathBuf;

use crate::types::{AppConfig, CrosshairProject};

pub fn config_path() -> PathBuf {
    let mut path = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("crosshair-maker");
    path.push("config.json");
    path
}

fn dirs_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(dir));
        }
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join(".config"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join("Library/Application Support"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Some(PathBuf::from(appdata));
        }
    }
    None
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str(&data) {
            return config;
        }
    }
    AppConfig::default()
}

pub fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, data);
    }
}

pub fn add_to_recent(config: &mut AppConfig, path: PathBuf) {
    config.recent_crosshairs.retain(|p| p != &path);
    config.recent_crosshairs.insert(0, path);
    if config.recent_crosshairs.len() > 20 {
        config.recent_crosshairs.truncate(20);
    }
    save_config(config);
}

pub fn project_dir() -> PathBuf {
    let mut path = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("crosshair-maker");
    path.push("projects");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Export "current" crosshair in the projects directory.
/// Saves as current.png (static) or current.apng (animated), deleting the other.
pub fn save_current_exports(pieces: &[crate::types::Piece], groups: &[crate::types::DynamicEffects]) {
    let dir = project_dir();
    let current_path = dir.join("current.json"); // virtual path for extension swapping
    crate::preview::save_exports(&current_path, pieces, groups);
}

pub fn save_project(project: &CrosshairProject, config: &mut AppConfig, path: Option<PathBuf>) -> Option<PathBuf> {
    let path = path.unwrap_or_else(|| {
        let mut p = project_dir();
        p.push(format!("{}.json", sanitize_filename(&project.name)));
        p
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(project) {
        if std::fs::write(&path, data).is_ok() {
            add_to_recent(config, path.clone());
            return Some(path);
        }
    }
    None
}

pub fn load_project(path: &PathBuf) -> Result<CrosshairProject, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    // Pre-process legacy files that use h_gap/v_gap instead of left_gap/right_gap/etc.
    let data = preprocess_legacy_cross(&data);
    
    let mut project: CrosshairProject = serde_json::from_str(&data)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;
    
    // Migrate legacy color types
    for piece in &mut project.pieces {
        migrate_color_types(piece);
    }
    
    // Post-process pieces that may still have defaults to fill (from partial migrations)
    for piece in &mut project.pieces {
        migrate_cross_piece(piece);
    }

    // Stable piece ids (links point at them) and the pre-per-piece-effects
    // group format.
    crate::links::assign_uids(&mut project.pieces);
    let mut legacy: Vec<crate::types::DynamicEffects> = std::mem::take(&mut project.dynamic_groups);
    if legacy.is_empty() {
        legacy.extend(project.dynamic_effects.take());
    }
    if !legacy.is_empty() {
        crate::links::migrate_legacy_groups(&mut project.pieces, &legacy);
    }

    Ok(project)
}

fn preprocess_legacy_cross(data: &str) -> String {
    // Check if this is a legacy file with h_gap/v_gap fields
    if !data.contains("\"h_gap\"") {
        return data.to_string();
    }
    
    let mut result = data.to_string();
    
    // Replace h_gap with left_gap and right_gap
    result = result.replace("\"h_gap\":", "\"left_gap\":");
    // Insert right_gap after left_gap: value, (need to find pattern and replace)
    // Actually, simpler: replace h_gap with left_gap, then add right_gap with same value
    
    // For v_gap -> top_gap and bottom_gap
    result = result.replace("\"v_gap\":", "\"top_gap\":");
    
    // length -> left_length, right_length, top_length, bottom_length
    result = result.replace("\"length\":", "\"left_length\":");
    
    // thickness -> left_thickness, etc.
    result = result.replace("\"thickness\":", "\"left_thickness\":");
    
    // Now we need to duplicate values. This is tricky with simple string replacement.
    // Instead, let's use a more robust approach: parse as Value, transform, serialize
    
    use serde_json::Value;
    match serde_json::from_str::<Value>(&result) {
        Ok(mut value) => {
            transform_legacy_cross_value(&mut value);
            serde_json::to_string(&value).unwrap_or(result)
        }
        Err(_) => result,
    }
}

fn transform_legacy_cross_value(value: &mut serde_json::Value) {
    use serde_json::Value;
    
    if let Value::Object(ref mut map) = value {
        if let Some(ref mut pieces) = map.get_mut("pieces") {
            if let Value::Array(ref mut arr) = pieces {
                for item in arr.iter_mut() {
                    if let Value::Object(ref mut piece_map) = item {
                        if let Some(Value::Object(ref mut cross)) = piece_map.get_mut("Cross") {
                            // Transform h_gap -> left_gap, right_gap
                            if let Some(h_gap) = cross.get("left_gap").cloned() {
                                cross.insert("right_gap".to_string(), h_gap.clone());
                                cross.insert("top_gap".to_string(), h_gap.clone());
                                cross.insert("bottom_gap".to_string(), h_gap.clone());
                            }
                            // Transform v_gap -> top_gap, bottom_gap (already replaced in string pass, but ensure both)
                            // Transform length -> all 4 lengths
                            if let Some(length) = cross.get("left_length").cloned() {
                                cross.insert("right_length".to_string(), length.clone());
                                cross.insert("top_length".to_string(), length.clone());
                                cross.insert("bottom_length".to_string(), length.clone());
                            }
                            // Transform thickness -> all 4 thicknesses
                            if let Some(thickness) = cross.get("left_thickness").cloned() {
                                cross.insert("right_thickness".to_string(), thickness.clone());
                                cross.insert("top_thickness".to_string(), thickness.clone());
                                cross.insert("bottom_thickness".to_string(), thickness.clone());
                            }
                        }
                    }
                }
            }
        }
    }
}

fn migrate_color_types(piece: &mut crate::types::Piece) {
    use crate::types::Piece;
    match piece {
        Piece::Cross { color_type, .. }
        | Piece::Dot { color_type, .. }
        | Piece::Line { color_type, .. }
        | Piece::Rectangle { color_type, .. }
        | Piece::HappyFace { color_type, .. } => {
            color_type.migrate();
        }
        Piece::RectPattern { obj, .. } | Piece::CircPattern { obj, .. } => {
            migrate_color_types(obj);
        }
    }
}

fn migrate_cross_piece(piece: &mut crate::types::Piece) {
    use crate::types::Piece;
    let Piece::Cross {
        origin: _,
        ref mut left_gap,
        ref mut right_gap,
        ref mut top_gap,
        ref mut bottom_gap,
        ref mut left_thickness,
        ref mut right_thickness,
        ref mut top_thickness,
        ref mut bottom_thickness,
        ref mut left_length,
        ref mut right_length,
        ref mut top_length,
        ref mut bottom_length,
        ref mut lock_gap,
        ref mut lock_all,
        ..
    } = piece else {
        return;
    };
    
    if *left_gap == 0 && *right_gap == 0 && *top_gap == 0 && *bottom_gap == 0 {
        let defaults = crate::types::default_cross_piece();
        if let crate::types::Piece::Cross { 
            left_gap: d_lg, right_gap: d_rg, top_gap: d_tg, bottom_gap: d_bg,
            ..
        } = defaults {
            *left_gap = d_lg;
            *right_gap = d_rg;
            *top_gap = d_tg;
            *bottom_gap = d_bg;
        }
    }
    if *left_thickness == 0 && *right_thickness == 0 && *top_thickness == 0 && *bottom_thickness == 0 {
        let defaults = crate::types::default_cross_piece();
        if let crate::types::Piece::Cross { 
            left_thickness: d_lt, right_thickness: d_rt, top_thickness: d_tt, bottom_thickness: d_bt,
            ..
        } = defaults {
            *left_thickness = d_lt;
            *right_thickness = d_rt;
            *top_thickness = d_tt;
            *bottom_thickness = d_bt;
        }
    }
    if *left_length == 0 && *right_length == 0 && *top_length == 0 && *bottom_length == 0 {
        let defaults = crate::types::default_cross_piece();
        if let crate::types::Piece::Cross { 
            left_length: d_ll, right_length: d_rl, top_length: d_tl, bottom_length: d_bl,
            ..
        } = defaults {
            *left_length = d_ll;
            *right_length = d_rl;
            *top_length = d_tl;
            *bottom_length = d_bl;
        }
    }
    if !*lock_gap {
        *lock_gap = true;
    }
    if !*lock_all {
        *lock_all = true;
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}
