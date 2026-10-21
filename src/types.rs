use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_one() -> f64 { 1.0 }

fn default_180() -> f64 { 180.0 }

// ── Dynamic effect chain ────────────────────────────────────────
// Project-level: defines what processing happens where Dynamic pieces are.
// Each effect is a checkbox (enabled) + strength slider + optional params.
// Fixed processing order: invert → dodge → burn → complement → luma_invert → hue_rotate → saturate.

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DynamicEffects {
    #[serde(default)]
    pub invert: EffectSimple,
    #[serde(default)]
    pub dodge: EffectTint,
    #[serde(default)]
    pub burn: EffectTint,
    #[serde(default)]
    pub complement: EffectSimple,
    #[serde(default)]
    pub luma_invert: EffectSimple,
    #[serde(default)]
    pub hue_rotate: EffectHueRotate,
    #[serde(default)]
    pub saturate: EffectSaturate,
    /// Overall opacity of the dynamic effect (0.0 = invisible, 1.0 = full).
    #[serde(default = "default_one")]
    pub opacity: f64,
}

impl Default for DynamicEffects {
    fn default() -> Self {
        Self {
            invert: EffectSimple { enabled: false, strength: 1.0 },
            dodge: EffectTint { enabled: false, strength: 1.0, tint: "#ffffffff".into() },
            burn: EffectTint { enabled: false, strength: 1.0, tint: "#000000ff".into() },
            complement: EffectSimple { enabled: false, strength: 1.0 },
            luma_invert: EffectSimple { enabled: false, strength: 1.0 },
            hue_rotate: EffectHueRotate { enabled: false, strength: 1.0, angle: 180.0 },
            saturate: EffectSaturate { enabled: false, strength: 1.0, amount: 1.0 },
            opacity: 1.0,
        }
    }
}

impl DynamicEffects {
    /// True if at least one effect is enabled.
    pub fn has_any_enabled(&self) -> bool {
        self.invert.enabled || self.dodge.enabled || self.burn.enabled
            || self.complement.enabled || self.luma_invert.enabled
            || self.hue_rotate.enabled || self.saturate.enabled
    }

    /// Write effects to the `.dynamic.cfg` text format for krosshair.
    /// Fixed order: invert, dodge, burn, complement, lumainvert, huerotate, saturate.
    /// Only writes enabled effects.
    pub fn to_cfg_string(&self) -> String {
        let mut s = String::new();
        if self.invert.enabled {
            s.push_str(&format!("invert {:.4}\n", self.invert.strength));
        }
        if self.dodge.enabled {
            let (r, g, b) = hex_to_rgb_f32(&self.dodge.tint);
            s.push_str(&format!("dodge {:.4} {:.4} {:.4} {:.4}\n", self.dodge.strength, r, g, b));
        }
        if self.burn.enabled {
            let (r, g, b) = hex_to_rgb_f32(&self.burn.tint);
            s.push_str(&format!("burn {:.4} {:.4} {:.4} {:.4}\n", self.burn.strength, r, g, b));
        }
        if self.complement.enabled {
            s.push_str(&format!("complement {:.4}\n", self.complement.strength));
        }
        if self.luma_invert.enabled {
            s.push_str(&format!("lumainvert {:.4}\n", self.luma_invert.strength));
        }
        if self.hue_rotate.enabled {
            s.push_str(&format!("huerotate {:.4} {:.4}\n", self.hue_rotate.strength, self.hue_rotate.angle));
        }
        if self.saturate.enabled {
            s.push_str(&format!("saturate {:.4} {:.4}\n", self.saturate.strength, self.saturate.amount));
        }
        if self.opacity < 1.0 {
            s.push_str(&format!("opacity {:.4}\n", self.opacity));
        }
        s
    }

