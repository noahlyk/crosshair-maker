mod cross;
mod dot;
mod happy_face;
mod line;
mod patterns;
mod rectangle;

use std::fmt::Write;

use crate::types::Piece;

// ── shared helpers ──────────────────────────────────────────────────

/// Split an integer dimension around a center point.
///
/// Returns `(neg_side, pos_side)` — how many units extend in the negative
/// vs positive direction.  For even dimensions the split is perfectly
/// symmetric.  For odd dimensions the `anchor_component` (the relevant
/// axis of `OddAnchor::offset()`) decides where the extra unit goes:
///
///  * `0.0`  (BottomRight-x / BottomLeft-y) → extra goes positive
///  * `-1.0` (TopLeft-x / TopRight-y)       → extra goes negative
///  * `-0.5` (Center)                       → split fractionally (half+0.5 each)
///
/// Example: dim=5, anchor=-1  → (3, 2)  — extra pixel on negative side
///          dim=5, anchor=0   → (2, 3)  — extra pixel on positive side
///          dim=5, anchor=-0.5→ (2.5, 2.5)
pub(crate) fn split_dim(dim: i32, anchor_component: f64) -> (f64, f64) {
    let half = dim / 2;
    if dim % 2 == 0 {
        (half as f64, half as f64)
    } else {
        (
            half as f64 - anchor_component,
            half as f64 + 1.0 + anchor_component,
        )
    }
}

/// Format a coordinate: integer when possible, one-decimal otherwise.
pub(crate) fn fmt_coord(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Format a dimension (width / height): integer when possible, one-decimal otherwise.
pub(crate) fn fmt_dim(v: f64) -> String {
    fmt_coord(v)
}

/// Emit an SVG `<rect>` with f64 position *and* f64 size.
pub(crate) fn write_rect(svg: &mut String, x: f64, y: f64, w: f64, h: f64, color: &str) {
    let fill = apply_color(color);
    write!(
        svg,
        r#"<rect x="{}" y="{}" width="{}" height="{}" {fill}/>"#,
        fmt_coord(x),
        fmt_coord(y),
        fmt_dim(w),
        fmt_dim(h),
    )
    .unwrap();
}

/// Build the SVG `fill` (+ optional `fill-opacity`) attribute string.
pub(crate) fn apply_color(color: &str) -> String {
    if let Some(hex) = color.strip_prefix('#') {
        if hex.len() == 8 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
                u8::from_str_radix(&hex[6..8], 16),
            ) {
                if a < 255 {
                    let opacity = f64::from(a) / 255.0;
                    return format!(
                        "fill=\"#{r:02x}{g:02x}{b:02x}\" fill-opacity=\"{opacity:.3}\"",
                    );
                }
                return format!("fill=\"#{r:02x}{g:02x}{b:02x}\"");
            }
        } else if hex.len() == 6 {
            return format!("fill=\"{color}\"");
        }
    }
    format!("fill=\"{color}\"")
}

/// Bounding box of an axis-aligned box after rotating it by `deg` degrees
/// around `(px, py)`.  Returns `(min_x, max_x, min_y, max_y)`.
fn rotate_bounds(px: f64, py: f64, deg: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> (f64, f64, f64, f64) {
    let (sin_r, cos_r) = deg.to_radians().sin_cos();
    let mut out = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        let (dx, dy) = (x - px, y - py);
        let rx = px + dx * cos_r - dy * sin_r;
        let ry = py + dx * sin_r + dy * cos_r;
        out = (out.0.min(rx), out.1.max(rx), out.2.min(ry), out.3.max(ry));
    }
    out
}

// ── public API ──────────────────────────────────────────────────────

pub use patterns::{expand_pieces, offset_piece};

