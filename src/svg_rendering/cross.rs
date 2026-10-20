use crate::types::Piece;
use super::{fmt_coord, split_dim, write_rect};

pub fn draw_cross(svg: &mut String, cx: i32, cy: i32, piece: &Piece) {
    let Piece::Cross {
        origin,
        left_gap, right_gap, top_gap, bottom_gap,
        left_thickness, right_thickness, top_thickness, bottom_thickness,
        left_length, right_length, top_length, bottom_length,
        color,
        odd_anchor,
        rotation,
        center_dot,
        hard_edges,
        ..
    } = piece
    else {
        return;
    };

    let (ox, oy) = *origin;
    let (ax, ay) = odd_anchor.offset();
    let cross_cx = cx as f64 + ox as f64;
    let cross_cy = cy as f64 - oy as f64;

    let rotated = *rotation != 0.0;
    if rotated {
        svg.push_str(&format!(
            r#"<g transform="rotate({} {} {})"{}>"#,
            fmt_coord(*rotation), fmt_coord(cross_cx), fmt_coord(cross_cy),
            if *hard_edges { r#" shape-rendering="crispEdges""# } else { "" },
        ));
    }

    let (_, top_gap_neg) = split_dim(*top_gap, ay);
    let (bottom_gap_pos, _) = split_dim(*bottom_gap, ay);
    let (_, left_gap_neg) = split_dim(*left_gap, ax);
    let (right_gap_pos, _) = split_dim(*right_gap, ax);

    let (_, top_thick_neg) = split_dim(*top_thickness, ay);
    let (bottom_thick_pos, _) = split_dim(*bottom_thickness, ay);
    let (_, left_thick_neg) = split_dim(*left_thickness, ax);
    let (right_thick_pos, _) = split_dim(*right_thickness, ax);

    let top_x = cross_cx - top_thick_neg;
    let bot_x = cross_cx - bottom_thick_pos;
    let left_y = cross_cy - left_thick_neg;
    let right_y = cross_cy - right_thick_pos;

    write_rect(svg, top_x, cross_cy - top_gap_neg - *top_length as f64, *top_thickness as f64, *top_length as f64, color);
    write_rect(svg, bot_x, cross_cy + bottom_gap_pos, *bottom_thickness as f64, *bottom_length as f64, color);

    write_rect(svg, cross_cx - left_gap_neg - *left_length as f64, left_y, *left_length as f64, *left_thickness as f64, color);
    write_rect(svg, cross_cx + right_gap_pos, right_y, *right_length as f64, *right_thickness as f64, color);

    if *center_dot {
        let size = (*left_thickness)
            .max(*right_thickness)
            .max(*top_thickness)
            .max(*bottom_thickness);
        let (neg_x, _) = split_dim(size, ax);
        let (neg_y, _) = split_dim(size, ay);
        write_rect(svg, cross_cx - neg_x, cross_cy - neg_y, size as f64, size as f64, color);
    }

    if rotated {
        svg.push_str("</g>");
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{default_cross_piece, OddAnchor, Piece};

    #[test]
    fn rotation_and_center_dot() {
        let mut p = default_cross_piece();
        let plain = crate::svg_rendering::generate_svg(20, 20, std::slice::from_ref(&p));
        assert!(!plain.contains("rotate"));
        let base = plain.matches("<rect").count();
        if let Piece::Cross { rotation, center_dot, .. } = &mut p {
            *rotation = 45.0;
            *center_dot = true;
        }
        let svg = crate::svg_rendering::generate_svg(20, 20, std::slice::from_ref(&p));
        assert!(svg.contains("rotate(45 "));
        assert_eq!(svg.matches("<rect").count(), base + 1);
        assert!(!svg.contains("crispEdges"));
    }

    #[test]
    fn hard_edges_have_no_partial_alpha() {
        for anchor in [OddAnchor::TopLeft, OddAnchor::BottomRight, OddAnchor::Center] {
            for t in [2, 3] {
                let mut p = default_cross_piece();
                if let Piece::Cross {
                    rotation, hard_edges, odd_anchor, left_thickness, right_thickness,
                    top_thickness, bottom_thickness, left_length, right_length,
                    top_length, bottom_length, ..
                } = &mut p {
                    *rotation = 45.0;
                    *hard_edges = true;
                    *odd_anchor = anchor;
                    for v in [left_thickness, right_thickness, top_thickness, bottom_thickness] { *v = t; }
                    for v in [left_length, right_length, top_length, bottom_length] { *v = 12; }
                }
                let svg = crate::svg_rendering::generate_svg(30, 30, std::slice::from_ref(&p));
                assert!(svg.contains("crispEdges"));
                let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap();
                let mut pm = resvg::tiny_skia::Pixmap::new(60, 60).unwrap();
                resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pm.as_mut());
                assert!(pm.pixels().iter().any(|px| px.alpha() > 0));
                assert!(pm.pixels().iter().all(|px| px.alpha() == 0 || px.alpha() == 255),
                    "partial alpha with {anchor:?} t={t}");
            }
        }
    }
}
