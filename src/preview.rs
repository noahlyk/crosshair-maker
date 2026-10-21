use eframe::egui;
use std::fs::File;
use std::io::BufWriter;

use crate::bg_embedded::BACKGROUND_NAMES;
use crate::svg_rendering::{generate_svg, generate_svg_scaled, infer_bounds, infer_extent};
use crate::types::Piece;

#[derive(Clone, PartialEq)]
pub enum PreviewBackground {
    None,
    Custom(String),
}

impl PreviewBackground {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::Custom(name) => name.clone(),
        }
    }
}

pub struct PreviewState {
    preview_image: Option<egui::ColorImage>,
    preview_texture: Option<egui::TextureHandle>,
    needs_preview_update: bool,
    max_preview_size: u32,
    override_max: bool,
    /// Animation start time (monotonic).
    animation_start: std::time::Instant,
    /// Selected background for live preview.
    pub selected_bg: PreviewBackground,
    /// Decoded background image (cached).
    bg_image: Option<(PreviewBackground, std::rc::Rc<image::RgbaImage>)>,
    /// Composite texture (background + crosshair + effects).
    composite_texture: Option<egui::TextureHandle>,
    /// Hash of everything the composite depends on; unchanged = reuse the texture.
    composite_key: u64,
    /// Rasterized crosshair + dynamic masks for the current pieces and scale.
    raster_cache: Option<std::rc::Rc<RasterCache>>,
    /// Zoom level (1.0 = default).
    zoom_level: f32,
    /// Crosshair scale as (numerator, denominator) for clean integer scaling.
    /// e.g. (2,1) = x2, (1,2) = x1/2. Avoids fractional anti-aliasing artifacts.
    pub crosshair_scale: (u32, u32),
    /// When true, dynamic effects sample the background at native (unscaled)
    /// resolution so the preview honestly represents the real pixel count.
    pub honest_pixels: bool,
    /// Background pan offset (pixels in composite space).
    bg_pan: egui::Vec2,
}

/// Rasterized layers of the live preview.  Rendering the (scaled) SVGs is the
/// expensive part, so they are redone only when the pieces, scale or size change.
struct RasterCache {
    key: u64,
    crosshair_rgba: Option<Vec<u8>>,
    masks: Vec<(crate::types::DynamicEffects, Vec<u8>)>,
}

/// Pooled colors for a rectangle of the background.
struct PooledRegion {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// Packed 0..1 RGB, row-major over the rectangle.
    data: Vec<f32>,
}

impl PooledRegion {
    fn get(&self, x: i32, y: i32) -> Option<(f32, f32, f32)> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        let i = ((y - self.y0) as usize * self.w as usize + (x - self.x0) as usize) * 3;
        Some((self.data[i], self.data[i + 1], self.data[i + 2]))
    }
}

/// Pool the NxN neighbourhood of every pixel in `[x0,x1) x [y0,y1)` of `bg`
/// (edges clamped to the image).  Only that rectangle is computed: a box
/// filter is separable, so it is two 1-D passes — O(N) per pixel instead of
/// O(N²) — over the rectangle plus an N/2 margin.  Big regions are split
/// across threads.
fn pool_region(bg: &image::RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, n: i32, mode: crate::types::PoolMode) -> PooledRegion {
    use crate::types::PoolMode;
    let (bw, bh) = (bg.width() as i32, bg.height() as i32);
    let (x0, y0) = (x0.clamp(0, bw), y0.clamp(0, bh));
    let (x1, y1) = (x1.clamp(x0, bw), y1.clamp(y0, bh));
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let empty = PooledRegion { x0, y0, w: w as i32, h: h as i32, data: Vec::new() };
    if w == 0 || h == 0 {
        return empty;
    }
    let r = (n / 2).max(0);
    let raw = bg.as_raw();
    let px = |x: i32, y: i32, c: usize| -> f32 {
        raw[((y.clamp(0, bh - 1) as usize * bw as usize) + x.clamp(0, bw - 1) as usize) * 4 + c] as f32 / 255.0
    };

    let combine = move |acc: f32, v: f32| match mode {
        PoolMode::Average => acc + v,
        PoolMode::Max => acc.max(v),
        PoolMode::Min => acc.min(v),
    };
    let init = match mode {
        PoolMode::Average | PoolMode::Max => 0.0f32,
        PoolMode::Min => 1.0f32,
    };
    let norm = if mode == PoolMode::Average { 1.0 / n as f32 } else { 1.0 };

    // horizontal pass over the rows the vertical pass will need
    let ty0 = (y0 - r).max(0);
    let ty1 = (y1 + r).min(bh);
    let th = (ty1 - ty0) as usize;
    let mut tmp = vec![0.0f32; th * w * 3];

    let threads = if w * th > 40_000 {
        std::thread::available_parallelism().map_or(4, |t| t.get()).min(16)
    } else {
        1
    };
    let rows_per = th.div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (ci, chunk) in tmp.chunks_mut(rows_per * w * 3).enumerate() {
            let px = &px;
            sc.spawn(move || {
                for (ry, row) in chunk.chunks_mut(w * 3).enumerate() {
                    let y = ty0 + (ci * rows_per + ry) as i32;
                    for x in 0..w {
                        for c in 0..3 {
                            let mut acc = init;
                            for dx in -r..=r {
                                acc = combine(acc, px(x0 + x as i32 + dx, y, c));
                            }
                            row[x * 3 + c] = acc * norm;
                        }
                    }
                }
            });
        }
    });

    // vertical pass
    let mut out = vec![0.0f32; w * h * 3];
    let rows_per = h.div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (ci, chunk) in out.chunks_mut(rows_per * w * 3).enumerate() {
            let tmp = &tmp;
            sc.spawn(move || {
                for (ry, row) in chunk.chunks_mut(w * 3).enumerate() {
                    let y = y0 + (ci * rows_per + ry) as i32;
                    for x in 0..w {
                        for c in 0..3 {
                            let mut acc = init;
                            for dy in -r..=r {
                                let yy = (y + dy).clamp(0, bh - 1) - ty0;
                                acc = combine(acc, tmp[(yy as usize * w + x) * 3 + c]);
                            }
                            row[x * 3 + c] = acc * norm;
                        }
                    }
                }
            });
        }
    });
    PooledRegion { x0, y0, w: w as i32, h: h as i32, data: out }
}

fn hash_of(parts: &[&dyn std::fmt::Debug]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        format!("{p:?}").hash(&mut h);
    }
    h.finish()
}