/// Tight bounding box of all visible pieces, in SVG-relative coordinates
/// (x = piece-space x, y = flipped piece-space y).  Values are relative to
/// the SVG center point (cx, cy).
pub fn infer_bounds(pieces: &[Piece]) -> (f64, f64, f64, f64) {
    let mut min_x: f64 = f64::MAX;
    let mut max_x: f64 = f64::MIN;
    let mut min_y: f64 = f64::MAX;
    let mut max_y: f64 = f64::MIN;

    for piece in pieces {
        if !piece.is_visible() {
            continue;
        }
let (ax, ay) = piece.odd_anchor().offset();
        match piece {
            Piece::Cross {
                origin,
                left_gap, right_gap, top_gap, bottom_gap,
                left_thickness, right_thickness, top_thickness, bottom_thickness,
                left_length, right_length, top_length, bottom_length,
                ..
            } => {
                let (ox, oy) = *origin;

                let (left_gap_pos, _) = split_dim(*left_gap, ax);
                let (right_gap_pos, _) = split_dim(*right_gap, ax);
                let (top_gap_pos, _) = split_dim(*top_gap, ay);
                let (bottom_gap_pos, _) = split_dim(*bottom_gap, ay);

                let (left_thick_neg, _) = split_dim(*left_thickness, ax);
                let (_, right_thick_pos) = split_dim(*right_thickness, ax);
                let (top_thick_neg, _) = split_dim(*top_thickness, ay);
                let (_, bottom_thick_pos) = split_dim(*bottom_thickness, ay);

                let top_reach = top_gap_pos as f64 + *top_length as f64;
                let bot_reach = bottom_gap_pos as f64 + *bottom_length as f64;
                let left_reach = left_gap_pos as f64 + *left_length as f64;
                let right_reach = right_gap_pos as f64 + *right_length as f64;

                min_x = min_x.min(ox as f64 - left_reach - left_thick_neg).min(ox as f64 - left_thick_neg);
                max_x = max_x.max(ox as f64 + right_reach + right_thick_pos).max(ox as f64 + right_thick_pos);
                min_y = min_y.min(-(oy as f64) - top_reach - top_thick_neg).min(-(oy as f64) - top_thick_neg);
                max_y = max_y.max(-(oy as f64) + bot_reach + bottom_thick_pos).max(-(oy as f64) + bottom_thick_pos);
                if let Piece::Cross { rotation, .. } = piece {
                    if *rotation != 0.0 {
                        let (x0, x1, y0, y1) = rotate_bounds(
                            ox as f64, -(oy as f64), *rotation,
                            (ox as f64 - left_reach - left_thick_neg).min(ox as f64 - left_thick_neg),
                            (ox as f64 + right_reach + right_thick_pos).max(ox as f64 + right_thick_pos),
                            (-(oy as f64) - top_reach - top_thick_neg).min(-(oy as f64) - top_thick_neg),
                            (-(oy as f64) + bot_reach + bottom_thick_pos).max(-(oy as f64) + bottom_thick_pos),
                        );
                        min_x = min_x.min(x0);
                        max_x = max_x.max(x1);
                        min_y = min_y.min(y0);
                        max_y = max_y.max(y1);
                    }
                }
            }
            Piece::Dot { origin, size, .. } => {
                let s = size.cast_signed();
                let (ox, oy) = *origin;
                let (neg, pos) = split_dim(s, ax);
                let (neg_y, pos_y) = split_dim(s, ay);
                min_x = min_x.min(ox as f64 - neg);
                max_x = max_x.max(ox as f64 + pos);
                min_y = min_y.min(-(oy as f64) - neg_y);
                max_y = max_y.max(-(oy as f64) + pos_y);
            }
            Piece::Line {
                origin,
                vector,
                thickness,
                ..
            } => {
                let (ox, oy) = *origin;
                let (vx, vy) = *vector;
                let (t_neg_x, t_pos_x) = split_dim(*thickness, ax);
                let (t_neg_y, t_pos_y) = split_dim(*thickness, ay);
                let x1 = ox as f64;
                let x2 = (ox + vx) as f64;
                let y1 = oy as f64;
                let y2 = (oy + vy) as f64;
                min_x = min_x.min(x1.min(x2) - t_neg_x);
                max_x = max_x.max(x1.max(x2) + t_pos_x);
                min_y = min_y.min((-y1).min(-y2) - t_neg_y);
                max_y = max_y.max((-y1).max(-y2) + t_pos_y);
            }
            Piece::Rectangle {
                origin,
                width,
                height,
                ..
            } => {
                let (ox, oy) = *origin;
                let w = width.cast_signed();
                let h = height.cast_signed();
                let (w_neg, w_pos) = split_dim(w, ax);
                let (h_neg, h_pos) = split_dim(h, ay);
                min_x = min_x.min(ox as f64 - w_neg);
                max_x = max_x.max(ox as f64 + w_pos);
                min_y = min_y.min(-(oy as f64) - h_neg);
                max_y = max_y.max(-(oy as f64) + h_pos);
            }
            Piece::RectPattern {
                origin,
                x_distance,
                x_quantity,
                y_distance,
                y_quantity,
                obj,
                ..
            } => {
                let (ox, oy) = *origin;
                let xq = x_quantity.cast_signed();
                let yq = y_quantity.cast_signed();
                let x_corners = [ox, ox + (xq - 1) * *x_distance];
                let y_corners = [oy, oy + (yq - 1) * *y_distance];
                for &px in &x_corners {
                    for &py in &y_corners {
                        let vp = offset_piece(obj, px, py);
                        extent_of_primitive(&vp, &mut min_x, &mut max_x, &mut min_y, &mut max_y);
                    }
                }
            }
            Piece::CircPattern {
                origin,
                radius,
                quantity,
                start_deg,
                obj,
                ..
            } => {
                let (ox, oy) = *origin;
                let q = *quantity;
                if q == 0 {
                    continue;
                }
                let angle_step = 360.0 / f64::from(q);
                for i in 0..q {
                    let angle_deg = -start_deg + 90.0 + f64::from(i).mul_add(angle_step, 0.0);
                    let angle_rad = angle_deg.to_radians();
                    #[allow(clippy::cast_possible_truncation)]
                    let dx = ox + (f64::from(*radius) * angle_rad.cos()).round() as i32;
                    #[allow(clippy::cast_possible_truncation)]
                    let dy = oy + (f64::from(*radius) * angle_rad.sin()).round() as i32;
                    let vp = offset_piece(obj, dx, dy);
                    extent_of_primitive(&vp, &mut min_x, &mut max_x, &mut min_y, &mut max_y);
                }
            }
            Piece::HappyFace { origin, size, .. } => {
                let (ox, oy) = *origin;
                let s = f64::from(size.cast_signed());
                let scale = s / 3.0;
                let dot = size.cast_signed();
                let (d_neg_x, d_pos_x) = split_dim(dot, ax);
                let (d_neg_y, d_pos_y) = split_dim(dot, ay);
                let offsets: [(i32, i32); 7] = [
                    (-3, 4), (3, 4), (-6, -1), (-3, -4), (0, -4), (3, -4), (6, -1),
                ];
                for (dx, dy) in offsets {
                    let px = ox as f64 + (f64::from(dx) * scale).round();
                    let py = -(oy as f64) - (f64::from(dy) * scale).round();
                    min_x = min_x.min(px - d_neg_x);
                    max_x = max_x.max(px + d_pos_x);
                    min_y = min_y.min(py - d_neg_y);
                    max_y = max_y.max(py + d_pos_y);
                }
            }
        }
    }

    if min_x == f64::MAX {
        min_x = 0.0;
        max_x = 0.0;
        min_y = 0.0;
        max_y = 0.0;
    }

    (min_x, min_y, max_x, max_y)
}

