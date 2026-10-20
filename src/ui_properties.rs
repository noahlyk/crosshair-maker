use eframe::egui;

use crate::types::{OddAnchor, Piece, DynamicEffects};

fn slider_i32(ui: &mut egui::Ui, val: &mut i32, range: std::ops::RangeInclusive<i32>, label: &str) -> bool {
    ui.add(egui::Slider::new(val, range).text(label).step_by(1.0).clamping(egui::SliderClamping::Always)).changed()
}

fn slider_u32(ui: &mut egui::Ui, val: &mut u32, range: std::ops::RangeInclusive<u32>, label: &str) -> bool {
    ui.add(egui::Slider::new(val, range).text(label).step_by(1.0).clamping(egui::SliderClamping::Always)).changed()
}

// f64 sliders - NO clamping so effects can go above 1 or below 0
fn slider_f64(ui: &mut egui::Ui, val: &mut f64, range: std::ops::RangeInclusive<f64>, label: &str) -> bool {
    ui.add(egui::Slider::new(val, range).text(label).step_by(1.0).clamping(egui::SliderClamping::Never)).changed()
}

fn slider_f64_fine(ui: &mut egui::Ui, val: &mut f64, range: std::ops::RangeInclusive<f64>, label: &str, step: f64) -> bool {
    ui.add(egui::Slider::new(val, range).text(label).step_by(step).clamping(egui::SliderClamping::Never)).changed()
}

fn edit_origin(ui: &mut egui::Ui, origin: &mut (i32, i32)) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Origin");
        changed |= ui.add(egui::DragValue::new(&mut origin.0).speed(1.0)).changed();
        ui.label("Y");
        changed |= ui.add(egui::DragValue::new(&mut origin.1).speed(1.0)).changed();
    });
    changed
}

fn edit_odd_anchor(ui: &mut egui::Ui, anchor: &mut OddAnchor) -> bool {
    let mut idx = match *anchor {
        OddAnchor::TopRight => 0,
        OddAnchor::TopLeft => 1,
        OddAnchor::BottomLeft => 2,
        OddAnchor::BottomRight => 3,
        OddAnchor::Center => 4,
    };
    let prev = idx;
    egui::ComboBox::from_label("Odd Anchor")
        .selected_text(format!("{anchor}"))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut idx, 0, "TopRight");
            ui.selectable_value(&mut idx, 1, "TopLeft");
            ui.selectable_value(&mut idx, 2, "BottomLeft");
            ui.selectable_value(&mut idx, 3, "BottomRight");
            ui.selectable_value(&mut idx, 4, "Center");
        });
    if idx != prev {
        *anchor = match idx {
            0 => OddAnchor::TopRight,
            1 => OddAnchor::TopLeft,
            2 => OddAnchor::BottomLeft,
            3 => OddAnchor::BottomRight,
            _ => OddAnchor::Center,
        };
        true
    } else {
        false
    }
}