    /// Apply the effect chain to an RGB pixel (CPU-side, for live preview).
    /// Input/output are 0.0-1.0 floats. Fixed order.
    pub fn apply_to_pixel(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        let orig = [r, g, b];
        let mut c = [r, g, b];
        if self.invert.enabled {
            let s = self.invert.strength as f32;
            c = [lerp(c[0], 1.0 - c[0], s), lerp(c[1], 1.0 - c[1], s), lerp(c[2], 1.0 - c[2], s)];
        }
        if self.dodge.enabled {
            let s = self.dodge.strength as f32;
            let (tr, tg, tb) = hex_to_rgb_f32(&self.dodge.tint);
            c = [lerp(c[0], (c[0]+tr).min(1.0), s), lerp(c[1], (c[1]+tg).min(1.0), s), lerp(c[2], (c[2]+tb).min(1.0), s)];
        }
        if self.burn.enabled {
            let s = self.burn.strength as f32;
            let (tr, tg, tb) = hex_to_rgb_f32(&self.burn.tint);
            c = [lerp(c[0], (c[0]-tr).max(0.0), s), lerp(c[1], (c[1]-tg).max(0.0), s), lerp(c[2], (c[2]-tb).max(0.0), s)];
        }
        if self.complement.enabled {
            let s = self.complement.strength as f32;
            let hsl = rgb_to_hsl_f32(c[0], c[1], c[2]);
            let comp = hsl_to_rgb_f32((hsl[0] + 0.5) % 1.0, 1.0, 1.0 - hsl[2]);
            c = [lerp(c[0], comp[0], s), lerp(c[1], comp[1], s), lerp(c[2], comp[2], s)];
        }
        if self.luma_invert.enabled {
            let s = self.luma_invert.strength as f32;
            let hsl = rgb_to_hsl_f32(c[0], c[1], c[2]);
            let inv = hsl_to_rgb_f32(hsl[0], hsl[1], 1.0 - hsl[2]);
            c = [lerp(c[0], inv[0], s), lerp(c[1], inv[1], s), lerp(c[2], inv[2], s)];
        }
        if self.hue_rotate.enabled {
            let s = self.hue_rotate.strength as f32;
            let hsl = rgb_to_hsl_f32(c[0], c[1], c[2]);
            let h = (hsl[0] + self.hue_rotate.angle as f32 / 360.0) % 1.0;
            let rot = hsl_to_rgb_f32(h, hsl[1], 1.0 - hsl[2]);
            c = [lerp(c[0], rot[0], s), lerp(c[1], rot[1], s), lerp(c[2], rot[2], s)];
        }
        if self.saturate.enabled {
            let s = self.saturate.strength as f32;
            let hsl = rgb_to_hsl_f32(c[0], c[1], c[2]);
            let sat = hsl_to_rgb_f32(hsl[0], self.saturate.amount as f32, 1.0 - hsl[2]);
            c = [lerp(c[0], sat[0], s), lerp(c[1], sat[1], s), lerp(c[2], sat[2], s)];
        }
        // Apply overall opacity: blend between original and fully-effected
        if self.opacity < 1.0 {
            let o = self.opacity as f32;
            c = [lerp(orig[0], c[0], o), lerp(orig[1], c[1], o), lerp(orig[2], c[2], o)];
        }
        (c[0], c[1], c[2])
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

fn rgb_to_hsl_f32(r: f32, g: f32, b: f32) -> [f32; 3] {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if d < 0.00001 { return [0.0, 0.0, l]; }
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        ((g - b) / d) % 6.0
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    let h = if h < 0.0 { h + 1.0 } else { h };
    [h, s, l]
}

fn hue_to_rgb_f32(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t < 1.0/6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.0/2.0 { return q; }
    if t < 2.0/3.0 { return p + (q - p) * (2.0/3.0 - t) * 6.0; }
    p
}

fn hsl_to_rgb_f32(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s < 0.00001 { return [l, l, l]; }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    [
        hue_to_rgb_f32(p, q, h + 1.0/3.0),
        hue_to_rgb_f32(p, q, h),
        hue_to_rgb_f32(p, q, h - 1.0/3.0),
    ]
}

/// Simple effect: just enabled + strength.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectSimple {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub strength: f64,
}

impl Default for EffectSimple {
    fn default() -> Self { Self { enabled: false, strength: 1.0 } }
}

/// Tinted effect (dodge/burn): strength + RGB tint color.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectTint {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub strength: f64,
    #[serde(default = "default_white_tint")]
    pub tint: String,
}