pub fn infer_extent(pieces: &[Piece]) -> (i32, i32) {
    let (min_x, min_y, max_x, max_y) = infer_bounds(pieces);
    #[allow(clippy::cast_possible_truncation)]
    let extent = max_x
        .abs()
        .max(min_x.abs())
        .max(max_y.abs())
        .max(min_y.abs())
        .ceil() as i32;
    (extent, extent)
}

/// Helper: accumulate bounds for a primitive piece used inside pattern
/// expansion in `infer_extent`.  Handles all leaf piece types.
fn extent_of_primitive(
    piece: &Piece,
    min_x: &mut f64,
    max_x: &mut f64,
    min_y: &mut f64,
    max_y: &mut f64,
) {
    let (ax, ay) = piece.odd_anchor().offset();
    match piece {
        Piece::Dot { origin, size, .. } => {
            let s = size.cast_signed();
            let (ox, oy) = *origin;
            let (neg_x, pos_x) = split_dim(s, ax);
            let (neg_y, pos_y) = split_dim(s, ay);
            *min_x = min_x.min(ox as f64 - neg_x);
            *max_x = max_x.max(ox as f64 + pos_x);
            *min_y = min_y.min(-(oy as f64) - neg_y);
            *max_y = max_y.max(-(oy as f64) + pos_y);
        }
        Piece::Line {
            origin,
            vector,
            thickness,
            ..
        } => {
            let (ox, oy) = *origin;
            let (vx, vy) = *vector;
            let (t_neg_x, t_pos_x) = split_dim(*thickness, ax);
            let (t_neg_y, t_pos_y) = split_dim(*thickness, ay);
            let x1 = ox as f64;
            let x2 = (ox + vx) as f64;
            let y1 = oy as f64;
            let y2 = (oy + vy) as f64;
            *min_x = min_x.min(x1.min(x2) - t_neg_x);
            *max_x = max_x.max(x1.max(x2) + t_pos_x);
            *min_y = min_y.min((-y1).min(-y2) - t_neg_y);
            *max_y = max_y.max((-y1).max(-y2) + t_pos_y);
        }
        Piece::Rectangle {
            origin,
            width,
            height,
            ..
        } => {
            let (ox, oy) = *origin;
            let w = width.cast_signed();
            let h = height.cast_signed();
            let (w_neg, w_pos) = split_dim(w, ax);
            let (h_neg, h_pos) = split_dim(h, ay);
            *min_x = min_x.min(ox as f64 - w_neg);
            *max_x = max_x.max(ox as f64 + w_pos);
            *min_y = min_y.min(-(oy as f64) - h_neg);
            *max_y = max_y.max(-(oy as f64) + h_pos);
        }
        Piece::Cross {
            origin,
            left_gap, right_gap, top_gap, bottom_gap,
            left_thickness, right_thickness, top_thickness, bottom_thickness,
            left_length, right_length, top_length, bottom_length,
            ..
        } => {
            let (ox, oy) = *origin;

            let (left_gap_pos, _) = split_dim(*left_gap, ax);
            let (right_gap_pos, _) = split_dim(*right_gap, ax);
            let (top_gap_pos, _) = split_dim(*top_gap, ay);
            let (bottom_gap_pos, _) = split_dim(*bottom_gap, ay);

            let (left_thick_neg, _) = split_dim(*left_thickness, ax);
            let (_, right_thick_pos) = split_dim(*right_thickness, ax);
            let (top_thick_neg, _) = split_dim(*top_thickness, ay);
            let (_, bottom_thick_pos) = split_dim(*bottom_thickness, ay);

            let top_reach = top_gap_pos as f64 + *top_length as f64;
            let bot_reach = bottom_gap_pos as f64 + *bottom_length as f64;
            let left_reach = left_gap_pos as f64 + *left_length as f64;
            let right_reach = right_gap_pos as f64 + *right_length as f64;

            *min_x = min_x.min(ox as f64 - left_reach - left_thick_neg).min(ox as f64 - left_thick_neg);
            *max_x = max_x.max(ox as f64 + right_reach + right_thick_pos).max(ox as f64 + right_thick_pos);
            *min_y = min_y.min(-(oy as f64) - top_reach - top_thick_neg).min(-(oy as f64) - top_thick_neg);
            *max_y = max_y.max(-(oy as f64) + bot_reach + bottom_thick_pos).max(-(oy as f64) + bottom_thick_pos);
            if let Piece::Cross { rotation, .. } = piece {
                if *rotation != 0.0 {
                    let (x0, x1, y0, y1) = rotate_bounds(
                        ox as f64, -(oy as f64), *rotation,
                        (ox as f64 - left_reach - left_thick_neg).min(ox as f64 - left_thick_neg),
                        (ox as f64 + right_reach + right_thick_pos).max(ox as f64 + right_thick_pos),
                        (-(oy as f64) - top_reach - top_thick_neg).min(-(oy as f64) - top_thick_neg),
                        (-(oy as f64) + bot_reach + bottom_thick_pos).max(-(oy as f64) + bottom_thick_pos),
                    );
                    *min_x = min_x.min(x0);
                    *max_x = max_x.max(x1);
                    *min_y = min_y.min(y0);
                    *max_y = max_y.max(y1);
                }
            }
        }
        Piece::HappyFace { origin, size, .. } => {
            let (ox, oy) = *origin;
            let s = f64::from(size.cast_signed());
            let scale = s / 3.0;
            let dot = size.cast_signed();
            let (d_neg_x, d_pos_x) = split_dim(dot, ax);
            let (d_neg_y, d_pos_y) = split_dim(dot, ay);
            let offsets: [(i32, i32); 7] = [
                (-3, 4), (3, 4), (-6, -1), (-3, -4), (0, -4), (3, -4), (6, -1),
            ];
            for (dx, dy) in offsets {
                let px = ox as f64 + (f64::from(dx) * scale).round();
                let py = -(oy as f64) - (f64::from(dy) * scale).round();
                *min_x = min_x.min(px - d_neg_x);
                *max_x = max_x.max(px + d_pos_x);
                *min_y = min_y.min(py - d_neg_y);
                *max_y = max_y.max(py + d_pos_y);
            }
        }
        _ => {}
    }
}

