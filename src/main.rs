#![allow(clippy::too_many_lines, clippy::many_single_char_names)]

mod types;
mod links;
mod svg_rendering;
mod project_io;
mod preview;
mod ui_pieces;
mod ui_properties;
mod bg_embedded;

use std::path::PathBuf;

use eframe::egui;

use types::{AppConfig, CrosshairProject, Piece};
use preview::PreviewState;

struct CrosshairApp {
    pieces: Vec<Piece>,
    link_lib: links::LinkLibrary,
    selected_indices: Vec<usize>,
    status_message: String,
    project_name: String,
    current_file_path: Option<PathBuf>,
    config: AppConfig,
    preview: PreviewState,
    show_new_dialog: bool,
    show_save_as_dialog: bool,
    show_delete_confirm: bool,
    new_project_name: String,
    piece_thumbnails: preview::PieceThumbnailCache,
    recent_thumbnails: preview::RecentThumbnailCache,
    /// Whether the project has unsaved changes.
    has_unsaved_changes: bool,
    /// Snapshot of pieces at last save (for dirty tracking).
    last_saved_snapshot: String,
    /// Pending action that triggered unsaved-changes dialog.
    pending_unsaved_action: Option<UnsavedAction>,
    /// Confirmation dialog for removing a recent crosshair.
    show_remove_recent_confirm: Option<usize>,
    /// Error message from loading a project.
    load_error: Option<String>,
}

#[derive(Clone)]
enum UnsavedAction {
    OpenProject(PathBuf),
    NewProject(String),
    SetCurrent(PathBuf),
}