impl PreviewState {
    pub fn new() -> Self {
        Self {
            preview_image: None,
            preview_texture: None,
            needs_preview_update: true,
            max_preview_size: 1920,
            override_max: false,
            animation_start: std::time::Instant::now(),
            selected_bg: PreviewBackground::None,
            bg_image: None,
            composite_texture: None,
            composite_key: 0,
            raster_cache: None,
            zoom_level: 1.0,
            crosshair_scale: (1, 1),
            honest_pixels: true,
            bg_pan: egui::Vec2::ZERO,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.needs_preview_update = true;
    }

    /// Current animation frame value (0.0..inf, wraps via modular arithmetic in color functions).
    pub fn animation_frame(&self) -> f64 {
        self.animation_start.elapsed().as_secs_f64()
    }

    pub fn generate_svg(&self, pieces: &[Piece]) -> String {
        let (extent_x, extent_y) = infer_extent(pieces);
        let (extent_x, extent_y) = if self.override_max {
            (extent_x.cast_unsigned(), extent_y.cast_unsigned())
        } else {
            let max = self.max_preview_size;
            (
                extent_x.cast_unsigned().min(max),
                extent_y.cast_unsigned().min(max),
            )
        };
        let (sn, sd) = self.crosshair_scale;
        if sn != 1 || sd != 1 {
            generate_svg_scaled(extent_x, extent_y, pieces, sn, sd)
        } else {
            generate_svg(extent_x, extent_y, pieces)
        }
    }

    pub fn generate_svg_full(&self, pieces: &[Piece]) -> String {
        let (extent_x, extent_y) = infer_extent(pieces);
        let (sn, sd) = self.crosshair_scale;
        if sn != 1 || sd != 1 {
            generate_svg_scaled(extent_x.cast_unsigned(), extent_y.cast_unsigned(), pieces, sn, sd)
        } else {
            generate_svg(extent_x.cast_unsigned(), extent_y.cast_unsigned(), pieces)
        }
    }

    pub fn update(&mut self, ctx: &egui::Context, pieces: &[Piece]) {
        let has_animation = pieces.iter().any(|p| has_animated_color(p));
        let has_overrides = pieces.iter().any(|p| needs_color_override(p));

        if !self.needs_preview_update && !has_animation {
            return;
        }
        if egui::DragAndDrop::has_any_payload(ctx) {
            return;
        }
        self.needs_preview_update = false;

        // Apply color overrides for non-Solid types (Eraser, Rainbow, Gradient)
        let effective_pieces: Vec<Piece>;
        let pieces_ref = if has_overrides {
            if has_animation {
                ctx.request_repaint();
            }
            let frame = self.animation_frame();
            effective_pieces = apply_color_override(pieces, frame);
            &effective_pieces
        } else {
            pieces
        };

        let svg = self.generate_svg(pieces_ref);
        let tree = match resvg::usvg::Tree::from_str(
            &svg,
            &resvg::usvg::Options::default(),
        ) {
            Ok(t) => t,
            Err(_) => return,
        };

        let size = tree.size();
        let w = size.width() as u32;
        let h = size.height() as u32;

        if w == 0 || h == 0 {
            return;
        }

        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut pixmap.as_mut(),
        );

        self.preview_image = Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &pixmap.data(),
        ));
    }

    pub fn texture(&mut self, ctx: &egui::Context, pieces: &[Piece]) -> Option<egui::TextureHandle> {
        self.update(ctx, pieces);

        if let Some(image) = &self.preview_image {
            let needs_new = match &self.preview_texture {
                None => true,
                Some(tex) => tex.size() != image.size,
            };
            if needs_new {
                self.preview_texture = Some(ctx.load_texture(
                    "crosshair_preview",
                    image.clone(),
                    egui::TextureOptions::NEAREST,
                ));
            } else if let Some(tex) = &mut self.preview_texture {
                tex.set_partial([0, 0], image.clone(), egui::TextureOptions::NEAREST);
            }
        }

        self.preview_texture.clone()
    }

    pub fn max_preview_size(&self) -> u32 {
        self.max_preview_size
    }

    pub fn override_max(&self) -> bool {
        self.override_max
    }

    pub fn set_override_max(&mut self, val: bool) {
        self.override_max = val;
        self.mark_dirty();
    }

    /// Format crosshair scale for display (e.g. "x2", "x1/3").
    pub fn scale_label(&self) -> String {
        let (n, d) = self.crosshair_scale;
        if d == 1 {
            format!("x{n}")
        } else {
            format!("x{n}/{d}")
        }
    }

    /// Step the crosshair scale up (larger).
    pub fn scale_up(&mut self) {
        let (n, d) = self.crosshair_scale;
        self.crosshair_scale = if d > 1 {
            // x1/3 -> x1/2, x1/2 -> x1
            (n, d - 1)
        } else {
            // x1 -> x2, x2 -> x3, etc.
            (n + 1, 1)
        };
        self.mark_dirty();
    }

    /// Step the crosshair scale down (smaller).
    pub fn scale_down(&mut self) {
        let (n, d) = self.crosshair_scale;
        self.crosshair_scale = if n > 1 {
            // x3 -> x2, x2 -> x1
            (n - 1, d)
        } else {
            // x1 -> x1/2, x1/2 -> x1/3, cap at x1/8
            if d < 8 {
                (1, d + 1)
            } else {
                (1, 8)
            }
        };
        self.mark_dirty();
    }

    /// Get or load the selected background image.
    fn ensure_bg_image(&mut self) -> Option<std::rc::Rc<image::RgbaImage>> {
        if self.selected_bg == PreviewBackground::None {
            self.bg_image = None;
            return None;
        }
        // Check if cached bg matches selection
        if let Some((ref cached_bg, _)) = self.bg_image {
            if *cached_bg == self.selected_bg {
                return self.bg_image.as_ref().map(|(_, img)| img.clone());
            }
        }
        // Load the background from embedded data
        let name = self.selected_bg.label();
        if let Some(data) = crate::bg_embedded::get_background(&name) {
            if let Ok(img) = image::load_from_memory(data) {
                self.bg_image = Some((self.selected_bg.clone(), std::rc::Rc::new(img.to_rgba8())));
                return self.bg_image.as_ref().map(|(_, img)| img.clone());
            }
        }
        self.bg_image = None;
        None
    }
}

// ── thumbnail helpers ───────────────────────────────────────────────

/// Zero out origins so the piece renders centered for thumbnails.
fn zero_origin(piece: &mut Piece) {
    match piece {
        Piece::Cross { origin, .. }
        | Piece::Dot { origin, .. }
        | Piece::Line { origin, .. }
        | Piece::Rectangle { origin, .. }
        | Piece::HappyFace { origin, .. } => *origin = (0, 0),
        Piece::RectPattern { origin, obj, .. } | Piece::CircPattern { origin, obj, .. } => {
            *origin = (0, 0);
            zero_origin(obj);
        }
    }
}