fn is_eraser(piece: &Piece) -> bool {
    matches!(
        piece.color_type(),
        crate::types::ColorType::Eraser | crate::types::ColorType::Dynamic { .. }
    )
}

fn is_dynamic(piece: &Piece) -> bool {
    matches!(piece.color_type(), crate::types::ColorType::Dynamic { .. })
}

fn draw_piece(svg: &mut String, cx: i32, cy: i32, piece: &Piece) {
    match piece {
        Piece::Cross { .. } => cross::draw_cross(svg, cx, cy, piece),
        Piece::Dot { .. } => dot::draw_dot(svg, cx, cy, piece),
        Piece::Line { .. } => line::draw_line(svg, cx, cy, piece),
        Piece::Rectangle { .. } => rectangle::draw_rectangle(svg, cx, cy, piece),
        Piece::HappyFace { .. } => happy_face::draw_happy_face(svg, cx, cy, piece),
        Piece::RectPattern { .. } | Piece::CircPattern { .. } => {}
    }
}

/// Generate an SVG with integer scaling applied.  When `scale_num > scale_den`,
/// every pixel becomes `scale_num/scale_den` pixels — no fractional coordinates.
pub fn generate_svg_scaled(extent_x: u32, extent_y: u32, pieces: &[Piece], scale_num: u32, scale_den: u32) -> String {
    // Scale the output dimensions
    let base_w = extent_x as i32 * 2;
    let base_h = extent_y as i32 * 2;
    let out_w = base_w as u32 * scale_num / scale_den;
    let out_h = base_h as u32 * scale_num / scale_den;
    let scale_f = scale_num as f64 / scale_den as f64;

    let cx = base_w / 2;
    let cy = base_h / 2;

    let expanded = expand_pieces(pieces);

    let mut svg = String::new();
    write!(
        svg,
        r#"<svg width="{out_w}" height="{out_h}" xmlns="http://www.w3.org/2000/svg">"#
    )
    .unwrap();
    write!(
        svg,
        r#"<rect x="0" y="0" width="{out_w}" height="{out_h}" fill-opacity="0"/>"#
    )
    .unwrap();

    // Wrap all content in a scale transform
    write!(svg, r#"<g transform="scale({scale_f})" shape-rendering="crispEdges">"#).unwrap();

    render_pieces_to_svg(&mut svg, base_w, base_h, cx, cy, &expanded);

    svg.push_str("</g></svg>");
    svg
}

pub fn generate_svg(extent_x: u32, extent_y: u32, pieces: &[Piece]) -> String {
    let width = extent_x.cast_signed() * 2;
    let height = extent_y.cast_signed() * 2;
    let cx = width / 2;
    let cy = height / 2;

    let expanded = expand_pieces(pieces);

    let mut svg = String::new();
    write!(
        svg,
        r#"<svg width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg">"#
    )
    .unwrap();
    write!(
        svg,
        r#"<rect x="0" y="0" width="{width}" height="{height}" fill-opacity="0"/>"#
    )
    .unwrap();

    render_pieces_to_svg(&mut svg, width, height, cx, cy, &expanded);

    svg.push_str("</svg>");
    svg
}

/// True when the piece's fill is not fully opaque.
fn is_translucent(piece: &Piece) -> bool {
    let c = piece.base_color();
    match c.strip_prefix('#') {
        Some(hex) if hex.len() == 8 => u8::from_str_radix(&hex[6..8], 16).map_or(false, |a| a < 255),
        _ => false,
    }
}

/// Shared piece rendering logic used by both `generate_svg` and `generate_svg_scaled`.
///
/// Pieces are composited bottom-to-top with "replace" semantics: an eraser or
/// dynamic piece, and any translucent piece, first cuts its own shape out of
/// everything drawn so far, then (unless it is an eraser) draws itself.  Opaque
/// pieces simply paint over what is below.
fn render_pieces_to_svg(svg: &mut String, width: i32, height: i32, cx: i32, cy: i32, expanded: &[Piece]) {
    let mut body = String::new();
    let mut mask_counter: u32 = 0;

    for piece in expanded {
        let erases = is_eraser(piece) || (piece.is_visible() && is_translucent(piece));
        if erases && !body.is_empty() {
            mask_counter += 1;
            let mask_id = format!("em{}", mask_counter);
            let mut cut = String::new();
            write!(cut, r#"<defs><mask id="{mask_id}" maskUnits="userSpaceOnUse" x="0" y="0" width="{width}" height="{height}">"#).unwrap();
            write!(cut, r#"<rect x="0" y="0" width="{width}" height="{height}" fill="white"/>"#).unwrap();
            let mut eraser_piece = piece.clone();
            eraser_piece.set_color_override("#000000ff");
            draw_piece(&mut cut, cx, cy, &eraser_piece);
            cut.push_str("</mask></defs>");
            write!(cut, r#"<g mask="url(#{mask_id})">"#).unwrap();
            cut.push_str(&body);
            cut.push_str("</g>");
            body = cut;
        }
        // Erasers draw nothing, except Dynamic ones which draw with their own color.
        if !is_eraser(piece) || is_dynamic(piece) {
            draw_piece(&mut body, cx, cy, piece);
        }
    }
    svg.push_str(&body);
}