fn default_white_tint() -> String { "#ffffffff".to_string() }

impl Default for EffectTint {
    fn default() -> Self { Self { enabled: false, strength: 1.0, tint: "#ffffffff".into() } }
}

/// Hue rotation effect: strength + angle in degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectHueRotate {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub strength: f64,
    #[serde(default = "default_180")]
    pub angle: f64,
}

impl Default for EffectHueRotate {
    fn default() -> Self { Self { enabled: false, strength: 1.0, angle: 180.0 } }
}

/// Saturation effect: strength + amount (0 = desaturate, 1 = max).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectSaturate {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_one")]
    pub strength: f64,
    #[serde(default = "default_one")]
    pub amount: f64,
}

impl Default for EffectSaturate {
    fn default() -> Self { Self { enabled: false, strength: 1.0, amount: 1.0 } }
}

/// Convert a hex color string to RGB floats (0.0-1.0).
fn hex_to_rgb_f32(hex: &str) -> (f32, f32, f32) {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex.get(0..2).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
    let g = u8::from_str_radix(&hex.get(2..4).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
    let b = u8::from_str_radix(&hex.get(4..6).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
    (r, g, b)
}

// ── Legacy types (kept for serde backward compat) ───────────────

/// Old per-mode tags. Only used for deserializing legacy projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DynamicModeTag {
    Invert, Dodge, Burn, Complement, LumaInvert, HueRotate, Saturate,
}

/// Old modes struct. Only used for deserializing legacy projects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DynamicModes {
    #[serde(default)] pub invert: bool,
    #[serde(default)] pub dodge: bool,
    #[serde(default)] pub burn: bool,
    #[serde(default)] pub complement: bool,
    #[serde(default)] pub lumainvert: bool,
    #[serde(default)] pub huerotate: bool,
    #[serde(default)] pub saturate: bool,
}

// ── Color types ─────────────────────────────────────────────────

/// Legacy enum kept only for deserializing old project files.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GradientTransition {
    Loop,
    Bounce,
    SmoothLoop,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LoopMode {
    Bounce,
    Cycle,
}

impl Default for LoopMode {
    fn default() -> Self { LoopMode::Bounce }
}