pub fn edit_piece(ui: &mut egui::Ui, piece: &mut Piece, effects: &mut DynamicEffects) -> bool {
let mut changed = false;
    match piece {
        Piece::Cross {
            origin, color, color_type, odd_anchor, lock_all, lock_gap, rotation, center_dot,
            left_gap, right_gap, top_gap, bottom_gap,
            left_thickness, right_thickness, top_thickness, bottom_thickness,
            left_length, right_length, top_length, bottom_length,
            ..
        } => {
            ui.label("Cross");
            changed |= edit_origin(ui, origin);
            changed |= slider_f64(ui, rotation, -360.0..=360.0, "Rotation");
            changed |= ui.checkbox(center_dot, "Center Dot").changed();

            // Lock All: main control - when checked, all 4 sides sync to one value
            if ui.checkbox(lock_all, "Lock All").changed() {
                if *lock_all {
                    *lock_gap = true; // auto-check Lock Axes too
                    *right_gap = *left_gap;
                    *top_gap = *left_gap;
                    *bottom_gap = *left_gap;
                    *right_thickness = *left_thickness;
                    *top_thickness = *left_thickness;
                    *bottom_thickness = *left_thickness;
                    *right_length = *left_length;
                    *top_length = *left_length;
                    *bottom_length = *left_length;
                    changed = true;
                }
            }

            // Lock Axes: only show when Lock All is unchecked - syncs H/V pairs
            if !*lock_all {
                if ui.checkbox(lock_gap, "Lock Axes").changed() {
                    if *lock_gap {
                        *right_gap = *left_gap;
                        *top_gap = *left_gap;
                        *bottom_gap = *left_gap;
                        *right_thickness = *left_thickness;
                        *top_thickness = *left_thickness;
                        *bottom_thickness = *left_thickness;
                        *right_length = *left_length;
                        *top_length = *left_length;
                        *bottom_length = *left_length;
                        changed = true;
                    }
                }
            }

            if *lock_all {
                // Lock All checked: show 1 slider
                ui.group(|ui| {
                    ui.label("All Sides:");
                    let gap_changed = slider_i32(ui, left_gap, -100..=100, "Gap");
                    if gap_changed {
                        *right_gap = *left_gap;
                        *top_gap = *left_gap;
                        *bottom_gap = *left_gap;
                    }
                    let thick_changed = slider_i32(ui, left_thickness, 1..=50, "Thickness");
                    if thick_changed {
                        *right_thickness = *left_thickness;
                        *top_thickness = *left_thickness;
                        *bottom_thickness = *left_thickness;
                    }
                    let len_changed = slider_i32(ui, left_length, -200..=200, "Length");
                    if len_changed {
                        *right_length = *left_length;
                        *top_length = *left_length;
                        *bottom_length = *left_length;
                    }
                    changed |= gap_changed || thick_changed || len_changed;
                });
            } else if *lock_gap {
                // Lock Axes checked: show 2 (H/V)
                ui.group(|ui| {
                    ui.label("Horizontal:");
                    let gap_changed = slider_i32(ui, left_gap, -100..=100, "Gap");
                    if gap_changed {
                        *right_gap = *left_gap;
                    }
                    let thick_changed = slider_i32(ui, left_thickness, 1..=50, "Thickness");
                    if thick_changed {
                        *right_thickness = *left_thickness;
                    }
                    let len_changed = slider_i32(ui, left_length, -200..=200, "Length");
                    if len_changed {
                        *right_length = *left_length;
                    }
                    changed |= gap_changed || thick_changed || len_changed;
                });
                ui.group(|ui| {
                    ui.label("Vertical:");
                    let gap_changed = slider_i32(ui, top_gap, -100..=100, "Gap");
                    if gap_changed {
                        *bottom_gap = *top_gap;
                    }
                    let thick_changed = slider_i32(ui, top_thickness, 1..=50, "Thickness");
                    if thick_changed {
                        *bottom_thickness = *top_thickness;
                    }
                    let len_changed = slider_i32(ui, top_length, -200..=200, "Length");
                    if len_changed {
                        *bottom_length = *top_length;
                    }
                    changed |= gap_changed || thick_changed || len_changed;
                });
            } else {
                // All 4 independent - show Left/Right/Top/Bottom groups
                ui.group(|ui| {
                    ui.label("Left:");
                    changed |= slider_i32(ui, left_gap, -100..=100, "Gap");
                    changed |= slider_i32(ui, left_thickness, 1..=50, "Thickness");
                    changed |= slider_i32(ui, left_length, -200..=200, "Length");
                });
                ui.group(|ui| {
                    ui.label("Right:");
                    changed |= slider_i32(ui, right_gap, -100..=100, "Gap");
                    changed |= slider_i32(ui, right_thickness, 1..=50, "Thickness");
                    changed |= slider_i32(ui, right_length, -200..=200, "Length");
                });
                ui.group(|ui| {
                    ui.label("Top:");
                    changed |= slider_i32(ui, top_gap, -100..=100, "Gap");
                    changed |= slider_i32(ui, top_thickness, 1..=50, "Thickness");
                    changed |= slider_i32(ui, top_length, -200..=200, "Length");
                });
                ui.group(|ui| {
                    ui.label("Bottom:");
                    changed |= slider_i32(ui, bottom_gap, -100..=100, "Gap");
                    changed |= slider_i32(ui, bottom_thickness, 1..=50, "Thickness");
                    changed |= slider_i32(ui, bottom_length, -200..=200, "Length");
                });
            }

            ui.separator();
            changed |= edit_color_section(ui, color, color_type, effects);
            ui.separator();
            changed |= edit_odd_anchor(ui, odd_anchor);
        }
        Piece::Dot { origin, size, color, color_type, odd_anchor, .. } => {
            ui.label("Dot");
            changed |= edit_origin(ui, origin);
            changed |= slider_u32(ui, size, 1..=100, "Size");
            ui.separator();
            changed |= edit_color_section(ui, color, color_type, effects);
            ui.separator();
            changed |= edit_odd_anchor(ui, odd_anchor);
        }
        Piece::Line { origin, vector, thickness, color, color_type, odd_anchor, anti_aliasing, .. } => {
            ui.label("Line");
            changed |= edit_origin(ui, origin);
            ui.horizontal(|ui| {
                ui.label("Vector");
                changed |= ui.add(egui::DragValue::new(&mut vector.0).speed(1.0)).changed();
                ui.label("Y");
                changed |= ui.add(egui::DragValue::new(&mut vector.1).speed(1.0)).changed();
            });
            changed |= slider_i32(ui, thickness, 1..=50, "Thickness");
            ui.checkbox(anti_aliasing, "Anti-aliasing");
            ui.separator();
            changed |= edit_color_section(ui, color, color_type, effects);
            ui.separator();
            changed |= edit_odd_anchor(ui, odd_anchor);
        }
        Piece::Rectangle { origin, width, height, rotation, color, color_type, odd_anchor, .. } => {
            ui.label("Rectangle");
            changed |= edit_origin(ui, origin);
            changed |= slider_u32(ui, width, 1..=500, "Width");
            changed |= slider_u32(ui, height, 1..=500, "Height");
            changed |= slider_f64(ui, rotation, -360.0..=360.0, "Rotation");
            ui.separator();
            changed |= edit_color_section(ui, color, color_type, effects);
            ui.separator();
            changed |= edit_odd_anchor(ui, odd_anchor);
        }
        Piece::RectPattern { origin, x_distance, x_quantity, y_distance, y_quantity, obj, .. } => {
            ui.label("RectPattern");
            changed |= edit_origin(ui, origin);
            changed |= slider_i32(ui, x_distance, -200..=200, "X Distance");
            changed |= slider_u32(ui, x_quantity, 1..=50, "X Quantity");
            changed |= slider_i32(ui, y_distance, -200..=200, "Y Distance");
            changed |= slider_u32(ui, y_quantity, 1..=50, "Y Quantity");
            ui.separator();
            ui.label("Inner piece:");
            changed |= edit_piece_type_selector(ui, obj);
            changed |= edit_piece(ui, obj, effects);
        }
        Piece::CircPattern { origin, radius, quantity, start_deg, obj, .. } => {
            ui.label("CircPattern");
            changed |= edit_origin(ui, origin);
            changed |= slider_i32(ui, radius, 1..=500, "Radius");
            changed |= slider_u32(ui, quantity, 1..=50, "Quantity");
            changed |= slider_f64(ui, start_deg, -360.0..=360.0, "Start Degrees");
            ui.separator();
            ui.label("Inner piece:");
            changed |= edit_piece_type_selector(ui, obj);
            changed |= edit_piece(ui, obj, effects);
        }
        Piece::HappyFace { origin, size, color, color_type, odd_anchor, .. } => {
            ui.label("HappyFace");
            changed |= edit_origin(ui, origin);
            changed |= slider_u32(ui, size, 1..=100, "Size");
            ui.separator();
            changed |= edit_color_section(ui, color, color_type, effects);
            ui.separator();
            changed |= edit_odd_anchor(ui, odd_anchor);
        }
    }
    changed
}

