use std::path::PathBuf;

use eframe::egui;
use egui::{Id, NumExt};

const THUMB_DISPLAY: f32 = 20.0;

#[allow(clippy::too_many_arguments)]
pub fn render_pieces_panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    pieces: &mut Vec<crate::types::Piece>,
    selected_indices: &mut Vec<usize>,
    status_message: &mut String,
    project_name: &str,
    current_file_path: &Option<PathBuf>,
    current_crosshair_path: &Option<PathBuf>,
    config: &mut crate::types::AppConfig,
    show_new_dialog: &mut bool,
    _show_save_as_dialog: &mut bool,
    new_project_name: &mut String,
    piece_thumbnails: &crate::preview::PieceThumbnailCache,
    recent_thumbnails: &crate::preview::RecentThumbnailCache,
    mut on_open: impl FnMut(PathBuf),
    mut on_save: impl FnMut(),
    mut on_save_as: impl FnMut(),
    mut on_export_svg: impl FnMut(),
    mut on_export_png: impl FnMut(),
    mut on_export_apng: impl FnMut(),
    mut on_request_delete: impl FnMut(),
    mut on_set_current: impl FnMut(PathBuf),
    mut on_remove_recent: impl FnMut(usize),
) {
    ui.heading("Crosshair Maker");
    ui.separator();

    ui.horizontal(|ui| {
        if ui.button("New").clicked() {
            *show_new_dialog = true;
            *new_project_name = String::new();
        }
        if ui.button("Open").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Crosshair Project", &["json"])
                .pick_file()
            {
                on_open(path);
            }
        }
    });

    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            on_save();
        }
        if ui.button("Save As...").clicked() {
            on_save_as();
            *new_project_name = project_name.to_string();
        }
    });

    ui.horizontal(|ui| {
        if ui.button("Export SVG").clicked() {
            on_export_svg();
        }
        if ui.button("Export PNG").clicked() {
            on_export_png();
        }
        if ui.button("Export APNG").clicked() {
            on_export_apng();
        }
    });

    if !status_message.is_empty() {
        ui.small(status_message);
    }

    ui.separator();

    let current_name = if let Some(ref path) = current_file_path {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| project_name.to_string())
    } else {
        project_name.to_string()
    };
    ui.strong(format!("Current: {}", current_name));

    ui.separator();
    ui.heading("Recent Crosshairs");

    let recent_count = config.recent_crosshairs.len();
    if recent_count == 0 {
        ui.small("No recent crosshairs");
    } else {
        egui::ScrollArea::vertical()
            .id_salt("recent_crosshairs")
            .max_height(150.0)
            .show(ui, |ui| {
                let mut clicked_path: Option<PathBuf> = None;
                let mut remove_idx: Option<usize> = None;
                for i in 0..recent_count {
                    let path = &config.recent_crosshairs[i];
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let _is_current = current_file_path.as_ref() == Some(path);
                    let is_active = current_crosshair_path.as_ref() == Some(path);

                    ui.horizontal(|ui| {
                        // Current radio button (can only set, not unset)
                        let radio_text = if is_active {
                            egui::RichText::new("◉").size(16.0)
                        } else {
                            egui::RichText::new("○").size(16.0)
                        };
                        if ui.add(egui::Button::new(radio_text).frame(false))
                            .on_hover_text("Set as current crosshair")
                            .clicked()
                        {
                            if !is_active {
                                on_set_current(path.clone());
                            }
                        }

                        // Thumbnail
                        if let Some(tex) = recent_thumbnails.get(path) {
                            let tex_size = tex.size();
                            let aspect = tex_size[0] as f32 / tex_size[1].max(1) as f32;
                            let w = THUMB_DISPLAY * aspect.min(1.0);
                            let h = THUMB_DISPLAY / aspect.max(1.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::Vec2::new(THUMB_DISPLAY, THUMB_DISPLAY),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(30));
                            let img_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::new(w, h));
                            ui.painter().image(
                                tex.id(),
                                img_rect,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::Vec2::new(THUMB_DISPLAY, THUMB_DISPLAY),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(30));
                        }

                        let name_label = egui::RichText::new(&name);
                        if ui.add(egui::Button::new(name_label).frame(false)).clicked() {
                            clicked_path = Some(path.clone());
                        }
                        if ui.small_button("X").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                }
                if let Some(p) = clicked_path {
                    on_open(p);
                }
                if let Some(idx) = remove_idx {
                    on_remove_recent(idx);
                }
            });
    }

    ui.separator();
    ui.heading("Pieces");

    egui::ScrollArea::vertical().id_salt("pieces_list").show(ui, |ui| {
        let mut reorder: Option<(usize, usize)> = None;
        let mut swap: Option<(usize, usize)> = None;

        for i in (0..pieces.len()).rev() {
            let piece = &pieces[i];
            let selected = selected_indices.contains(&i);
            let type_name = piece.type_name();
            let visible = piece.is_visible();

            let item_id = Id::new(("piece_item", i));
            let is_being_dragged = ctx.is_being_dragged(item_id);

            let (_drop_inner, dropped_payload) = ui.dnd_drop_zone::<usize, _>(
                egui::Frame::NONE,
                |ui| {
                    if is_being_dragged {
                        egui::DragAndDrop::set_payload(ui.ctx(), i);
                        let layer_id = egui::LayerId::new(egui::Order::Tooltip, item_id);
                        let resp = ui.scope_builder(
                            egui::UiBuilder::new().layer_id(layer_id),
                            |ui| {
                                ui.horizontal(|ui| {
                                    let _ = ui.selectable_label(selected, type_name);
                                });
                            },
                        );
                        if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
                            let delta = pointer_pos - resp.response.rect.center();
                            ui.ctx().transform_layer_shapes(
                                layer_id,
                                egui::emath::TSTransform::from_translation(
                                    egui::emath::Vec2::new(delta.x, delta.y),
                                ),
                            );
                        }
                    } else {
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                let can_move_up = i < pieces.len() - 1;
                                let can_move_down = i > 0;

                                let arrow_style = egui::RichText::new("▲").size(8.0);
                                let up_btn = egui::Button::new(arrow_style)
                                    .min_size(egui::Vec2::new(14.0, 14.0))
                                    .frame(false);
                                if ui.add_enabled(can_move_up, up_btn).clicked() {
                                    swap = Some((i, i + 1));
                                }

                                let down_btn = egui::Button::new(egui::RichText::new("▼").size(8.0))
                                    .min_size(egui::Vec2::new(14.0, 14.0))
                                    .frame(false);
                                if ui.add_enabled(can_move_down, down_btn).clicked() {
                                    swap = Some((i, i - 1));
                                }

                                let eye_icon = if visible {
                                    egui::RichText::new("👁").size(12.0)
                                } else {
                                    egui::RichText::new("🚫").size(12.0)
                                };
                                let vis_btn = egui::Button::new(eye_icon)
                                    .min_size(egui::Vec2::new(18.0, 18.0))
                                    .frame(false);
                                if ui.add(vis_btn).clicked() {
                                    pieces[i].set_visible(!visible);
                                }

                                // Piece thumbnail
                                if let Some(tex) = piece_thumbnails.get(i) {
                                    let tex_size = tex.size();
                                    let aspect = tex_size[0] as f32 / tex_size[1].max(1) as f32;
                                    let w = THUMB_DISPLAY * aspect.min(1.0);
                                    let h = THUMB_DISPLAY / aspect.max(1.0);
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::Vec2::new(THUMB_DISPLAY, THUMB_DISPLAY),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(30));
                                    let img_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::new(w, h));
                                    ui.painter().image(
                                        tex.id(),
                                        img_rect,
                                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::Vec2::new(THUMB_DISPLAY, THUMB_DISPLAY),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(30));
                                }

                                let galley = ui.fonts(|f| f.layout_no_wrap(
                                    type_name.to_string(),
                                    egui::FontId::default(),
                                    egui::Color32::PLACEHOLDER,
                                ));
                                let mut desired_size = galley.size()
                                    + ui.spacing().button_padding * 2.0;
                                desired_size.y = desired_size.y.at_least(ui.spacing().interact_size.y);
                                let (rect, resp) = ui.allocate_at_least(desired_size, egui::Sense::click_and_drag());

                                if resp.clicked() {
                                    if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                                        if selected {
                                            selected_indices.retain(|&x| x != i);
                                        } else {
                                            selected_indices.push(i);
                                        }
                                    } else {
                                        selected_indices.clear();
                                        selected_indices.push(i);
                                    }
                                }
                                if resp.drag_started() {
                                    ui.ctx().set_dragged_id(item_id);
                                }

                                if ui.is_rect_visible(rect) {
                                    let text_color = if !visible {
                                        egui::Color32::DARK_GRAY
                                    } else if selected {
                                        ui.visuals().widgets.inactive.text_color()
                                    } else {
                                        ui.visuals().widgets.inactive.text_color()
                                    };
                                    if selected {
                                        let visuals = ui.style().interact_selectable(&resp, true);
                                        let rect = rect.expand(visuals.expansion);
                                        ui.painter().rect(
                                            rect,
                                            visuals.corner_radius,
                                            visuals.weak_bg_fill,
                                            visuals.bg_stroke,
                                            egui::epaint::StrokeKind::Inside,
                                        );
                                    }
                                    let text_pos = ui.layout()
                                        .align_size_within_rect(galley.size(), rect.shrink2(ui.spacing().button_padding))
                                        .min;
                                    ui.painter().galley(text_pos, galley, text_color);
                                }
                            });
                        });
                    }
                },
            );

            if let Some(dropped_index) = dropped_payload {
                reorder = Some((*dropped_index, i));
            }
        }

        if let Some((a, b)) = swap {
            pieces.swap(a, b);
            for sel in selected_indices.iter_mut() {
                if *sel == a {
                    *sel = b;
                } else if *sel == b {
                    *sel = a;
                }
            }
        }

        if let Some((dragged_idx, target_idx)) = reorder {
            if dragged_idx != target_idx {
                let piece = pieces.remove(dragged_idx);
                pieces.insert(target_idx, piece);

                let mut new_indices = Vec::new();
                for sel in selected_indices.iter() {
                    if *sel == dragged_idx {
                        new_indices.push(target_idx);
                    } else if dragged_idx < target_idx {
                        if *sel > dragged_idx && *sel <= target_idx {
                            new_indices.push(sel - 1);
                        } else {
                            new_indices.push(*sel);
                        }
                    } else {
                        if *sel >= target_idx && *sel < dragged_idx {
                            new_indices.push(sel + 1);
                        } else {
                            new_indices.push(*sel);
                        }
                    }
                }
                *selected_indices = new_indices;
            }
        }
    });

    ui.separator();
    let remove_label = if selected_indices.len() > 1 {
        format!("Remove Selected ({})", selected_indices.len())
    } else {
        "Remove Selected".to_string()
    };
    if ui.button(&remove_label).clicked() {
        if !selected_indices.is_empty() {
            on_request_delete();
        }
    }

    ui.separator();
    ui.heading("Add Piece");
    let da = crate::types::OddAnchor::default();
    let dc = crate::types::ColorType::default();
    if ui.button("Cross").clicked() {
        pieces.push(crate::types::Piece::Cross {
            uid: 0,
            origin: (0, 0),
            left_gap: 2, right_gap: 2, top_gap: 2, bottom_gap: 2,
            left_thickness: 2, right_thickness: 2, top_thickness: 2, bottom_thickness: 2,
            left_length: 4, right_length: 4, top_length: 4, bottom_length: 4,
            color: "#00ff7dff".to_string(),
            color_type: dc.clone(),
            visible: true,
            odd_anchor: da,
            lock_gap: true,
            lock_all: true,
            rotation: 0.0,
            center_dot: false,
            hard_edges: false,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("Dot").clicked() {
        pieces.push(crate::types::Piece::Dot {
            uid: 0,
            origin: (0, 0),
            size: 2,
            color: "#ff5050ff".to_string(),
            color_type: dc.clone(),
            visible: true,
            odd_anchor: da,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("Line").clicked() {
        pieces.push(crate::types::Piece::Line {
            uid: 0,
            origin: (0, 0),
            vector: (10, 0),
            thickness: 2,
            color: "#ffffffff".to_string(),
            color_type: dc.clone(),
            visible: true,
            odd_anchor: da,
            anti_aliasing: false,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("Rectangle").clicked() {
        pieces.push(crate::types::Piece::Rectangle {
            uid: 0,
            origin: (0, 0),
            width: 10,
            height: 10,
            rotation: 0.0,
            color: "#ffffffff".to_string(),
            color_type: dc.clone(),
            visible: true,
            odd_anchor: da,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("HappyFace").clicked() {
        pieces.push(crate::types::Piece::HappyFace {
            uid: 0,
            origin: (0, 0),
            size: 3,
            color: "#00ff7dff".to_string(),
            color_type: dc.clone(),
            visible: true,
            odd_anchor: da,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("RectPattern").clicked() {
        pieces.push(crate::types::Piece::RectPattern {
            origin: (0, 0),
            x_distance: 10,
            x_quantity: 3,
            y_distance: 10,
            y_quantity: 3,
            obj: Box::new(crate::types::Piece::Dot {
                uid: 0,
                origin: (0, 0),
                size: 2,
                color: "#ff5050ff".to_string(),
                color_type: dc.clone(),
                visible: true,
                odd_anchor: da,
            }),
            visible: true,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
    if ui.button("CircPattern").clicked() {
        pieces.push(crate::types::Piece::CircPattern {
            origin: (0, 0),
            radius: 20,
            quantity: 8,
            start_deg: 0.0,
            obj: Box::new(crate::types::Piece::Dot {
                uid: 0,
                origin: (0, 0),
                size: 2,
                color: "#5050ffff".to_string(),
                color_type: dc.clone(),
                visible: true,
                odd_anchor: da,
            }),
            visible: true,
        });
        selected_indices.clear();
        selected_indices.push(pieces.len() - 1);
    }
}