impl std::fmt::Display for LoopMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopMode::Bounce => write!(f, "Bounce"),
            LoopMode::Cycle => write!(f, "Cycle"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InterpolationMode {
    Smooth,
    Instant,
}

impl Default for InterpolationMode {
    fn default() -> Self { InterpolationMode::Smooth }
}

impl std::fmt::Display for InterpolationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpolationMode::Smooth => write!(f, "Smooth"),
            InterpolationMode::Instant => write!(f, "Instant Cuts"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ColorType {
    Solid,
    Eraser,
    Rainbow {
        #[serde(default = "default_one")]
        saturation: f64,
        #[serde(default = "default_one")]
        lightness: f64,
        #[serde(default = "default_one")]
        alpha: f64,
        #[serde(default = "default_one")]
        speed: f64,
        #[serde(default)]
        reverse: bool,
    },
    GradientCycle {
        #[serde(default)]
        colors: Vec<String>,
        #[serde(default = "default_one")]
        speed: f64,
        #[serde(default)]
        loop_mode: LoopMode,
        #[serde(default)]
        interpolation: InterpolationMode,
        #[serde(default)]
        transition: Option<GradientTransition>,
        #[serde(default)]
        color2: Option<String>,
    },
    /// Dynamic: marks this piece as "apply the project's dynamic effect
    /// chain here".  The piece acts as an eraser in the main crosshair
    /// image and appears in the binary mask.  The actual effect parameters
    /// live on `CrosshairProject::dynamic_effects`.
    ///
    /// Legacy fields (`mode`, `modes`, `tint`, `strength`) are silently
    /// accepted by serde but ignored — effects are project-level now.
    #[serde(alias = "ContrastInvert")]
    Dynamic {
        // Legacy fields — accepted for backward compat, ignored at runtime.
        #[serde(default, alias = "mode")]
        _legacy_mode: Option<DynamicModeTag>,
        #[serde(default)]
        _legacy_modes: Option<DynamicModes>,
        #[serde(default)]
        _legacy_tint: Option<String>,
        #[serde(default)]
        _legacy_strength: Option<f64>,
    },
}

impl Default for ColorType {
    fn default() -> Self {
        ColorType::Solid
    }
}

impl ColorType {
    /// Migrate legacy fields on load.
    pub fn migrate(&mut self) {
        if let ColorType::GradientCycle { colors, color2, transition, loop_mode, interpolation, .. } = self {
            if let Some(c2) = color2.take() {
                if colors.is_empty() {
                    colors.push(c2);
                }
            }
            if let Some(t) = transition.take() {
                match t {
                    GradientTransition::Bounce => {
                        *loop_mode = LoopMode::Bounce;
                        *interpolation = InterpolationMode::Smooth;
                    }
                    GradientTransition::Loop | GradientTransition::SmoothLoop => {
                        *loop_mode = LoopMode::Cycle;
                        *interpolation = InterpolationMode::Smooth;
                    }
                }
            }
        }
        // Note: Dynamic legacy fields (_legacy_mode, etc.) are kept for serde
        // but the actual effect config is on CrosshairProject::dynamic_effects.
        // Migration from old single-mode to new multi-effect happens at the
        // project level in load_project().
    }
}

// ── Odd anchor ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum OddAnchor {
    #[default]
    TopRight,
    TopLeft,
    BottomLeft,
    BottomRight,
    Center,
}

impl OddAnchor {
    pub fn offset(self) -> (f64, f64) {
        match self {
            OddAnchor::TopRight => (0.0, -1.0),
            OddAnchor::TopLeft => (-1.0, -1.0),
            OddAnchor::BottomLeft => (-1.0, 0.0),
            OddAnchor::BottomRight => (0.0, 0.0),
            OddAnchor::Center => (-0.5, -0.5),
        }
    }
}

impl std::fmt::Display for OddAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OddAnchor::TopRight => write!(f, "TopRight"),
            OddAnchor::TopLeft => write!(f, "TopLeft"),
            OddAnchor::BottomLeft => write!(f, "BottomLeft"),
            OddAnchor::BottomRight => write!(f, "BottomRight"),
            OddAnchor::Center => write!(f, "Center"),
        }
    }
}