fn edit_piece_type_selector(ui: &mut egui::Ui, obj: &mut Box<Piece>) -> bool {
    let mut changed = false;
    let current = obj.type_name();
    let types = ["Dot", "Line", "Rectangle", "Cross", "HappyFace"];
    egui::ComboBox::from_label("Inner type")
        .selected_text(current)
        .show_ui(ui, |ui| {
            for t in &types {
                if ui.selectable_label(current == *t, *t).clicked() && current != *t {
                    *obj = Box::new(default_piece_of_type(t));
                    changed = true;
                }
            }
        });
    changed
}

fn default_piece_of_type(name: &str) -> Piece {
    use crate::types::ColorType;
    let default_color_type = ColorType::default();
    match name {
        "Dot" => Piece::Dot {
            origin: (0, 0), size: 2, color: "#ff5050ff".to_string(),
            color_type: default_color_type, visible: true, odd_anchor: OddAnchor::default(),
        },
        "Line" => Piece::Line {
            origin: (0, 0), vector: (10, 0), thickness: 2, color: "#ffffffff".to_string(),
            color_type: default_color_type, visible: true, odd_anchor: OddAnchor::default(),
            anti_aliasing: false,
        },
        "Rectangle" => Piece::Rectangle {
            origin: (0, 0), width: 10, height: 10, rotation: 0.0, color: "#ffffffff".to_string(),
            color_type: default_color_type, visible: true, odd_anchor: OddAnchor::default(),
        },
        "Cross" => Piece::Cross {
            origin: (0, 0),
            left_gap: 2, right_gap: 2, top_gap: 2, bottom_gap: 2,
            left_thickness: 2, right_thickness: 2, top_thickness: 2, bottom_thickness: 2,
            left_length: 4, right_length: 4, top_length: 4, bottom_length: 4,
            color: "#00ff7dff".to_string(), color_type: default_color_type,
            visible: true, odd_anchor: OddAnchor::default(), lock_gap: true, lock_all: true, rotation: 0.0, center_dot: false,
        },
        "HappyFace" => Piece::HappyFace {
            origin: (0, 0), size: 3, color: "#00ff7dff".to_string(),
            color_type: default_color_type, visible: true, odd_anchor: OddAnchor::default(),
        },
        _ => Piece::Dot {
            origin: (0, 0), size: 2, color: "#ffffffff".to_string(),
            color_type: default_color_type, visible: true, odd_anchor: OddAnchor::default(),
        },
    }
}