impl CrosshairApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set monospace as the default font for the entire app.
        let mut style = (*cc.egui_ctx.style()).clone();
        for text_style in [
            egui::TextStyle::Body,
            egui::TextStyle::Monospace,
            egui::TextStyle::Button,
            egui::TextStyle::Heading,
            egui::TextStyle::Small,
        ] {
            let size = style.text_styles[&text_style].size;
            style.text_styles.insert(
                text_style,
                egui::FontId::new(size, egui::FontFamily::Monospace),
            );
        }
        cc.egui_ctx.set_style(style);
        let config = project_io::load_config();
        Self {
            pieces: types::default_pieces(),
            link_lib: links::LinkLibrary::default(),
            selected_indices: Vec::new(),
            status_message: String::new(),
            project_name: "Untitled".to_string(),
            current_file_path: None,
            config,
            preview: PreviewState::new(),
            show_new_dialog: false,
            show_save_as_dialog: false,
            show_delete_confirm: false,
            new_project_name: String::new(),
            piece_thumbnails: preview::PieceThumbnailCache::new(),
            recent_thumbnails: preview::RecentThumbnailCache::new(),
            has_unsaved_changes: false,
            last_saved_snapshot: Self::take_snapshot(&types::default_pieces()),
            pending_unsaved_action: None,
            show_remove_recent_confirm: None,
            load_error: None,
        }
    }

    fn take_snapshot(pieces: &[types::Piece]) -> String {
        serde_json::to_string(pieces).unwrap_or_default()
    }

    /// Pieces with links resolved + the dynamic effect groups: what gets
    /// rendered and exported.
    fn resolved(&self) -> links::Resolved {
        links::resolve(&self.pieces, &self.link_lib)
    }

    fn take_snapshot_current(&self) -> String {
        Self::take_snapshot(&self.pieces)
    }

    fn mark_saved(&mut self) {
        self.last_saved_snapshot = self.take_snapshot_current();
        self.has_unsaved_changes = false;
    }

    fn check_unsaved(&mut self) {
        self.has_unsaved_changes = self.take_snapshot_current() != self.last_saved_snapshot;
    }

    fn update_piece_thumbnails(&mut self, ctx: &egui::Context) {
        let frame = self.preview.animation_frame();
        let resolved = self.resolved();
        self.piece_thumbnails.update(ctx, &resolved.pieces, frame);
    }

    fn load_recent_thumbnails(&mut self, ctx: &egui::Context) {
        // Keep the currently-open project's thumbnail in sync with live edits
        if let Some(ref path) = self.current_file_path {
            self.recent_thumbnails.set_live_pieces(path, &self.pieces);
        }
        let frame = self.preview.animation_frame();
        self.recent_thumbnails.update(ctx, &self.config.recent_crosshairs, frame);
    }

    fn invalidate_recent_thumbnail(&mut self, path: &PathBuf) {
        self.recent_thumbnails.invalidate(path);
    }

    fn save_with_exports(&self, path: &std::path::Path) {
        let r = self.resolved();
        preview::save_exports(path, &r.pieces, &r.groups);
        self.update_current_if_matches(path);
    }

    fn update_current_if_matches(&self, path: &std::path::Path) {
        if self.config.current_crosshair.as_ref().map(|p| p.as_path()) == Some(path) {
            let r = self.resolved();
            project_io::save_current_exports(&r.pieces, &r.groups);
        }
    }

    fn set_as_current(&mut self, path: PathBuf) {
        let r = self.resolved();
        project_io::save_current_exports(&r.pieces, &r.groups);
        self.config.current_crosshair = Some(path);
        project_io::save_config(&self.config);
    }

    fn remove_recent_crosshair(&mut self, idx: usize) {
        if let Some(path) = self.config.recent_crosshairs.get(idx).cloned() {
            self.config.recent_crosshairs.remove(idx);
            project_io::save_config(&self.config);
            self.recent_thumbnails.invalidate(&path);
            if let Some(stem) = path.file_stem() {
                let stem = stem.to_string_lossy();
                let parent = path.parent().unwrap_or(&path);
                let _ = std::fs::remove_file(path.clone());
                let _ = std::fs::remove_file(parent.join(format!("{}.svg", stem)));
                let _ = std::fs::remove_file(parent.join(format!("{}.png", stem)));
                let _ = std::fs::remove_file(parent.join(format!("{}.apng", stem)));
                let _ = std::fs::remove_file(parent.join(format!("{}.dynamic.png", stem)));
                let _ = std::fs::remove_file(parent.join(format!("{}.mask.png", stem)));
                let _ = std::fs::remove_file(parent.join(format!("{}.mask.apng", stem)));
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(&format!("{}.mask.", stem)) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
            self.status_message = "Crosshair removed".to_string();
        }
    }

    fn export_svg(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("crosshair.svg")
            .save_file()
        {
            let svg = self.preview.generate_svg_full(&self.pieces);
            if let Err(e) = std::fs::write(&path, &svg) {
                self.status_message = format!("Error: {e}");
            } else {
                self.status_message =
                    format!("Saved SVG to {}", path.display());
            }
        }
    }

    fn export_png(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("crosshair.png")
            .save_file()
        {
            let svg = self.preview.generate_svg_full(&self.pieces);
            let tree = resvg::usvg::Tree::from_str(
                &svg,
                &resvg::usvg::Options::default(),
            );
            match tree {
                Ok(tree) => {
                    let size = tree.size();
                    let mut pixmap =
                        resvg::tiny_skia::Pixmap::new(
                            size.width() as u32,
                            size.height() as u32,
                        )
                        .unwrap();
                    resvg::render(
                        &tree,
                        resvg::tiny_skia::Transform::default(),
                        &mut pixmap.as_mut(),
                    );
                    if let Err(e) = image::save_buffer(
                        &path,
                        pixmap.data(),
                        pixmap.width(),
                        pixmap.height(),
                        image::ColorType::Rgba8,
                    ) {
                        self.status_message = format!("Error: {e}");
                    } else {
                        self.status_message =
                            format!("Saved PNG to {}", path.display());
                    }
                }
                Err(e) => {
                    self.status_message = format!("SVG parse error: {e}");
                }
            }
        }
    }

    fn export_apng(&mut self) {
        use std::fs::File;
        use std::io::BufWriter;
        use crate::svg_rendering::{generate_svg, infer_extent};

        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("crosshair.apng")
            .save_file()
        {
            let (extent_x, extent_y) = infer_extent(&self.pieces);
            let w = (extent_x * 2).max(1) as u32;
            let h = (extent_y * 2).max(1) as u32;

            let has_animation = self.pieces.iter().any(|p| {
                matches!(
                    p.color_type(),
                    crate::types::ColorType::Rainbow { .. } | crate::types::ColorType::GradientCycle { .. }
                )
            });

            if !has_animation {
                self.status_message = "No animated colors to export as APNG".to_string();
                return;
            }

            // Compute cycle duration so the APNG covers one full loop
            let cycle_duration = crate::types::max_animation_cycle(&self.pieces);
            let export_fps: u32 = 30;
            let max_frames: u32 = 300;
            let num_frames: u32 = ((cycle_duration * export_fps as f64).round() as u32)
                .max(2)
                .min(max_frames);
            let frame_delay_secs = cycle_duration / num_frames as f64;
            let delay_den: u16 = 1000;
            let delay_num: u16 = ((frame_delay_secs * 1000.0).round() as u16).max(1);

            let mut frames: Vec<Vec<u8>> = Vec::new();
            for i in 0..num_frames {
                let frame_time = i as f64 * frame_delay_secs;
                let colored_pieces: Vec<crate::types::Piece> = self.pieces
                    .iter()
                    .map(|p| {
                        if !matches!(p.color_type(), crate::types::ColorType::Solid) {
                            let mut new_piece = p.clone();
                            let animated_color = p.get_animated_color(frame_time);
                            new_piece.set_color_override(&animated_color);
                            new_piece
                        } else {
                            p.clone()
                        }
                    })
                    .collect();

                let svg = generate_svg(w, h, &colored_pieces);
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

            if frames.is_empty() {
                self.status_message = "Failed to generate APNG frames".to_string();
                return;
            }

            let actual_frames = frames.len() as u32;
            if let Ok(file) = File::create(&path) {
                let mut buf_writer = BufWriter::new(file);
                let result = {
                    let mut encoder = png::Encoder::new(&mut buf_writer, w, h);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    encoder.set_animated(actual_frames, 0).unwrap();
                    encoder.set_frame_delay(delay_num, delay_den).unwrap();

                    match encoder.write_header() {
                        Ok(mut png_writer) => {
                            for frame_data in &frames {
                                let _ = png_writer.write_image_data(frame_data);
                            }
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                };
                match result {
                    Ok(()) => self.status_message = format!("Saved APNG to {}", path.display()),
                    Err(e) => self.status_message = format!("APNG encode error: {e}"),
                }
            } else {
                self.status_message = "Failed to create APNG file".to_string();
            }
        }
    }

    fn save_project(&mut self) {
        if let Some(ref path) = self.current_file_path.clone() {
            let project = CrosshairProject {
                name: self.project_name.clone(),
                pieces: self.pieces.clone(),
                odd_anchor: None,
                dynamic_groups: Vec::new(),
                dynamic_effects: None,
            };
            if project_io::save_project(&project, &mut self.config, Some(path.clone())).is_some() {
                self.save_with_exports(path);
                self.invalidate_recent_thumbnail(path);
                self.mark_saved();
                self.status_message = format!("Saved to {}", path.display());
            } else {
                self.status_message = "Failed to save project".to_string();
            }
        } else {
            self.show_save_as_dialog = true;
        }
    }

    fn save_project_as(&mut self) {
        self.show_save_as_dialog = true;
    }

    fn do_save_as(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        let project = CrosshairProject {
            name: name.to_string(),
            pieces: self.pieces.clone(),
            odd_anchor: None,
            dynamic_groups: Vec::new(),
            dynamic_effects: None,
        };
        self.project_name = name.to_string();
        if let Some(path) = project_io::save_project(&project, &mut self.config, None) {
            self.save_with_exports(&path);
            self.invalidate_recent_thumbnail(&path);
            self.current_file_path = Some(path.clone());
            self.mark_saved();
            self.status_message = format!("Saved as '{}' to {}", name, path.display());
        } else {
            self.status_message = "Failed to save project".to_string();
        }
        self.show_save_as_dialog = false;
    }

    fn new_project(&mut self, name: &str) {
        self.pieces = types::default_pieces();
        self.project_name = name.to_string();
        self.current_file_path = None;
        self.selected_indices.clear();
        self.preview.mark_dirty();
        self.piece_thumbnails = preview::PieceThumbnailCache::new();
        self.show_new_dialog = false;
        self.mark_saved();
        self.status_message = format!("New project '{}'", name);
    }

    fn open_project(&mut self, path: PathBuf) {
        match project_io::load_project(&path) {
            Ok(project) => {
                let name = project.name.clone();
                self.pieces = project.pieces;
                self.project_name = name.clone();
                self.current_file_path = Some(path.clone());
                self.selected_indices.clear();
                self.preview.mark_dirty();
                self.piece_thumbnails = preview::PieceThumbnailCache::new();
                project_io::add_to_recent(&mut self.config, path.clone());
                self.mark_saved();
                self.status_message = format!("Opened '{}'", name);
            }
            Err(e) => {
                self.status_message = format!("Failed to open {}", path.display());
                self.load_error = Some(e);
            }
        }
    }
}

impl eframe::App for CrosshairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── keyboard shortcuts ──────────────────────────────────
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save_project();
        }

        // piece ids (links point at them) + the saved-crosshair library
        if links::assign_uids(&mut self.pieces) {
            self.preview.mark_dirty();
        }
        self.link_lib.refresh_if_stale();

        // ── thumbnail generation ────────────────────────────────
        self.update_piece_thumbnails(ctx);
        self.load_recent_thumbnails(ctx);

        let mut open_path: Option<PathBuf> = None;
        let mut do_save = false;
        let mut do_save_as = false;
        let mut do_export_svg = false;
        let mut do_export_png = false;
        let mut do_export_apng = false;
        let mut do_set_current: Option<PathBuf> = None;

        self.check_unsaved();

        egui::SidePanel::left("pieces_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                let mut mark_dirty = false;
                let current_ch = self.config.current_crosshair.clone();
                ui_pieces::render_pieces_panel(
                    ui,
                    ctx,
                    &mut self.pieces,
                    &mut self.selected_indices,
                    &mut self.status_message,
                    &self.project_name,
                    &self.current_file_path,
                    &current_ch,
                    &mut self.config,
                    &mut self.show_new_dialog,
                    &mut self.show_save_as_dialog,
                    &mut self.new_project_name,
                    &self.piece_thumbnails,
                    &self.recent_thumbnails,
                    |path| open_path = Some(path),
                    || do_save = true,
                    || do_save_as = true,
                    || do_export_svg = true,
                    || do_export_png = true,
                    || do_export_apng = true,
                    || self.show_delete_confirm = true,
                    |path| do_set_current = Some(path),
                    |idx| self.show_remove_recent_confirm = Some(idx),
                );
                if !self.selected_indices.is_empty() {
                    mark_dirty = true;
                }
                if mark_dirty {
                    self.preview.mark_dirty();
                }
            });

        if let Some(path) = open_path {
            // Skip save prompt if default project with no changes
            let is_default_project = self.current_file_path.is_none();
            let should_skip_prompt = is_default_project && !self.has_unsaved_changes;
            
            if self.has_unsaved_changes && !should_skip_prompt {
                self.pending_unsaved_action = Some(UnsavedAction::OpenProject(path));
            } else {
                self.open_project(path);
            }
        }
        if do_save {
            self.save_project();
        }
        if do_save_as {
            self.save_project_as();
        }
        if do_export_svg {
            self.export_svg();
        }
        if do_export_png {
            self.export_png();
        }
        if do_export_apng {
            self.export_apng();
        }
        if let Some(path) = do_set_current {
            // Skip save prompt if:
            // 1. No file path loaded yet (default project), AND
            // 2. No actual changes made from default
            let is_default_project = self.current_file_path.is_none();
            let should_skip_prompt = is_default_project && !self.has_unsaved_changes;
            
            if self.has_unsaved_changes && !should_skip_prompt {
                self.pending_unsaved_action = Some(UnsavedAction::SetCurrent(path));
            } else {
                self.open_project(path.clone());
                self.set_as_current(path);
                self.save_project();
            }
        }

        egui::SidePanel::right("properties_panel")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Properties");
                    ui.separator();

                    if self.selected_indices.len() == 1 {
                        let idx = self.selected_indices[0];
                        let choices = ui_properties::link_choices(
                            &self.pieces,
                            idx,
                            &self.link_lib,
                            self.current_file_path.as_deref().and_then(links::stem_of).as_deref(),
                        );
                        if let Some(piece) = self.pieces.get_mut(idx) {
                            if ui_properties::edit_piece(ui, piece, &choices) {
                                self.preview.mark_dirty();
                                self.check_unsaved();
                            }
                        }
                    } else if self.selected_indices.len() > 1 {
                        ui.label(format!("{} pieces selected", self.selected_indices.len()));
                        ui.separator();
                        ui.label("Hidden Properties (Multi-Select)");
                        ui.small("Edit properties for each selected piece individually.");
                    } else {
                        ui.label("Select a piece to edit");
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Preview");
            ui.separator();

            let resolved = links::resolve(&self.pieces, &self.link_lib);
            preview::render_preview_panel(ui, ctx, &mut self.preview, &resolved.pieces, &resolved.groups);
        });

        if self.show_new_dialog {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("New Project")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Project name:");
                    ui.text_edit_singleline(&mut self.new_project_name);
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            create = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                self.show_new_dialog = false;
            } else if create && !self.new_project_name.is_empty() {
                let name = self.new_project_name.clone();
                if name.eq_ignore_ascii_case("current") {
                    self.status_message = "Cannot use name 'current' — reserved".to_string();
                } else {
                    self.show_new_dialog = false;
                    // Skip save prompt if default project with no changes
                    let is_default_project = self.current_file_path.is_none();
                    let should_skip_prompt = is_default_project && !self.has_unsaved_changes;
                    
                    if self.has_unsaved_changes && !should_skip_prompt {
                        self.pending_unsaved_action = Some(UnsavedAction::NewProject(name));
                    } else {
                        self.new_project(&name);
                    }
                }
            }
        }

        if self.show_save_as_dialog {
            let mut save = false;
            let mut cancel = false;
            egui::Window::new("Save As")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Project name:");
                    ui.text_edit_singleline(&mut self.new_project_name);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                self.show_save_as_dialog = false;
            } else if save && !self.new_project_name.is_empty() {
                let name = self.new_project_name.clone();
                if name.eq_ignore_ascii_case("current") {
                    self.status_message = "Cannot use name 'current' — reserved".to_string();
                } else {
                    self.do_save_as(&name);
                }
            }
        }

        if self.show_delete_confirm {
            let mut do_delete = false;
            let mut cancel_delete = false;
            egui::Window::new("Confirm Delete")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::LEFT_TOP, [10.0, 50.0])
                .show(ctx, |ui| {
                    ui.label("Are you sure you want to remove the selected piece?");
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            do_delete = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_delete = true;
                        }
                    });
                });
            if cancel_delete {
                self.show_delete_confirm = false;
            } else if do_delete {
                let mut indices: Vec<usize> = self.selected_indices.clone();
                indices.sort_unstable();
                indices.reverse();
                for idx in indices {
                    if idx < self.pieces.len() {
                        self.pieces.remove(idx);
                    }
                }
                self.selected_indices.clear();
                self.preview.mark_dirty();
                self.check_unsaved();
                self.show_delete_confirm = false;
            }
        }

        if let Some(ref action) = self.pending_unsaved_action {
            let action = action.clone();
            let mut proceed = false;
            let mut discard = false;
            let mut cancel = false;
            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Do you want to save before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save & Continue").clicked() {
                            proceed = true;
                        }
                        if ui.button("Don't Save").clicked() {
                            discard = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                self.pending_unsaved_action = None;
            } else if discard {
                match action {
                    UnsavedAction::OpenProject(path) => {
                        self.open_project(path);
                    }
                    UnsavedAction::NewProject(name) => {
                        self.new_project(&name);
                    }
                    UnsavedAction::SetCurrent(path) => {
                        self.open_project(path.clone());
                        self.set_as_current(path);
                        self.save_project();
                    }
                }
                self.pending_unsaved_action = None;
            } else if proceed {
                self.save_project();
                match action {
                    UnsavedAction::OpenProject(path) => {
                        self.open_project(path);
                    }
                    UnsavedAction::NewProject(name) => {
                        self.new_project(&name);
                    }
                    UnsavedAction::SetCurrent(path) => {
                        self.open_project(path.clone());
                        self.set_as_current(path);
                        self.save_project();
                    }
                }
                self.pending_unsaved_action = None;
            }
        }

        if let Some(idx) = self.show_remove_recent_confirm {
            let mut remove = false;
            let mut cancel = false;
            egui::Window::new("Remove Crosshair")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::LEFT_TOP, [10.0, 50.0])
                .show(ctx, |ui| {
                    ui.label("Remove this crosshair from recent list?");
                    ui.label("This will also delete the project files (JSON, SVG, PNG, APNG, and dynamic files).");
                    ui.horizontal(|ui| {
                        if ui.button("Remove").clicked() {
                            remove = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                self.show_remove_recent_confirm = None;
            } else if remove {
                self.remove_recent_crosshair(idx);
                self.show_remove_recent_confirm = None;
            }
        }

        if let Some(ref error) = self.load_error {
            let mut close = false;
            egui::Window::new("Failed to Load Project")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Could not load the project file:");
                    ui.separator();
                    let file_info = if let Some(ref path) = self.current_file_path.as_ref().or_else(|| {
                        self.config.recent_crosshairs.first()
                    }) {
                        format!("File: {}", path.display())
                    } else {
                        String::new()
                    };
                    if !file_info.is_empty() {
                        ui.label(&file_info);
                    }
                    ui.label(egui::RichText::new(error).color(egui::Color32::RED));
                    ui.separator();
                    ui.label("This may be due to an incompatible file format from an older version.");
                    ui.label("The file may need to be re-saved to update its format.");
                    ui.horizontal(|ui| {
                        let full_error = format!(
                            "Failed to Load Project\n{}\n{}\n\n{}\n{}\n",
                            if !file_info.is_empty() { &file_info } else { "No file path" },
                            error,
                            "This may be due to an incompatible file format from an older version.",
                            "The file may need to be re-saved to update its format."
                        );
                        if ui.button("Copy Error").clicked() {
                            ui.ctx().copy_text(full_error);
                        }
                        if ui.button("OK").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.load_error = None;
            }
        }
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Crosshair Maker"),
        ..Default::default()
    };

    eframe::run_native(
        "Crosshair Maker",
        native_options,
        Box::new(|cc| Ok(Box::new(CrosshairApp::new(cc)))),
    )
}