// ── Pieces ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Piece {
    Cross {
        origin: (i32, i32),
        #[serde(default)]
        left_gap: i32,
        #[serde(default)]
        right_gap: i32,
        #[serde(default)]
        top_gap: i32,
        #[serde(default)]
        bottom_gap: i32,
        #[serde(default)]
        left_thickness: i32,
        #[serde(default)]
        right_thickness: i32,
        #[serde(default)]
        top_thickness: i32,
        #[serde(default)]
        bottom_thickness: i32,
        left_length: i32,
        right_length: i32,
        top_length: i32,
        bottom_length: i32,
        color: String,
        #[serde(default)]
        color_type: ColorType,
        visible: bool,
        #[serde(default)]
        odd_anchor: OddAnchor,
        #[serde(default = "default_true")]
        lock_gap: bool,
        #[serde(default = "default_true")]
        lock_all: bool,
        /// Rotation in degrees around the cross center (45 turns + into x).
        #[serde(default)]
        rotation: f64,
        /// Draw a dot at the center, with the gap separating it from the arms.
        #[serde(default)]
        center_dot: bool,
        /// Render rotated arms with hard (non anti-aliased) pixel edges.
        #[serde(default)]
        hard_edges: bool,
    },
    Dot {
        origin: (i32, i32),
        size: u32,
        color: String,
        #[serde(default)]
        color_type: ColorType,
        visible: bool,
        #[serde(default)]
        odd_anchor: OddAnchor,
    },
    Line {
        origin: (i32, i32),
        vector: (i32, i32),
        thickness: i32,
        color: String,
        #[serde(default)]
        color_type: ColorType,
        visible: bool,
        #[serde(default)]
        odd_anchor: OddAnchor,
        #[serde(default)]
        anti_aliasing: bool,
    },
    Rectangle {
        origin: (i32, i32),
        width: u32,
        height: u32,
        rotation: f64,
        color: String,
        #[serde(default)]
        color_type: ColorType,
        visible: bool,
        #[serde(default)]
        odd_anchor: OddAnchor,
    },
    RectPattern {
        origin: (i32, i32),
        x_distance: i32,
        x_quantity: u32,
        y_distance: i32,
        y_quantity: u32,
        obj: Box<Self>,
        visible: bool,
    },
    CircPattern {
        origin: (i32, i32),
        radius: i32,
        quantity: u32,
        start_deg: f64,
        obj: Box<Self>,
        visible: bool,
    },
    HappyFace {
        origin: (i32, i32),
        size: u32,
        color: String,
        #[serde(default)]
        color_type: ColorType,
        visible: bool,
        #[serde(default)]
        odd_anchor: OddAnchor,
    },
}