pub fn edit_color(ui: &mut egui::Ui, color: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Color");
        changed |= ui.text_edit_singleline(color).changed();
        let mut c = parse_color(color);
        if ui.color_edit_button_srgba(&mut c).changed() {
            let [r, g, b, a] = c.to_srgba_unmultiplied();
            *color = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
            changed = true;
        }
    });
    changed
}

use crate::types::ColorType;

/// Unified color section: color type dropdown, base color (when applicable), and sub-properties.
fn edit_color_section(ui: &mut egui::Ui, color: &mut String, color_type: &mut ColorType, effects: &mut DynamicEffects) -> bool {
    let mut changed = false;
    changed |= edit_color_type(ui, color_type, effects);
    if matches!(color_type, ColorType::Solid) {
        changed |= edit_color(ui, color);
    }
    changed
}

pub fn edit_color_type(ui: &mut egui::Ui, color_type: &mut ColorType, effects: &mut DynamicEffects) -> bool {
    let mut changed = false;

    let mut selected_idx = match *color_type {
        ColorType::Solid => 0,
        ColorType::Eraser => 1,
        ColorType::Rainbow { .. } => 2,
        ColorType::GradientCycle { .. } => 3,
        ColorType::Dynamic { .. } => 4,
    };

    ui.horizontal(|ui| {
        ui.label("Color Type");
        egui::ComboBox::from_id_salt("color_type_selector")
            .selected_text(match *color_type {
                ColorType::Solid => "Solid",
                ColorType::Eraser => "Eraser",
                ColorType::Rainbow { .. } => "Rainbow",
                ColorType::GradientCycle { .. } => "Gradient Cycle",
                ColorType::Dynamic { .. } => "Dynamic",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut selected_idx, 0, "Solid").clicked() {
                    *color_type = ColorType::Solid;
                    changed = true;
                }
                if ui.selectable_value(&mut selected_idx, 1, "Eraser").clicked() {
                    *color_type = ColorType::Eraser;
                    changed = true;
                }
                if ui.selectable_value(&mut selected_idx, 2, "Rainbow").clicked() {
                    *color_type = ColorType::Rainbow {
                        saturation: 1.0, lightness: 1.0, alpha: 1.0, speed: 1.0, reverse: false,
                    };
                    changed = true;
                }
                if ui.selectable_value(&mut selected_idx, 3, "Gradient Cycle").clicked() {
                    *color_type = ColorType::GradientCycle {
                        colors: vec!["#ff0000ff".to_string(), "#0000ffff".to_string()],
                        speed: 1.0,
                        loop_mode: crate::types::LoopMode::default(),
                        interpolation: crate::types::InterpolationMode::default(),
                        transition: None,
                        color2: None,
                    };
                    changed = true;
                }
                if ui.selectable_value(&mut selected_idx, 4, "Dynamic").clicked() {
                    *color_type = ColorType::Dynamic {
                        _legacy_mode: None,
                        _legacy_modes: None,
                        _legacy_tint: None,
                        _legacy_strength: None,
                    };
                    changed = true;
                }
            });
    });

    match color_type {
        ColorType::Rainbow { saturation, lightness, alpha, speed, reverse } => {
            changed |= slider_f64_fine(ui, saturation, 0.0..=1.0, "Saturation", 0.01);
            changed |= slider_f64_fine(ui, lightness, 0.0..=1.0, "Lightness", 0.01);
            changed |= slider_f64_fine(ui, alpha, 0.0..=1.0, "Transparency", 0.01);
            changed |= slider_f64_fine(ui, speed, 0.1..=10.0, "Speed", 0.1);
            if ui.checkbox(reverse, "Reverse").changed() {
                changed = true;
            }
        }
        ColorType::GradientCycle { colors, speed, loop_mode, interpolation, .. } => {
            changed |= slider_f64_fine(ui, speed, 0.1..=10.0, "Speed", 0.1);
            changed |= edit_loop_mode(ui, loop_mode);
            changed |= edit_interpolation_mode(ui, interpolation);
            ui.label("Gradient Colors:");
            let num_colors = colors.len();
            let mut remove_idx: Option<usize> = None;
            let mut swap: Option<(usize, usize)> = None;
            for i in 0..num_colors {
                let mut color_val = colors[i].clone();
                let mut removed = false;
                ui.horizontal(|ui| {
                    let up_enabled = i > 0;
                    let down_enabled = i < num_colors - 1;
                    if ui.add_enabled(up_enabled, egui::Button::new(
                        egui::RichText::new("▲").size(8.0)).min_size(egui::Vec2::new(14.0, 14.0)).frame(false)
                    ).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add_enabled(down_enabled, egui::Button::new(
                        egui::RichText::new("▼").size(8.0)).min_size(egui::Vec2::new(14.0, 14.0)).frame(false)
                    ).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.text_edit_singleline(&mut color_val).changed() {
                        changed = true;
                    }
                    let mut c = parse_color(&color_val);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        let [r, g, b, a] = c.to_srgba_unmultiplied();
                        color_val = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
                        changed = true;
                    }
                    if num_colors > 1 {
                        if ui.small_button("X").clicked() {
                            removed = true;
                        }
                    }
                });
                if removed {
                    remove_idx = Some(i);
                } else {
                    colors[i] = color_val;
                }
            }
            if let Some((a, b)) = swap {
                colors.swap(a, b);
                changed = true;
            }
            if let Some(idx) = remove_idx {
                colors.remove(idx);
                changed = true;
            }
            if ui.small_button("+ Add Color").clicked() {
                colors.push("#ffffffff".to_string());
                changed = true;
            }
        }
        ColorType::Dynamic { .. } => {
            ui.separator();
            ui.label("Dynamic Effects");
            ui.small("Effects applied in fixed order where this piece is drawn.");
            ui.separator();
            changed |= edit_dynamic_effects(ui, effects);
        }
        _ => {}
    }
    changed
}