/// Render a single piece, tightly cropped to its content.
/// Origins are zeroed so the piece renders centered, then the output
/// is cropped to the minimum bounding box (no wasted transparent space).
fn render_piece_image(piece: &Piece, frame: f64) -> Option<egui::ColorImage> {
    if !piece.is_visible() {
        return None;
    }
    let mut centered = piece.clone();
    // Apply color override for non-Solid types
    if needs_color_override(&centered) {
        let color = centered.get_animated_color(frame);
        centered.set_color_override(&color);
    }
    zero_origin(&mut centered);
    let pieces = [centered];

    // Get tight content bounds (relative to SVG center)
    let (bmin_x, bmin_y, bmax_x, bmax_y) = infer_bounds(&pieces);
    if bmax_x <= bmin_x || bmax_y <= bmin_y {
        return None;
    }

    // Generate SVG at symmetric extent (guaranteed to contain everything)
    let (ex, ey) = infer_extent(&pieces);
    if ex <= 0 || ey <= 0 {
        return None;
    }
    let svg = generate_svg(ex.cast_unsigned(), ey.cast_unsigned(), &pieces);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;

    // SVG center in pixel coords
    let cx = ex as f64;
    let cy = ey as f64;

    // Content region in SVG pixel coords (with 1px padding)
    let left = (cx + bmin_x).floor() as i32 - 1;
    let top = (cy + bmin_y).floor() as i32 - 1;
    let right = (cx + bmax_x).ceil() as i32 + 1;
    let bottom = (cy + bmax_y).ceil() as i32 + 1;

    let crop_w = (right - left).max(1) as u32;
    let crop_h = (bottom - top).max(1) as u32;

    // Render with translation so the content region starts at (0,0)
    let mut pixmap = resvg::tiny_skia::Pixmap::new(crop_w, crop_h)?;
    let transform = resvg::tiny_skia::Transform::from_translate(
        -(left as f32),
        -(top as f32),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    Some(egui::ColorImage::from_rgba_unmultiplied(
        [crop_w as usize, crop_h as usize],
        pixmap.data(),
    ))
}

// ── piece thumbnail cache ───────────────────────────────────────────

/// Caches per-piece thumbnails, only re-rendering when a piece changes.
pub struct PieceThumbnailCache {
    entries: Vec<Option<(String, egui::TextureHandle)>>,
}

impl PieceThumbnailCache {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Compare each piece's serialized form to the cache; only re-render
    /// the ones that actually changed (or animated pieces every frame).
    pub fn update(&mut self, ctx: &egui::Context, pieces: &[Piece], frame: f64) {
        // Grow / shrink to match piece count
        self.entries.resize_with(pieces.len(), || None);
        self.entries.truncate(pieces.len());

        for (i, piece) in pieces.iter().enumerate() {
            let animated = needs_color_override(piece);
            let key = serde_json::to_string(piece).unwrap_or_default();
            let needs_update = animated || match &self.entries[i] {
                Some((cached_key, _)) => *cached_key != key,
                None => true,
            };
            if needs_update {
                self.entries[i] = render_piece_image(piece, frame).map(|img| {
                    let tex = ctx.load_texture(
                        format!("piece_thumb_{i}"),
                        img,
                        egui::TextureOptions::NEAREST,
                    );
                    (key, tex)
                });
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<&egui::TextureHandle> {
        self.entries.get(index)?.as_ref().map(|(_, tex)| tex)
    }
}

// ── recent crosshair thumbnail cache ────────────────────────────────

/// Render the full crosshair (all pieces composited) as a small thumbnail.
fn render_crosshair_thumbnail(pieces: &[Piece], frame: f64) -> Option<egui::ColorImage> {
    if pieces.is_empty() {
        return None;
    }
    let effective = apply_color_override(pieces, frame);
    let (ex, ey) = infer_extent(&effective);
    if ex <= 0 || ey <= 0 {
        return None;
    }
    let svg = generate_svg(ex.cast_unsigned(), ey.cast_unsigned(), &effective);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let w = size.width() as u32;
    let h = size.height() as u32;
    if w == 0 || h == 0 {
        return None;
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

/// Caches recent crosshair thumbnails, loading pieces from project JSON
/// and re-rendering animated ones each frame.
pub struct RecentThumbnailCache {
    entries: std::collections::HashMap<std::path::PathBuf, RecentEntry>,
}

struct RecentEntry {
    pieces: Vec<Piece>,
    has_animation: bool,
    texture: Option<egui::TextureHandle>,
    key: String,
}

impl RecentThumbnailCache {
    pub fn new() -> Self {
        Self { entries: std::collections::HashMap::new() }
    }

    pub fn update(&mut self, ctx: &egui::Context, paths: &[std::path::PathBuf], frame: f64) {
        // Remove entries for paths no longer in the list
        self.entries.retain(|k, _| paths.contains(k));

        for path in paths {
            let entry = self.entries.entry(path.clone()).or_insert_with(|| {
                let pieces = crate::project_io::load_project(path)
                    .map(|p| p.pieces)
                    .unwrap_or_default();
                let has_animation = pieces.iter().any(|p| has_animated_color(p));
                let key = serde_json::to_string(&pieces).unwrap_or_default();
                RecentEntry { pieces, has_animation, texture: None, key }
            });

            let needs_update = entry.has_animation || entry.texture.is_none();
            if needs_update {
                if let Some(img) = render_crosshair_thumbnail(&entry.pieces, frame) {
                    entry.texture = Some(ctx.load_texture(
                        format!("recent_thumb_{}", path.display()),
                        img,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
        }
    }

    /// Override the cached pieces for a path with live data (e.g. the currently-open project).
    /// This ensures the thumbnail reflects unsaved edits.
    pub fn set_live_pieces(&mut self, path: &std::path::Path, pieces: &[Piece]) {
        let has_animation = pieces.iter().any(|p| has_animated_color(p));
        let key = serde_json::to_string(pieces).unwrap_or_default();
        let entry = self.entries.entry(path.to_path_buf()).or_insert_with(|| {
            RecentEntry { pieces: Vec::new(), has_animation: false, texture: None, key: String::new() }
        });
        if entry.key != key {
            entry.pieces = pieces.to_vec();
            entry.has_animation = has_animation;
            entry.key = key;
            entry.texture = None; // Force re-render
        }
    }

    pub fn get(&self, path: &std::path::Path) -> Option<&egui::TextureHandle> {
        self.entries.get(path)?.texture.as_ref()
    }

    pub fn invalidate(&mut self, path: &std::path::PathBuf) {
        self.entries.remove(path);
    }
}

/// Save SVG and PNG/APNG exports alongside a project JSON path.
pub fn save_exports(json_path: &std::path::Path, pieces: &[Piece], groups: &[crate::types::DynamicEffects]) {
    let (extent_x, extent_y) = infer_extent(pieces);
    if extent_x <= 0 || extent_y <= 0 {
        return;
    }

    // Check if any piece has animated colors
    let has_animation = pieces.iter().any(|p| {
        matches!(
            p.color_type(),
            crate::types::ColorType::Rainbow { .. } | crate::types::ColorType::GradientCycle { .. }
        )
    });

    if has_animation {
        save_animated_export(json_path, pieces, extent_x, extent_y);
    } else {
        save_static_export(json_path, pieces, extent_x, extent_y);
    }

    // Export dynamic mask + config
    save_dynamic_export(json_path, pieces, groups, extent_x, extent_y);
}

fn is_dynamic(piece: &Piece) -> bool {
    matches!(piece.color_type(), crate::types::ColorType::Dynamic { .. })
}

/// Pieces for rendering one dynamic group's mask: that group's pieces are
/// white, every other piece (static, erasers, other groups) is opaque black, so
/// anything layered above a piece cuts it out of the mask — and layered below
/// it is erased.  Read the result's red channel (white = apply this group).
fn dynamic_mask_pieces(pieces: &[Piece], group: u32) -> Vec<Piece> {
    pieces
        .iter()
        .map(|p| {
            let mut mp = p.clone();
            if p.dynamic_group() == Some(group) {
                mp.set_color_override("#ffffffff");
            } else {
                mp.set_color_override("#000000ff");
            }
            mp
        })
        .collect()
}

/// Dynamic groups that are in use and have an effect enabled, in group order.
/// These are the groups that get a mask and a cfg block.
fn active_groups<'a>(
    pieces: &[Piece],
    groups: &'a [crate::types::DynamicEffects],
) -> Vec<(u32, &'a crate::types::DynamicEffects)> {
    let mut ids: Vec<u32> = pieces.iter().filter_map(|p| p.dynamic_group()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| groups.get(id as usize).map(|e| (id, e)))
        .filter(|(_, e)| e.has_any_enabled())
        .collect()
}

/// File name of a group's mask: group 0 keeps the original
/// `{stem}.dynamic.png`, the rest are `{stem}.dynamic.{id}.png`.
fn dynamic_mask_name(stem: &str, id: u32) -> String {
    if id == 0 {
        format!("{stem}.dynamic.png")
    } else {
        format!("{stem}.dynamic.{id}.png")
    }
}

/// Save the dynamic masks + config.
///
/// Exports, for every dynamic group in use that has an effect enabled:
/// - `{stem}.dynamic.png` / `{stem}.dynamic.{id}.png` — grayscale mask of the
///   group (white where its pieces are visible after layering)
/// - `{stem}.dynamic.cfg` — one `group {id}` block of effect lines per group
///
/// Masks are written before the cfg (which lists the groups), and stale masks
/// of groups that no longer exist are removed.
fn save_dynamic_export(
    json_path: &std::path::Path,
    pieces: &[Piece],
    groups: &[crate::types::DynamicEffects],
    extent_x: i32,
    extent_y: i32,
) {
    let stem = json_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let cfg_path = json_path.with_file_name(format!("{stem}.dynamic.cfg"));
    let active = active_groups(pieces, groups);

    let w = extent_x as u32 * 2;
    let h = extent_y as u32 * 2;

    let mut kept: Vec<String> = Vec::new();
    let mut cfg = String::new();
    for (id, effects) in &active {
        let mask_pieces = dynamic_mask_pieces(pieces, *id);
        let svg = generate_svg(extent_x.cast_unsigned(), extent_y.cast_unsigned(), &mask_pieces);
        let Some(rgba_data) = rasterize_svg(&svg, w, h) else { continue };
        // the red channel is the mask value
        let gray: Vec<u8> = rgba_data.chunks(4).map(|px| px[0]).collect();
        let name = dynamic_mask_name(&stem, *id);
        let tmp = json_path.with_file_name(format!("{stem}.dynamic.tmp.png"));
        if image::save_buffer(&tmp, &gray, w, h, image::ColorType::L8).is_ok()
            && std::fs::rename(&tmp, json_path.with_file_name(&name)).is_ok()
        {
            cfg.push_str(&format!("group {id}\n"));
            cfg.push_str(&effects.to_cfg_string());
            kept.push(name);
        }
    }

    // Drop masks of groups that are gone (any `{stem}.dynamic*.png`).
    if let Some(dir) = json_path.parent() {
        let dir = if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir };
        let prefix = format!("{stem}.dynamic");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) && name.ends_with(".png") && !kept.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    if kept.is_empty() {
        let _ = std::fs::remove_file(&cfg_path);
    } else {
        let cfg_tmp = json_path.with_file_name(format!("{stem}.dynamic.cfg.tmp"));
        if std::fs::write(&cfg_tmp, &cfg).is_ok() {
            let _ = std::fs::rename(&cfg_tmp, &cfg_path);
        }
    }

    // Clean up legacy per-mode mask files
    for tag in crate::types::ALL_LEGACY_MODE_TAGS {
        let _ = std::fs::remove_file(json_path.with_file_name(format!("{stem}.mask.{tag}.png")));
    }
    let _ = std::fs::remove_file(json_path.with_file_name(format!("{stem}.mask.png")));
    let _ = std::fs::remove_file(json_path.with_file_name(format!("{stem}.mask.apng")));
}

/// Helper: rasterize an SVG string to RGBA pixel data.
fn rasterize_svg(svg: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Some(pixmap.data().to_vec())
}

fn has_animated_color(piece: &Piece) -> bool {
    matches!(
        piece.color_type(),
        crate::types::ColorType::Rainbow { .. } | crate::types::ColorType::GradientCycle { .. }
    )
}

fn needs_color_override(piece: &Piece) -> bool {
    !matches!(piece.color_type(), crate::types::ColorType::Solid)
}

fn apply_color_override(pieces: &[Piece], frame: f64) -> Vec<Piece> {
    apply_color_override_inner(pieces, frame, false)
}

/// Like `apply_color_override` but for export: ContrastInvert pieces
/// are rendered as transparent so they don't appear in the main image.
fn apply_color_override_for_export(pieces: &[Piece], frame: f64) -> Vec<Piece> {
    apply_color_override_inner(pieces, frame, true)
}

fn apply_color_override_inner(pieces: &[Piece], frame: f64, export_mode: bool) -> Vec<Piece> {
    pieces
        .iter()
        .map(|p| {
            if export_mode && is_dynamic(p) {
                // In export mode, Dynamic → transparent (handled by mask)
                let mut new_piece = p.clone();
                new_piece.set_color_override("#00000000");
                new_piece
            } else if needs_color_override(p) {
                let mut new_piece = p.clone();
                let animated_color = p.get_animated_color(frame);
                new_piece.set_color_override(&animated_color);
                new_piece
            } else {
                p.clone()
            }
        })
        .collect()
}

fn save_static_export(json_path: &std::path::Path, pieces: &[Piece], extent_x: i32, extent_y: i32) {
    // Apply non-Solid overrides (e.g. Eraser → transparent, ContrastInvert → transparent)
    let effective = apply_color_override_for_export(pieces, 0.0);
    let svg = generate_svg(extent_x.cast_unsigned(), extent_y.cast_unsigned(), &effective);
    let _ = std::fs::write(json_path.with_extension("svg"), &svg);

    if let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()) {
        let size = tree.size();
        let w = size.width() as u32;
        let h = size.height() as u32;
        if w > 0 && h > 0 {
            if let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(w, h) {
                resvg::render(
                    &tree,
                    resvg::tiny_skia::Transform::default(),
                    &mut pixmap.as_mut(),
                );
                // Write to a temp file then rename so krosshair never reads a partial PNG.
                let tmp = json_path.with_extension("tmp.png");
                if image::save_buffer(&tmp, pixmap.data(), w, h, image::ColorType::Rgba8).is_ok() {
                    let _ = std::fs::rename(&tmp, json_path.with_extension("png"));
                }
            }
        }
    }
    // Delete the apng (and legacy gif) if present: krosshair prefers
    // apng > gif > png, so a stale one would shadow this fresh png.
    let _ = std::fs::remove_file(json_path.with_extension("apng"));
    let _ = std::fs::remove_file(json_path.with_extension("gif"));
}

/// Target FPS for APNG export. Frame values use real-time seconds
/// so the exported animation matches the live preview speed.
const EXPORT_FPS: u32 = 30;
/// Maximum frames per APNG to keep file sizes reasonable.
const EXPORT_MAX_FRAMES: u32 = 300;

fn save_animated_export(json_path: &std::path::Path, pieces: &[Piece], extent_x: i32, extent_y: i32) {
    let w = extent_x as u32 * 2;
    let h = extent_y as u32 * 2;

    // Compute cycle duration so the APNG covers exactly one full loop
    let cycle_duration = crate::types::max_animation_cycle(pieces);
    let num_frames: u32 = ((cycle_duration * EXPORT_FPS as f64).round() as u32)
        .max(2)
        .min(EXPORT_MAX_FRAMES);
    let frame_delay_secs = cycle_duration / num_frames as f64;

    // APNG frame delay as integer ratio: delay_num / delay_den seconds.
    // Use millisecond precision: delay_num = round(delay_ms), delay_den = 1000.
    let delay_den: u16 = 1000;
    let delay_num: u16 = ((frame_delay_secs * 1000.0).round() as u16).max(1);

    // Save SVG (static)
    let svg = generate_svg(extent_x.cast_unsigned(), extent_y.cast_unsigned(), pieces);
    let _ = std::fs::write(json_path.with_extension("svg"), &svg);

    // Generate frames with real-time second values
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for i in 0..num_frames {
        let frame_time = i as f64 * frame_delay_secs;
        let colored_pieces = apply_color_override_for_export(pieces, frame_time);
        let svg = generate_svg(extent_x.cast_unsigned(), extent_y.cast_unsigned(), &colored_pieces);

        if let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()) {
            if let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(w, h) {
                resvg::render(
                    &tree,
                    resvg::tiny_skia::Transform::default(),
                    &mut pixmap.as_mut(),
                );
                frames.push(pixmap.data().to_vec());
            }
        }
    }

    // Save as APNG
    if !frames.is_empty() {
        let actual_frames = frames.len() as u32;
        // Write to a temp file, renamed into place below.
        let apng_path = json_path.with_extension("tmp.apng");
        if let Ok(file) = File::create(&apng_path) {
            let mut buf_writer = BufWriter::new(file);
            let mut encoder = png::Encoder::new(&mut buf_writer, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(actual_frames, 0).unwrap();
            encoder.set_frame_delay(delay_num, delay_den).unwrap();

            if let Ok(mut png_writer) = encoder.write_header() {
                for frame_data in &frames {
                    let _ = png_writer.write_image_data(frame_data);
                }
            };
        }
    }

    let _ = std::fs::rename(json_path.with_extension("tmp.apng"), json_path.with_extension("apng"));

    // Delete the png (and legacy gif) if present: they are mutually exclusive
    // with the apng and a stale gif would shadow it.
    let _ = std::fs::remove_file(json_path.with_extension("png"));
    let _ = std::fs::remove_file(json_path.with_extension("gif"));
}

// ── preview panel ───────────────────────────────────────────────────

pub fn render_preview_panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    preview: &mut PreviewState,
    pieces: &[Piece],
    groups: &[crate::types::DynamicEffects],
) {
    let (extent_x, extent_y) = infer_extent(pieces);
    let width = extent_x.cast_unsigned() * 2;
    let height = extent_y.cast_unsigned() * 2;

    // Effective size accounts for crosshair scale
    let (sn, sd) = preview.crosshair_scale;
    let effective_w = width * sn / sd;
    let effective_h = height * sn / sd;

    let max_size = preview.max_preview_size() * 2;
    let exceeds_max = effective_w > max_size || effective_h > max_size;
    // Auto-disengage override when the size drops back below the threshold
    if preview.override_max() && !exceeds_max {
        preview.set_override_max(false);
    }
    let is_capped = !preview.override_max() && exceeds_max;

    // Top controls row
    ui.horizontal(|ui| {
        if sn != 1 || sd != 1 {
            ui.label(format!("Size: {width}x{height} (scaled: {effective_w}x{effective_h})"));
        } else {
            ui.label(format!("Size: {width}x{height}"));
        }
        if is_capped {
            let mut ov = preview.override_max();
            let cb = ui.checkbox(&mut ov, egui::RichText::new("Override max size?").color(egui::Color32::RED));
            if cb.changed() {
                preview.set_override_max(ov);
            }
        }
        ui.separator();
        ui.label("Background:");
        let bg_names = BACKGROUND_NAMES;
        let bg_label = preview.selected_bg.label();
        egui::ComboBox::from_id_salt("preview_bg")
            .selected_text(bg_label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(preview.selected_bg == PreviewBackground::None, "None").clicked() {
                    preview.selected_bg = PreviewBackground::None;
                    preview.bg_pan = egui::Vec2::ZERO;
                    preview.mark_dirty();
                }
                for name in bg_names {
                    let is_selected = matches!(&preview.selected_bg, PreviewBackground::Custom(n) if n.as_str() == *name);
                    if ui.selectable_label(is_selected, *name).clicked() {
                        preview.selected_bg = PreviewBackground::Custom(name.to_string());
                        preview.bg_pan = egui::Vec2::ZERO;
                        preview.mark_dirty();
                    }
                }
            });
    });

    let has_bg = preview.selected_bg != PreviewBackground::None;
    let active = active_groups(pieces, groups);

    // Handle scroll-wheel: Ctrl+Scroll = crosshair scale (only with background),
    // plain Scroll = zoom.
    // We must use input_mut to consume Ctrl+Scroll events before egui interprets
    // them as a pinch-to-zoom gesture.
    let ctrl_held = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    if ctrl_held && has_bg {
        // Consume ALL scroll events when Ctrl is held so egui doesn't zoom
        let raw_scroll = ui.input_mut(|i| {
            let delta = i.raw_scroll_delta.y;
            if delta != 0.0 {
                // Zero out both scroll sources to prevent egui zoom
                i.raw_scroll_delta.y = 0.0;
                i.smooth_scroll_delta.y = 0.0;
            }
            delta
        });
        if raw_scroll > 0.0 {
            preview.scale_up();
        } else if raw_scroll < 0.0 {
            preview.scale_down();
        }
    } else {
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 {
            let factor = 1.0 + scroll_delta * 0.005;
            preview.zoom_level = (preview.zoom_level * factor).max(0.01);
        }
    }

    // Reset crosshair scale to x1 when background is None
    if !has_bg && preview.crosshair_scale != (1, 1) {
        preview.crosshair_scale = (1, 1);
        preview.mark_dirty();
    }

    // Show zoom level + crosshair scale (scale only when background is active)
    ui.horizontal(|ui| {
        ui.label(format!("Zoom: {:.0}%", preview.zoom_level * 100.0));
        if ui.small_button("Reset").clicked() {
            preview.zoom_level = 1.0;
        }
        if has_bg {
            ui.separator();
            ui.label(format!("Crosshair Scale: {}", preview.scale_label()));
            if ui.small_button("Reset").clicked() {
                preview.crosshair_scale = (1, 1);
                preview.mark_dirty();
            }
            let (sn, sd) = preview.crosshair_scale;
            if sn > sd {
                ui.checkbox(&mut preview.honest_pixels, "Honest Pixels");
            }
        }
    });

    let available_size = ui.available_size();
    let zoom = preview.zoom_level;

    if has_bg {
        if is_capped {
            // Size too large — show warning instead of compositing
            let (rect, _) = ui.allocate_exact_size(available_size, egui::Sense::hover());
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(20));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Preview disabled — scaled size {}x{} exceeds max.\nEnable 'Override max size' to render.", effective_w, effective_h),
                egui::FontId::default(),
                egui::Color32::from_rgb(255, 100, 100),
            );
        } else {
        // Live compositing mode: background + crosshair + dynamic effects
        ctx.request_repaint(); // Always repaint for mouse tracking

        // Get the crosshair texture for overlay
        let _crosshair_tex = preview.texture(ctx, pieces);

        // Allocate the preview rect (click+drag or middle-drag to pan)
        let (rect, response) = ui.allocate_exact_size(available_size, egui::Sense::click_and_drag());
        let mouse_in_rect = response.hovered();
        let mouse_pos = response.hover_pos();

        // Pan background with left-click-drag or middle-click-drag
        if response.dragged_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            preview.bg_pan += response.drag_delta();
        }

        // Hide cursor when hovering (but not while dragging)
        if mouse_in_rect && !response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }

        // Get background image dimensions
        let bg_img = preview.ensure_bg_image();
        if let Some(bg) = bg_img {
            let bg_w = bg.width();
            let bg_h = bg.height();

            // Compute composite: background centered and zoomed, crosshair at mouse or center
            let composite_w = rect.width() as u32;
            let composite_h = rect.height() as u32;
            if composite_w > 0 && composite_h > 0 {
                // Zoomed background dimensions + pan offset
                let zbg_w = (bg_w as f32 * zoom) as i32;
                let zbg_h = (bg_h as f32 * zoom) as i32;
                let bg_ox = (composite_w as i32 - zbg_w) / 2 + preview.bg_pan.x as i32;
                let bg_oy = (composite_h as i32 - zbg_h) / 2 + preview.bg_pan.y as i32;

                // Crosshair position (center of crosshair at mouse or at center of rect)
                // Apply crosshair scale to the rendered size
                let (sn, sd) = preview.crosshair_scale;
                let ch_w = width * sn / sd;
                let ch_h = height * sn / sd;
                let (ch_cx, ch_cy) = if let Some(mpos) = mouse_pos {
                    let local = mpos - rect.min;
                    (local.x as i32, local.y as i32)
                } else {
                    (composite_w as i32 / 2, composite_h as i32 / 2)
                };
                let ch_ox = ch_cx - ch_w as i32 / 2;
                let ch_oy = ch_cy - ch_h as i32 / 2;

                let frame = preview.animation_frame();
                let ext_x = extent_x.cast_unsigned().min(preview.max_preview_size());
                let ext_y = extent_y.cast_unsigned().min(preview.max_preview_size());
                let raster_w = ch_w.min(max_size);
                let raster_h = ch_h.min(max_size);

                // Rasterize crosshair + masks, reusing the last result while
                // pieces, scale and size are unchanged (the SVG work is what
                // gets slow at large scales).
                let animated = pieces.iter().any(has_animated_color);
                let raster_key = hash_of(&[
                    &serde_json::to_string(pieces).unwrap_or_default(),
                    &serde_json::to_string(groups).unwrap_or_default(),
                    &(sn, sd, ext_x, ext_y, raster_w, raster_h),
                    &animated.then(|| frame.to_bits()),
                ]);
                if preview.raster_cache.as_ref().map(|c| c.key) != Some(raster_key) {
                    let crosshair_rgba = {
                        let eff = apply_color_override_for_export(pieces, frame);
                        let svg = if sn != 1 || sd != 1 {
                            generate_svg_scaled(ext_x, ext_y, &eff, sn, sd)
                        } else {
                            generate_svg(ext_x, ext_y, &eff)
                        };
                        rasterize_svg(&svg, raster_w, raster_h)
                    };
                    let masks = active
                        .iter()
                        .filter_map(|(id, effects)| {
                            let mask_pieces = dynamic_mask_pieces(pieces, *id);
                            let svg = if sn != 1 || sd != 1 {
                                generate_svg_scaled(ext_x, ext_y, &mask_pieces, sn, sd)
                            } else {
                                generate_svg(ext_x, ext_y, &mask_pieces)
                            };
                            rasterize_svg(&svg, raster_w, raster_h).map(|m| ((*effects).clone(), m))
                        })
                        .collect();
                    preview.raster_cache = Some(std::rc::Rc::new(RasterCache {
                        key: raster_key,
                        crosshair_rgba,
                        masks,
                    }));
                }
                let raster = preview.raster_cache.clone().expect("raster cache just filled");
                let crosshair_rgba = &raster.crosshair_rgba;
                let masks_vec = &raster.masks;

                let actual_ch_w = raster_w;
                let actual_ch_h = raster_h;

                // Precompute scale ratio for honest dynamic effect sampling.
                // When scaled and honest_pixels is on, we snap background samples
                // to the native pixel grid so a 2x2 dot scaled to 20x20 still
                // only samples 4 bg pixels.
                let scale_ratio = sn as f32 / sd as f32;
                let is_scaled = sn != sd && preview.honest_pixels;

                // Nothing the composite depends on changed (e.g. the mouse is
                // still): keep the texture instead of redoing every pixel.
                let composite_key = hash_of(&[
                    &raster_key,
                    &(composite_w, composite_h, ch_ox, ch_oy, bg_ox, bg_oy),
                    &(zoom.to_bits(), is_scaled, scale_ratio.to_bits()),
                    &preview.selected_bg.label(),
                ]);
                let reuse = composite_key == preview.composite_key
                    && preview.composite_texture.as_ref().is_some_and(|t| {
                        t.size() == [composite_w as usize, composite_h as usize]
                    });
                let tex = if reuse {
                    preview.composite_texture.clone().expect("checked above")
                } else {
                preview.composite_key = composite_key;
                // Build composite: background (zoomed) → dynamic effects → crosshair overlay (native)
                // Honest-pixels mode samples one background pixel per native
                // crosshair pixel, so many composite pixels share one result.
                let mut native_cache: std::collections::HashMap<(i32, i32, usize), (f32, f32, f32)> =
                    std::collections::HashMap::new();
                // Pool only the part of the background under the crosshair, per
                // group (each group has its own size and mode).
                let to_bg = |cx: f32, off: i32| (cx - off as f32) / zoom;
                let rx0 = to_bg(ch_ox as f32, bg_ox).floor() as i32 - 2;
                let rx1 = to_bg((ch_ox + actual_ch_w as i32) as f32, bg_ox).ceil() as i32 + 2;
                let ry0 = to_bg(ch_oy as f32, bg_oy).floor() as i32 - 2;
                let ry1 = to_bg((ch_oy + actual_ch_h as i32) as f32, bg_oy).ceil() as i32 + 2;
                let pooled: Vec<Option<PooledRegion>> = masks_vec
                    .iter()
                    .map(|(effects, _)| {
                        let n = effects.sample_size_odd() as i32;
                        (n > 1).then(|| pool_region(&bg, rx0, ry0, rx1, ry1, n, effects.pooling))
                    })
                    .collect();
                let sample_bg = |gi: usize, x: i32, y: i32| -> (f32, f32, f32) {
                    if let Some(v) = pooled[gi].as_ref().and_then(|t| t.get(x, y)) {
                        return v;
                    }
                    let p = bg.get_pixel(x.clamp(0, bg_w as i32 - 1) as u32, y.clamp(0, bg_h as i32 - 1) as u32);
                    (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0)
                };
                let mut pixels = vec![0u8; (composite_w * composite_h * 4) as usize];
                for y in 0..composite_h {
                    for x in 0..composite_w {
                        let pi = ((y * composite_w + x) * 4) as usize;

                        // 1. Background pixel (sample from zoomed coordinates)
                        let bx_f = (x as f32 - bg_ox as f32) / zoom;
                        let by_f = (y as f32 - bg_oy as f32) / zoom;
                        let bx = bx_f as i32;
                        let by = by_f as i32;
                        let (mut pr, mut pg, mut pb) = if bx_f >= 0.0 && by_f >= 0.0 && bx >= 0 && by >= 0 && bx < bg_w as i32 && by < bg_h as i32 {
                            let px = bg.get_pixel(bx as u32, by as u32);
                            (px[0], px[1], px[2])
                        } else {
                            (20u8, 20u8, 20u8)
                        };

                        // Check if this pixel falls within the crosshair bounds
                        let lx = x as i32 - ch_ox;
                        let ly = y as i32 - ch_oy;
                        if lx >= 0 && ly >= 0 && lx < actual_ch_w as i32 && ly < actual_ch_h as i32 {
                            let ci = ((ly as u32 * actual_ch_w + lx as u32) * 4) as usize;

                            // 2. Dynamic effects (where mask is white, transform bg pixel)
                            //    When crosshair is scaled, snap bg sample to native pixel
                            //    grid so the effect honestly represents native resolution.
                            for (gi, (effects, mask)) in masks_vec.iter().enumerate() {
                                if ci + 3 < mask.len() {
                                    let mask_alpha = mask[ci] as f32 / 255.0;
                                    if mask_alpha > 0.001 {
                                    let (dr, dg, db) = if is_scaled {
                                        // Quantize to native crosshair pixel grid
                                        let native_x = (lx as f32 / scale_ratio).floor();
                                        let native_y = (ly as f32 / scale_ratio).floor();
                                        let memo_key = (native_x as i32, native_y as i32, gi);
                                        let cached = native_cache.get(&memo_key).copied();
                                        if let Some(hit) = cached {
                                            hit
                                        } else {
                                        // Map native pixel center back to composite space
                                        let snapped_cx = ch_ox as f32 + (native_x + 0.5) * scale_ratio;
                                        let snapped_cy = ch_oy as f32 + (native_y + 0.5) * scale_ratio;
                                        // Sample background at that snapped position
                                        let sbx = ((snapped_cx - bg_ox as f32) / zoom) as i32;
                                        let sby = ((snapped_cy - bg_oy as f32) / zoom) as i32;
                                        let v = if sbx >= 0 && sby >= 0 && sbx < bg_w as i32 && sby < bg_h as i32 {
                                            let (r, g, b) = sample_bg(gi, sbx, sby);
                                            effects.apply_to_pixel(r, g, b)
                                        } else {
                                            effects.apply_to_pixel(
                                                20.0 / 255.0,
                                                20.0 / 255.0,
                                                20.0 / 255.0,
                                            )
                                        };
                                        native_cache.insert(memo_key, v);
                                        v
                                        }
                                    } else if bx >= 0 && by >= 0 && bx < bg_w as i32 && by < bg_h as i32 {
                                        let (r, g, b) = sample_bg(gi, bx, by);
                                        effects.apply_to_pixel(r, g, b)
                                    } else {
                                        effects.apply_to_pixel(
                                            pr as f32 / 255.0,
                                            pg as f32 / 255.0,
                                            pb as f32 / 255.0,
                                        )
                                    };
                                    // Blend: lerp between original bg and effected result by mask alpha
                                    let orig_r = pr as f32 / 255.0;
                                    let orig_g = pg as f32 / 255.0;
                                    let orig_b = pb as f32 / 255.0;
                                    pr = ((orig_r + (dr - orig_r) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                    pg = ((orig_g + (dg - orig_g) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                    pb = ((orig_b + (db - orig_b) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                    }
                                }
                            }

                            // 3. Crosshair overlay (alpha-blend)
                            if let Some(ref ch) = crosshair_rgba {
                                if ci + 3 < ch.len() {
                                    let ca = ch[ci + 3] as f32 / 255.0;
                                    if ca > 0.001 {
                                        let sr = ch[ci] as f32 / 255.0;
                                        let sg = ch[ci + 1] as f32 / 255.0;
                                        let sb = ch[ci + 2] as f32 / 255.0;
                                        pr = ((sr + pr as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                        pg = ((sg + pg as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                        pb = ((sb + pb as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                    }
                                }
                            }
                        }

                        pixels[pi] = pr;
                        pixels[pi + 1] = pg;
                        pixels[pi + 2] = pb;
                        pixels[pi + 3] = 255;
                    }
                }

                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [composite_w as usize, composite_h as usize],
                    &pixels,
                );

                // Upload composite texture
                match &mut preview.composite_texture {
                    Some(tex) if tex.size() == image.size => {
                        tex.set_partial([0, 0], image, egui::TextureOptions::NEAREST);
                        tex.clone()
                    }
                    _ => {
                        let tex = ctx.load_texture("preview_composite", image, egui::TextureOptions::NEAREST);
                        preview.composite_texture = Some(tex.clone());
                        tex
                    }
                }
                };

                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        } else {
            // Background selected but failed to load
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(20));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Background failed to load", egui::FontId::default(), egui::Color32::GRAY);
        }
        } // end !is_capped else
    } else {
        // No background — apply dynamic effects with gray background, scale with zoom
        if !active.is_empty() {
            ctx.request_repaint();

            let ext_x = extent_x.cast_unsigned().min(preview.max_preview_size());
            let ext_y = extent_y.cast_unsigned().min(preview.max_preview_size());
            let raster_w = ext_x * 2;
            let raster_h = ext_y * 2;
            let frame = preview.animation_frame();

            let crosshair_rgba = {
                let eff = apply_color_override_for_export(pieces, frame);
                let svg = generate_svg(ext_x, ext_y, &eff);
                rasterize_svg(&svg, raster_w, raster_h)
            };

            let masks: Vec<(&crate::types::DynamicEffects, Vec<u8>)> = active
                .iter()
                .filter_map(|(id, effects)| {
                    let mask_pieces = dynamic_mask_pieces(pieces, *id);
                    let svg = generate_svg(ext_x, ext_y, &mask_pieces);
                    rasterize_svg(&svg, raster_w, raster_h).map(|m| (*effects, m))
                })
                .collect();

            if raster_w > 0 && raster_h > 0 {
                let mut pixels = vec![0u8; (raster_w * raster_h * 4) as usize];
                for y in 0..raster_h as usize {
                    for x in 0..raster_w as usize {
                        let pi = ((y * raster_w as usize + x) * 4) as usize;

                        let (mut pr, mut pg, mut pb) = (20u8, 20u8, 20u8);

                        for (effects, mask) in &masks {
                            if pi + 3 < mask.len() {
                                let mask_alpha = mask[pi] as f32 / 255.0;
                                if mask_alpha > 0.001 {
                                let (er, eg, eb) = effects.apply_to_pixel(
                                    pr as f32 / 255.0,
                                    pg as f32 / 255.0,
                                    pb as f32 / 255.0,
                                );
                                let orig_r = pr as f32 / 255.0;
                                let orig_g = pg as f32 / 255.0;
                                let orig_b = pb as f32 / 255.0;
                                pr = ((orig_r + (er - orig_r) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                pg = ((orig_g + (eg - orig_g) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                pb = ((orig_b + (eb - orig_b) * mask_alpha) * 255.0).clamp(0.0, 255.0) as u8;
                                }
                            }
                        }

                        if let Some(ref ch) = crosshair_rgba {
                            if pi + 3 < ch.len() {
                                let ca = ch[pi + 3] as f32 / 255.0;
                                if ca > 0.001 {
                                    let sr = ch[pi] as f32 / 255.0;
                                    let sg = ch[pi + 1] as f32 / 255.0;
                                    let sb = ch[pi + 2] as f32 / 255.0;
                                    pr = ((sr + pr as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                    pg = ((sg + pg as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                    pb = ((sb + pb as f32 / 255.0 * (1.0 - ca)) * 255.0).clamp(0.0, 255.0) as u8;
                                }
                            }
                        }

                        pixels[pi] = pr;
                        pixels[pi + 1] = pg;
                        pixels[pi + 2] = pb;
                        pixels[pi + 3] = 255;
                    }
                }

                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [raster_w as usize, raster_h as usize],
                    &pixels,
                );

                let tex = ctx.load_texture("crosshair_dynamic", image, egui::TextureOptions::NEAREST);

                let tex_size = tex.size();
                let aspect = tex_size[0] as f32 / tex_size[1] as f32;
                let avail_w = available_size.x;
                let avail_h = available_size.y;
                let (base_w, base_h) = if avail_w / avail_h > aspect {
                    (avail_h * aspect, avail_h)
                } else {
                    (avail_w, avail_w / aspect)
                };
                let img_w = base_w * zoom;
                let img_h = base_h * zoom;

                egui::ScrollArea::both()
                    .max_width(avail_w)
                    .max_height(avail_h)
                    .show(ui, |ui| {
                        let img_size = egui::Vec2::new(img_w, img_h);
                        let (rect, _response) = ui.allocate_exact_size(img_size, egui::Sense::hover());
                        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(20));
                        ui.painter().image(
                            tex.id(), rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    });
                return;
            }
        }

        // No dynamic effects — use standard texture path
        if let Some(texture) = preview.texture(ctx, pieces) {
            let tex_size = texture.size();
            let aspect = tex_size[0] as f32 / tex_size[1] as f32;
            let avail_w = available_size.x;
            let avail_h = available_size.y;
            // Fit to available space, then apply zoom
            let (base_w, base_h) = if avail_w / avail_h > aspect {
                (avail_h * aspect, avail_h)
            } else {
                (avail_w, avail_w / aspect)
            };
            let img_w = base_w * zoom;
            let img_h = base_h * zoom;

            // Use a scroll area so zoomed-in crosshair can be panned
            egui::ScrollArea::both()
                .max_width(avail_w)
                .max_height(avail_h)
                .show(ui, |ui| {
                    let img_size = egui::Vec2::new(img_w, img_h);
                    let (rect, _response) = ui.allocate_exact_size(img_size, egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(20));
                    ui.painter().image(
                        texture.id(), rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                });
        } else {
            let (rect, _) = ui.allocate_exact_size(available_size, egui::Sense::hover());
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(20));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "No preview available", egui::FontId::default(), egui::Color32::GRAY);
        }
    }
}

#[cfg(test)]
mod layering_tests {
    use super::*;
    use crate::types::{default_cross_piece, ColorType};

    fn cross(color: &str, dynamic: bool) -> Piece {
        cross_in(color, dynamic.then_some(0))
    }

    fn cross_in(color: &str, group: Option<u32>) -> Piece {
        let mut p = default_cross_piece();
        p.set_color_override(color);
        if let Some(group) = group {
            if let Piece::Cross { color_type, .. } = &mut p {
                *color_type = ColorType::Dynamic {
                    group,
                    effects: Default::default(),
                    _legacy_mode: None,
                    _legacy_modes: None,
                    _legacy_tint: None,
                    _legacy_strength: None,
                };
            }
        }
        p
    }

    fn render(pieces: &[Piece]) -> Vec<u8> {
        let svg = generate_svg(20, 20, pieces);
        rasterize_svg(&svg, 40, 40).unwrap()
    }

    #[test]
    fn solid_above_dynamic_cuts_the_mask() {
        let dyn_only = render(&dynamic_mask_pieces(&[cross("#ff0000ff", true)], 0));
        let covered = render(&dynamic_mask_pieces(&[cross("#ff0000ff", true), cross("#00ff00ff", false)], 0));
        assert!(dyn_only.chunks(4).any(|p| p[0] == 255));
        assert!(covered.chunks(4).all(|p| p[0] == 0));
    }

    #[test]
    fn dynamic_above_solid_erases_it() {
        let pieces = [cross("#00ff00ff", false), cross("#ff0000ff", true)];
        let eff = apply_color_override_for_export(&pieces, 0.0);
        assert!(render(&eff).chunks(4).all(|p| p[3] == 0));
    }

    #[test]
    fn translucent_replaces_what_is_below() {
        let one = render(&[cross("#0000ff80", false)]);
        let two = render(&[cross("#ff0000ff", false), cross("#0000ff80", false)]);
        assert_eq!(one, two);
    }

    #[test]
    fn later_eraser_erases_earlier_layers() {
        let mut e = cross("#000000ff", false);
        if let Piece::Cross { color_type, .. } = &mut e {
            *color_type = ColorType::Eraser;
        }
        let pieces = [cross("#ff0000ff", false), e.clone(), cross("#00ff00ff", false), e];
        assert!(render(&pieces).chunks(4).all(|p| p[3] == 0));
    }

    fn lit(mask: &[u8]) -> bool {
        mask.chunks(4).any(|p| p[0] > 0)
    }

    #[test]
    fn groups_get_separate_masks() {
        let pieces = [cross_in("#ffffffff", Some(0)), cross_in("#ffffffff", Some(1))];
        // both crosses sit at the same place: group 1 is above group 0
        let m0 = render(&dynamic_mask_pieces(&pieces, 0));
        let m1 = render(&dynamic_mask_pieces(&pieces, 1));
        assert!(!lit(&m0), "group 0 is fully covered by group 1 above it");
        assert!(lit(&m1));
    }

    #[test]
    fn group_below_is_erased_and_group_above_wins() {
        let a_above = [cross_in("#ffffffff", Some(1)), cross_in("#ffffffff", Some(0))];
        assert!(lit(&render(&dynamic_mask_pieces(&a_above, 0))));
        assert!(!lit(&render(&dynamic_mask_pieces(&a_above, 1))));
    }

    #[test]
    fn static_above_group_cuts_it_in_every_mask() {
        let pieces = [cross_in("#ffffffff", Some(0)), cross_in("#ffffffff", Some(1)), cross("#00ff00ff", false)];
        assert!(!lit(&render(&dynamic_mask_pieces(&pieces, 0))));
        assert!(!lit(&render(&dynamic_mask_pieces(&pieces, 1))));
    }

    #[test]
    fn export_writes_one_mask_and_block_per_group_and_prunes() {
        use crate::types::DynamicEffects;
        let dir = std::env::temp_dir().join(format!("cm_groups_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let json = dir.join("t.json");

        let mut g0 = DynamicEffects::default();
        g0.invert.enabled = true;
        let mut g2 = DynamicEffects::default();
        g2.saturate.enabled = true;
        let off = DynamicEffects::default();
        let groups = [g0, off, g2];

        // two crosses side by side would overlap here, so give them different layers
        let mut a = cross_in("#ffffffff", Some(0));
        let mut b = cross_in("#ffffffff", Some(2));
        if let (Piece::Cross { rotation: ra, .. }, Piece::Cross { rotation: rb, .. }) = (&mut a, &mut b) {
            *ra = 0.0;
            *rb = 45.0;
        }
        let pieces = [a, b];
        save_dynamic_export(&json, &pieces, &groups, 20, 20);

        assert!(dir.join("t.dynamic.png").exists());
        assert!(!dir.join("t.dynamic.1.png").exists(), "group 1 has no pieces/effects");
        assert!(dir.join("t.dynamic.2.png").exists());
        let cfg = std::fs::read_to_string(dir.join("t.dynamic.cfg")).unwrap();
        assert!(cfg.contains("group 0\ninvert"));
        assert!(cfg.contains("group 2\nsaturate"));

        // dropping group 2's piece prunes its mask and block
        save_dynamic_export(&json, &pieces[..1], &groups, 20, 20);
        assert!(!dir.join("t.dynamic.2.png").exists());
        let cfg = std::fs::read_to_string(dir.join("t.dynamic.cfg")).unwrap();
        assert!(!cfg.contains("group 2"));

        // nothing dynamic left: everything is removed
        save_dynamic_export(&json, &[cross("#ff0000ff", false)], &groups, 20, 20);
        assert!(!dir.join("t.dynamic.png").exists());
        assert!(!dir.join("t.dynamic.cfg").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pooled_region_matches_per_pixel_sampling() {
        use crate::types::{DynamicEffects, PoolMode};
        let (w, h) = (23u32, 17u32);
        let mut bg = image::RgbaImage::new(w, h);
        let mut seed = 12345u32;
        for p in bg.pixels_mut() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *p = image::Rgba([(seed >> 8) as u8, (seed >> 16) as u8, (seed >> 24) as u8, 255]);
        }
        // a sub-rectangle touching the left/top edge, and the whole image
        for (rx0, ry0, rx1, ry1) in [(-4, -3, 9, 8), (0, 0, w as i32, h as i32), (15, 10, 40, 30)] {
            for mode in PoolMode::ALL {
                for n in [3u32, 5, 15] {
                    let mut e = DynamicEffects::default();
                    e.sample_size = n;
                    e.pooling = mode;
                    let fast = pool_region(&bg, rx0, ry0, rx1, ry1, n as i32, mode);
                    for y in fast.y0..fast.y0 + fast.h {
                        for x in fast.x0..fast.x0 + fast.w {
                            let slow = e.pool(x, y, |sx, sy| {
                                let p = bg.get_pixel(sx.clamp(0, w as i32 - 1) as u32, sy.clamp(0, h as i32 - 1) as u32);
                                (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0)
                            });
                            let got = fast.get(x, y).unwrap();
                            assert!((got.0 - slow.0).abs() < 1e-4, "{mode:?} n={n} at {x},{y}");
                            assert!((got.1 - slow.1).abs() < 1e-4);
                            assert!((got.2 - slow.2).abs() < 1e-4);
                        }
                    }
                }
            }
        }
    }
}