impl Piece {
    pub fn type_name(&self) -> &'static str {
        match self {
            Piece::Cross { .. } => "Cross",
            Piece::Dot { .. } => "Dot",
            Piece::Line { .. } => "Line",
            Piece::Rectangle { .. } => "Rectangle",
            Piece::RectPattern { .. } => "RectPattern",
            Piece::CircPattern { .. } => "CircPattern",
            Piece::HappyFace { .. } => "HappyFace",
        }
    }

    pub fn is_visible(&self) -> bool {
        match self {
            Piece::Cross { visible, .. }
            | Piece::Dot { visible, .. }
            | Piece::Line { visible, .. }
            | Piece::Rectangle { visible, .. }
            | Piece::RectPattern { visible, .. }
            | Piece::CircPattern { visible, .. }
            | Piece::HappyFace { visible, .. } => *visible,
        }
    }

    pub fn set_visible(&mut self, val: bool) {
        match self {
            Piece::Cross { visible, .. }
            | Piece::Dot { visible, .. }
            | Piece::Line { visible, .. }
            | Piece::Rectangle { visible, .. }
            | Piece::RectPattern { visible, .. }
            | Piece::CircPattern { visible, .. }
            | Piece::HappyFace { visible, .. } => *visible = val,
        }
    }

    pub fn odd_anchor(&self) -> OddAnchor {
        match self {
            Piece::Cross { odd_anchor, .. }
            | Piece::Dot { odd_anchor, .. }
            | Piece::Line { odd_anchor, .. }
            | Piece::Rectangle { odd_anchor, .. }
            | Piece::HappyFace { odd_anchor, .. } => *odd_anchor,
            Piece::RectPattern { .. } | Piece::CircPattern { .. } => OddAnchor::default(),
        }
    }

    pub fn color_type(&self) -> ColorType {
        match self {
            Piece::Cross { color_type, .. }
            | Piece::Dot { color_type, .. }
            | Piece::Line { color_type, .. }
            | Piece::Rectangle { color_type, .. }
            | Piece::HappyFace { color_type, .. } => color_type.clone(),
            Piece::RectPattern { obj, .. } | Piece::CircPattern { obj, .. } => obj.color_type(),
        }
    }

    pub fn get_animated_color(&self, frame: f64) -> String {
        let ct = self.color_type();
        match ct {
            ColorType::Solid => self.base_color(),
            ColorType::Eraser => "#00000000".to_string(),
            // Dynamic pieces render as a semi-transparent checkerboard hint in the editor,
            // but for animated color purposes we just return a placeholder.
            ColorType::Dynamic { .. } => "#ff00ff80".to_string(),
            ColorType::Rainbow { saturation, lightness, alpha, speed, reverse } => {
                let dir = if reverse { -1.0 } else { 1.0 };
                let hue = (frame * speed * dir * 360.0).rem_euclid(360.0);
                let (r, g, b) = hsv_to_rgb(hue, saturation, lightness);
                let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            }
            ColorType::GradientCycle { colors, speed, loop_mode, interpolation, .. } => {
                if colors.len() < 2 {
                    return colors.first().cloned().unwrap_or_else(|| "#ffffffff".to_string());
                }
                let n = colors.len();
                let (seg, local_t) = match loop_mode {
                    LoopMode::Bounce => {
                        let segments = n - 1;
                        let total_len = segments as f64 * 2.0;
                        let t = (frame * speed) % total_len;
                        let t = if t > segments as f64 { total_len - t } else { t };
                        let seg = (t.floor() as usize).min(segments - 1);
                        (seg, t - seg as f64)
                    }
                    LoopMode::Cycle => {
                        let t = (frame * speed) % n as f64;
                        let seg = t.floor() as usize % n;
                        (seg, t - t.floor())
                    }
                };
                let next = match loop_mode {
                    LoopMode::Bounce => (seg + 1).min(n - 1),
                    LoopMode::Cycle => (seg + 1) % n,
                };
                let all_colors: Vec<&str> = colors.iter().map(|s| s.as_str()).collect();
                let c1 = parse_hex_color_rgba(all_colors[seg]);
                match interpolation {
                    InterpolationMode::Smooth => {
                        let c2 = parse_hex_color_rgba(all_colors[next]);
                        let r = ((c1.0 as f64 * (1.0 - local_t)) + (c2.0 as f64 * local_t)) as u8;
                        let g = ((c1.1 as f64 * (1.0 - local_t)) + (c2.1 as f64 * local_t)) as u8;
                        let b = ((c1.2 as f64 * (1.0 - local_t)) + (c2.2 as f64 * local_t)) as u8;
                        let a = ((c1.3 as f64 * (1.0 - local_t)) + (c2.3 as f64 * local_t)) as u8;
                        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
                    }
                    InterpolationMode::Instant => {
                        format!("#{:02x}{:02x}{:02x}{:02x}", c1.0, c1.1, c1.2, c1.3)
                    }
                }
            }
        }
    }

    pub fn base_color(&self) -> String {
        match self {
            Piece::Cross { color, .. }
            | Piece::Dot { color, .. }
            | Piece::Line { color, .. }
            | Piece::Rectangle { color, .. }
            | Piece::HappyFace { color, .. } => color.clone(),
            Piece::RectPattern { obj, .. } | Piece::CircPattern { obj, .. } => obj.base_color(),
        }
    }

    pub fn set_color_override(&mut self, color: &str) {
        match self {
            Piece::Cross { color: c, .. }
            | Piece::Dot { color: c, .. }
            | Piece::Line { color: c, .. }
            | Piece::Rectangle { color: c, .. }
            | Piece::HappyFace { color: c, .. } => *c = color.to_string(),
            Piece::RectPattern { obj, .. } | Piece::CircPattern { obj, .. } => obj.set_color_override(color),
        }
    }
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