fn edit_loop_mode(ui: &mut egui::Ui, loop_mode: &mut crate::types::LoopMode) -> bool {
    use crate::types::LoopMode;
    let mut changed = false;
    let mut idx = match *loop_mode { LoopMode::Bounce => 0, LoopMode::Cycle => 1, };
    let prev = idx;
    egui::ComboBox::from_id_salt("loop_mode")
        .selected_text(format!("{loop_mode}"))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut idx, 0, "Bounce");
            ui.selectable_value(&mut idx, 1, "Cycle");
        });
    if idx != prev {
        *loop_mode = match idx { 0 => LoopMode::Bounce, _ => LoopMode::Cycle, };
        changed = true;
    }
    changed
}

fn edit_interpolation_mode(ui: &mut egui::Ui, interpolation: &mut crate::types::InterpolationMode) -> bool {
    use crate::types::InterpolationMode;
    let mut changed = false;
    let mut idx = match *interpolation { InterpolationMode::Smooth => 0, InterpolationMode::Instant => 1, };
    let prev = idx;
    egui::ComboBox::from_id_salt("interpolation_mode")
        .selected_text(format!("{interpolation}"))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut idx, 0, "Smooth");
            ui.selectable_value(&mut idx, 1, "Instant Cuts");
        });
    if idx != prev {
        *interpolation = match idx { 0 => InterpolationMode::Smooth, _ => InterpolationMode::Instant, };
        changed = true;
    }
    changed
}

