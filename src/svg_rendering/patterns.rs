use crate::types::Piece;

pub fn offset_piece(piece: &Piece, dx: i32, dy: i32) -> Piece {
    let mut p = piece.clone();
    match &mut p {
        Piece::Cross { origin, .. }
        | Piece::Dot { origin, .. }
        | Piece::Line { origin, .. }
        | Piece::Rectangle { origin, .. }
        | Piece::HappyFace { origin, .. } => {
            origin.0 += dx;
            origin.1 += dy;
        }
        Piece::RectPattern { origin, obj, .. } | Piece::CircPattern { origin, obj, .. } => {
            origin.0 += dx;
            origin.1 += dy;
            **obj = offset_piece(obj, dx, dy);
        }
    }
    p
}

fn collect_pattern_pieces(piece: &Piece, pieces: &mut Vec<Piece>) {
    if !piece.is_visible() {
        return;
    }
    match piece {
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
            let xq: u32 = *x_quantity;
            let yq: u32 = *y_quantity;
            for xi in 0..xq {
                for yi in 0..yq {
                    let dx = ox + (xi as i32) * *x_distance;
                    let dy = oy + (yi as i32) * *y_distance;
                    let offset_obj = offset_piece(obj, dx, dy);
                    collect_pattern_pieces(&offset_obj, pieces);
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
            if *quantity == 0 {
                return;
            }
            let (ox, oy) = *origin;
            let angle_step = 360.0 / f64::from(*quantity);
            for i in 0..*quantity {
let angle_deg = -start_deg + 90.0 + f64::from(i).mul_add(angle_step, 0.0);
            let angle_rad = angle_deg.to_radians();
                #[allow(clippy::cast_possible_truncation)]
                let dx = ox + (f64::from(*radius) * angle_rad.cos()).round() as i32;
                #[allow(clippy::cast_possible_truncation)]
                let dy = oy + (f64::from(*radius) * angle_rad.sin()).round() as i32;
                let offset_obj = offset_piece(obj, dx, dy);
                collect_pattern_pieces(&offset_obj, pieces);
            }
        }
        _ => {
            pieces.push(piece.clone());
        }
    }
}

pub fn expand_pieces(pieces: &[Piece]) -> Vec<Piece> {
    let mut expanded = Vec::new();
    for piece in pieces {
        collect_pattern_pieces(piece, &mut expanded);
    }
    expanded
}