pub fn animation_cycle_duration(piece: &Piece) -> Option<f64> {
    match piece.color_type() {
        ColorType::Rainbow { speed, .. } => Some(1.0 / speed),
        ColorType::GradientCycle { colors, speed, loop_mode, .. } => {
            let n = colors.len();
            if n < 2 { return None; }
            let cycle = match loop_mode {
                LoopMode::Bounce => (n - 1) as f64 * 2.0 / speed,
                LoopMode::Cycle => n as f64 / speed,
            };
            Some(cycle)
        }
        _ => None,
    }
}

pub fn max_animation_cycle(pieces: &[Piece]) -> f64 {
    pieces.iter()
        .filter_map(|p| animation_cycle_duration(p))
        .fold(1.0_f64, f64::max)
}

fn parse_hex_color_rgba(hex: &str) -> (u8, u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let a = if hex.len() >= 8 {
        u8::from_str_radix(&hex[6..8], 16).unwrap_or(255)
    } else {
        255
    };
    (r, g, b, a)
}

// ── Project ─────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct CrosshairProject {
    pub name: String,
    pub pieces: Vec<Piece>,
    /// Legacy field kept for backwards-compatible deserialization.
    #[serde(default)]
    pub odd_anchor: Option<OddAnchor>,
    /// Global dynamic effect chain.  When any piece is Dynamic, these
    /// effects are applied where the mask is white.
    #[serde(default)]
    pub dynamic_effects: DynamicEffects,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub recent_crosshairs: Vec<std::path::PathBuf>,
    #[serde(default)]
    pub current_crosshair: Option<std::path::PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recent_crosshairs: Vec::new(),
            current_crosshair: None,
        }
    }
}

pub fn default_pieces() -> Vec<Piece> {
    use Piece::{Cross, Dot, HappyFace, RectPattern};
    let default_color_type = ColorType::default();
    vec![
        RectPattern {
            origin: (0, -10),
            x_distance: 10,
            x_quantity: 1,
            y_distance: -10,
            y_quantity: 3,
            obj: Box::new(Dot {
                origin: (0, 0),
                size: 1,
                color: "#ff5050ff".to_string(),
                color_type: default_color_type.clone(),
                visible: true,
                odd_anchor: OddAnchor::default(),
            }),
            visible: true,
        },
        Cross {
            origin: (0, 0),
            left_gap: 2,
            right_gap: 2,
            top_gap: 2,
            bottom_gap: 2,
            left_thickness: 2,
            right_thickness: 2,
            top_thickness: 2,
            bottom_thickness: 2,
            left_length: 2,
            right_length: 2,
            top_length: 2,
            bottom_length: 2,
            color: "#00ff7dff".to_string(),
            color_type: default_color_type.clone(),
            visible: true,
            odd_anchor: OddAnchor::default(),
            lock_gap: true,
            lock_all: true,
            rotation: 0.0,
            center_dot: false,
            hard_edges: false,
        },
        HappyFace {
            origin: (-50, -10),
            size: 3,
            color: "#00ff7dff".to_string(),
            color_type: default_color_type,
            visible: true,
            odd_anchor: OddAnchor::default(),
        },
    ]
}

pub fn default_cross_piece() -> Piece {
    Piece::Cross {
        origin: (0, 0),
        left_gap: 2,
        right_gap: 2,
        top_gap: 2,
        bottom_gap: 2,
        left_thickness: 2,
        right_thickness: 2,
        top_thickness: 2,
        bottom_thickness: 2,
        left_length: 4,
        right_length: 4,
        top_length: 4,
        bottom_length: 4,
        color: "#00ff7dff".to_string(),
        color_type: ColorType::default(),
        visible: true,
        odd_anchor: OddAnchor::default(),
        lock_gap: true,
        lock_all: true,
        rotation: 0.0,
        center_dot: false,
            hard_edges: false,
    }
}

/// All legacy mode file tags (for cleanup of old per-mode mask files).
pub const ALL_LEGACY_MODE_TAGS: &[&str] = &[
    "invert", "dodge", "burn", "complement", "lumainvert", "huerotate", "saturate",
];