// ── Dynamic Effects Panel (project-level) ───────────────────────

/// Render the dynamic effects panel (fixed order).  Returns true if anything changed.
pub fn edit_dynamic_effects(ui: &mut egui::Ui, effects: &mut DynamicEffects) -> bool {
    let mut changed = false;

    // Overall opacity
    changed |= slider_f64_fine(ui, &mut effects.opacity, 0.0..=1.0, "Opacity", 0.01);
    ui.separator();

    // Invert
    if ui.checkbox(&mut effects.invert.enabled, "Invert").changed() { changed = true; }
    if effects.invert.enabled {
        ui.indent("eff_invert", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.invert.strength, 0.0..=1.0, "Strength", 0.01);
        });
    }

    // Dodge
    if ui.checkbox(&mut effects.dodge.enabled, "Dodge").changed() { changed = true; }
    if effects.dodge.enabled {
        ui.indent("eff_dodge", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.dodge.strength, 0.0..=1.0, "Strength", 0.01);
            changed |= edit_color(ui, &mut effects.dodge.tint);
        });
    }

    // Burn
    if ui.checkbox(&mut effects.burn.enabled, "Burn").changed() { changed = true; }
    if effects.burn.enabled {
        ui.indent("eff_burn", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.burn.strength, 0.0..=1.0, "Strength", 0.01);
            changed |= edit_color(ui, &mut effects.burn.tint);
        });
    }

    ui.separator();

    // Complement
    if ui.checkbox(&mut effects.complement.enabled, "Complement").changed() { changed = true; }
    if effects.complement.enabled {
        ui.indent("eff_complement", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.complement.strength, 0.0..=1.0, "Strength", 0.01);
        });
    }

    // Luma Invert
    if ui.checkbox(&mut effects.luma_invert.enabled, "Luma Invert").changed() { changed = true; }
    if effects.luma_invert.enabled {
        ui.indent("eff_lumainvert", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.luma_invert.strength, 0.0..=1.0, "Strength", 0.01);
        });
    }

    // Hue Rotate
    if ui.checkbox(&mut effects.hue_rotate.enabled, "Hue Rotate").changed() { changed = true; }
    if effects.hue_rotate.enabled {
        ui.indent("eff_huerotate", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.hue_rotate.strength, 0.0..=1.0, "Strength", 0.01);
            changed |= slider_f64_fine(ui, &mut effects.hue_rotate.angle, -360.0..=360.0, "Angle", 1.0);
        });
    }

    // Saturate
    if ui.checkbox(&mut effects.saturate.enabled, "Saturate").changed() { changed = true; }
    if effects.saturate.enabled {
        ui.indent("eff_saturate", |ui| {
            changed |= slider_f64_fine(ui, &mut effects.saturate.strength, 0.0..=1.0, "Strength", 0.01);
            changed |= slider_f64_fine(ui, &mut effects.saturate.amount, 0.0..=1.0, "Amount", 0.01);
        });
    }

    changed
}

pub fn parse_color(color: &str) -> egui::Color32 {
    if let Some(hex) = color.strip_prefix('#') {
        if hex.len() == 8 {
            if let Ok(r) = u8::from_str_radix(&hex[0..2], 16) {
                if let Ok(g) = u8::from_str_radix(&hex[2..4], 16) {
                    if let Ok(b) = u8::from_str_radix(&hex[4..6], 16) {
                        if let Ok(a) = u8::from_str_radix(&hex[6..8], 16) {
                            return egui::Color32::from_rgba_unmultiplied(r, g, b, a);
                        }
                    }
                }
            }
        } else if hex.len() == 6 {
            if let Ok(r) = u8::from_str_radix(&hex[0..2], 16) {
                if let Ok(g) = u8::from_str_radix(&hex[2..4], 16) {
                    if let Ok(b) = u8::from_str_radix(&hex[4..6], 16) {
                        return egui::Color32::from_rgb(r, g, b);
                    }
                }
            }
        }
    }
    egui::Color32::WHITE
}
