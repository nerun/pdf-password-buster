// PDF Password Buster v1.0 - main window, password prompt, About and Help.
// Copyright 2026 Daniel Dias Rodrigues
//
// Licensed under the MIT License. You may use, copy, modify, merge, publish,
// distribute, sublicense and/or sell copies of this software under the terms
// of that license; see the LICENSE file distributed with this program.
// SPDX-License-Identifier: MIT

//! Main window: the result list, the four buttons, the progress bar,
//! the password prompt and the context menu.

use crate::engine::{self, Open};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const TITLE: &str = "PDF Password Buster v1.0";
pub const APP_NAME: &str = "PDF Password Buster";
const COPYRIGHT: &str = "Copyright 2026 Daniel Dias Rodrigues";

// Messages shown in the list (same wording as the original program).
const RESULT_ERROR: &str = "ERROR";
const RESULT_SUCCEED: &str = "SUCCEED";
const RESULT_FAILED: &str = "FAILED";
const EV_NOT_ENCRYPTED: &str = "File is not encrypted.";
const EV_REMOVED: &str = "Password Removed.";
const EV_BAD_PASSWORD: &str = "Password is not correct.";

// Classic desktop palette.
const FACE: Color32 = Color32::from_rgb(0xD4, 0xD0, 0xC8);
const FACE_LIGHT: Color32 = Color32::from_rgb(0xE8, 0xE6, 0xE1);
const SHADOW: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
const SELECT: Color32 = Color32::from_rgb(0x31, 0x6A, 0xC5);
const INK: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
/// Grid line colour of the original list view (sampled from its manual: #ECE9D8).
const GRID: Color32 = Color32::from_rgb(0xEC, 0xE9, 0xD8);
const ROW_H: f32 = 18.0;

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Error,
    Succeed,
    Failed,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Error => RESULT_ERROR,
            Status::Succeed => RESULT_SUCCEED,
            Status::Failed => RESULT_FAILED,
        }
    }
}

struct Row {
    status: Status,
    event: String,
    source: PathBuf,
    target: Option<PathBuf>,
    selected: bool,
}

/// Files waiting to be processed, one per frame, like the original's
/// message-pumping loop.
struct Batch {
    files: Vec<PathBuf>,
    next: usize,
    out_dir: Option<PathBuf>,
}

/// A file that needs a password typed by the person.
struct Prompt {
    source: PathBuf,
    target: PathBuf,
    user: String,
    owner: String,
    focus_done: bool,
}

struct Screenshot {
    path: PathBuf,
    frames: u32,
    requested: bool,
    image: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
}

pub struct RemoverApp {
    /// Program icon shown in the About box.
    about_icon: Option<egui::TextureHandle>,
    rows: Vec<Row>,
    batch: Option<Batch>,
    prompt: Option<Prompt>,
    progress: f32,
    /// Last passwords typed in the prompt; tried first on the next file
    /// (kept for this session only).
    remembered_owner: String,
    remembered_user: String,
    /// Ask for a password when a file needs one (original default: on).
    show_password_prompt: bool,
    show_about: bool,
    show_help: bool,
    anchor: Option<usize>,
    scroll_to_last: bool,
    screenshot: Option<Screenshot>,
}

impl RemoverApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        screenshot: Option<PathBuf>,
        files: Vec<PathBuf>,
        out_dir: Option<PathBuf>,
    ) -> Self {
        apply_style(&cc.egui_ctx);
        let about_icon = crate::decode_png(crate::ICON_64).map(|(w, h, rgba)| {
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            cc.egui_ctx.load_texture("about_icon", image, egui::TextureOptions::LINEAR)
        });
        let mut app = Self {
            about_icon,
            rows: Vec::new(),
            batch: None,
            prompt: None,
            progress: 0.0,
            remembered_owner: String::new(),
            remembered_user: String::new(),
            show_password_prompt: true,
            show_about: false,
            show_help: false,
            anchor: None,
            scroll_to_last: false,
            screenshot: screenshot.map(|path| Screenshot {
                path,
                frames: 0,
                requested: false,
                image: Arc::new(Mutex::new(None)),
            }),
        };
        if !files.is_empty() {
            app.start_with(files, out_dir);
        }
        app
    }

    // ----- batch processing -------------------------------------------------

    fn start(&mut self, files: Vec<PathBuf>) {
        self.start_with(files, None);
    }

    /// Starts a batch; `out_dir` given up front skips the folder question.
    fn start_with(&mut self, files: Vec<PathBuf>, mut out_dir: Option<PathBuf>) {
        if files.is_empty() || self.batch.is_some() {
            return;
        }
        let files: Vec<PathBuf> = files.into_iter().map(|f| std::path::absolute(&f).unwrap_or(f)).collect();
        if files.len() > 1 && out_dir.is_none() {
            // Several files: ask once for the output folder; names are kept.
            let start_dir = files[0].parent().map(Path::to_path_buf);
            let mut dlg = rfd::FileDialog::new().set_title("Please select an output directory ...");
            if let Some(d) = start_dir {
                dlg = dlg.set_directory(d);
            }
            match dlg.pick_folder() {
                Some(d) => out_dir = Some(d),
                None => return,
            }
        }
        self.progress = 0.0;
        self.batch = Some(Batch { files, next: 0, out_dir });
    }

    /// Where the decrypted copy of `source` goes. One file: ask with a save
    /// dialog. Several files: the chosen folder with the same file name.
    fn target_for(&self, source: &Path) -> Option<PathBuf> {
        let batch = self.batch.as_ref()?;
        if let Some(dir) = &batch.out_dir {
            return Some(dir.join(source.file_name()?));
        }
        let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut dlg = rfd::FileDialog::new()
            .set_title("Save As")
            .add_filter("PDF File(*.pdf)", &["pdf", "PDF"])
            .set_file_name(format!("{stem}_decrypted.pdf"));
        if let Some(d) = source.parent() {
            dlg = dlg.set_directory(d);
        }
        let mut path = dlg.save_file()?;
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        Some(path)
    }

    fn push_row(&mut self, status: Status, event: impl Into<String>, source: &Path, target: Option<PathBuf>) {
        self.rows.push(Row {
            status,
            event: event.into(),
            source: source.to_path_buf(),
            target,
            selected: false,
        });
        self.scroll_to_last = true;
    }

    fn finish_file(&mut self) {
        if let Some(b) = self.batch.as_mut() {
            let total = b.files.len().max(1);
            let i = b.next;
            // Same formula as the original: (i*100 + 99) / n percent.
            self.progress = ((i * 100 + 99) / total).min(100) as f32 / 100.0;
            b.next += 1;
            if b.next >= b.files.len() {
                self.batch = None;
            }
        }
    }

    fn save_result(&mut self, doc: Box<lopdf::Document>, source: &Path, target: PathBuf) {
        match engine::save_plain(*doc, &target) {
            Ok(()) => self.push_row(Status::Succeed, EV_REMOVED, source, Some(target)),
            Err(_) => {
                let name = source.display();
                self.push_row(Status::Failed, format!("Decrypt the '{name}' file failed."), source, Some(target))
            }
        }
    }

    /// Handles the next file of the batch.
    fn step(&mut self) {
        if self.prompt.is_some() {
            return;
        }
        let Some(source) = self.batch.as_ref().and_then(|b| b.files.get(b.next).cloned()) else {
            self.batch = None;
            return;
        };

        match engine::open(&source, &[]) {
            Open::Unreadable(_) => {
                let name = source.display().to_string();
                self.push_row(Status::Error, format!("Failed to open \"{name}\" file."), &source, None);
            }
            Open::NotEncrypted => self.push_row(Status::Error, EV_NOT_ENCRYPTED, &source, None),
            Open::Decrypted(doc) => match self.target_for(&source) {
                Some(target) => self.save_result(doc, &source, target),
                None => {} // save dialog cancelled: skip this file
            },
            Open::NeedPassword => {
                let Some(target) = self.target_for(&source) else {
                    self.finish_file();
                    return;
                };
                // Try the passwords typed last time first.
                let owner = self.remembered_owner.clone();
                let user = self.remembered_user.clone();
                if !owner.is_empty() || !user.is_empty() {
                    if let Open::Decrypted(doc) = engine::open(&source, &[&owner, &user]) {
                        self.save_result(doc, &source, target);
                        self.finish_file();
                        return;
                    }
                }
                if self.show_password_prompt {
                    self.prompt = Some(Prompt {
                        source,
                        target,
                        user,
                        owner,
                        focus_done: false,
                    });
                    return; // wait for the person
                }
                self.push_row(Status::Failed, EV_BAD_PASSWORD, &source, Some(target));
            }
        }
        self.finish_file();
    }

    fn answer_prompt(&mut self, ok: bool) {
        let Some(p) = self.prompt.take() else { return };
        if ok {
            self.remembered_owner = p.owner.clone();
            self.remembered_user = p.user.clone();
            if let Open::Decrypted(doc) = engine::open(&p.source, &[&p.owner, &p.user]) {
                self.save_result(doc, &p.source, p.target);
                self.finish_file();
                return;
            }
        }
        self.push_row(Status::Failed, EV_BAD_PASSWORD, &p.source, Some(p.target));
        self.finish_file();
    }

    // ----- list actions -----------------------------------------------------

    fn select_click(&mut self, idx: usize, ctrl: bool, shift: bool) {
        if shift {
            let a = self.anchor.unwrap_or(idx);
            let (lo, hi) = (a.min(idx), a.max(idx));
            for (i, r) in self.rows.iter_mut().enumerate() {
                r.selected = (lo..=hi).contains(&i) || (ctrl && r.selected);
            }
        } else if ctrl {
            self.rows[idx].selected = !self.rows[idx].selected;
            self.anchor = Some(idx);
        } else {
            for (i, r) in self.rows.iter_mut().enumerate() {
                r.selected = i == idx;
            }
            self.anchor = Some(idx);
        }
    }

    fn view_selected(&self, target: bool) {
        for r in self.rows.iter().filter(|r| r.selected) {
            let p = if target { r.target.as_deref() } else { Some(r.source.as_path()) };
            if let Some(p) = p.filter(|p| !p.as_os_str().is_empty()) {
                crate::open_with_system(p);
            }
        }
    }

    // ----- drawing ----------------------------------------------------------

    fn list(&mut self, ui: &mut egui::Ui) {
        let frame_rect = ui.max_rect();
        ui.painter().rect_filled(frame_rect, CornerRadius::ZERO, Color32::WHITE);
        sunken_border(ui.painter(), frame_rect);

        let ctrl = ui.input(|i| i.modifiers.command);
        let shift = ui.input(|i| i.modifiers.shift);
        let mut clicked: Option<usize> = None;
        let mut right_clicked: Option<usize> = None;
        let scroll_last = std::mem::take(&mut self.scroll_to_last);
        let n = self.rows.len();

        let inner = frame_rect.shrink(2.0);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
        // Column dividers in the same light tone as the row lines.
        // The table's own column lines are hidden (they sit 1 px right of the
        // header dividers); light lines are drawn below at the dividers instead.
        child.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
        let mut divider_xs: Vec<f32> = Vec::new();
        let mut table = TableBuilder::new(&mut child)
            .striped(false)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(88.0).at_least(50.0).clip(true))
            .column(Column::initial(150.0).at_least(50.0).clip(true))
            .column(Column::initial(150.0).at_least(50.0).clip(true))
            .column(Column::remainder().at_least(60.0).clip(true))
            .min_scrolled_height(0.0)
            .max_scroll_height(inner.height() - 20.0)
            .auto_shrink([false, false]);
        if scroll_last && n > 0 {
            table = table.scroll_to_row(n - 1, None);
        }

        let output = table
            .header(19.0, |mut header| {
                for title in ["Result", "Event", "Source Document", "Target Document"] {
                    header.col(|ui| {
                        let r = ui.max_rect();
                        divider_xs.push(r.right() - 1.0);
                        ui.painter().rect_filled(r, CornerRadius::ZERO, FACE);
                        raised_border(ui.painter(), r);
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(RichText::new(title).color(INK)).selectable(false).truncate());
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, n, |mut row| {
                    let i = row.index();
                    let r = &self.rows[i];
                    let sel = r.selected;
                    let text_color = if sel { Color32::WHITE } else { INK };
                    let cell = |s: String| RichText::new(s).color(text_color);
                    let status = r.status;
                    let event = r.event.clone();
                    let source = r.source.display().to_string();
                    let target = r.target.as_ref().map(|t| t.display().to_string()).unwrap_or_default();

                    row.col(|ui| {
                        if sel {
                            ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, SELECT);
                        }
                        ui.add_space(2.0);
                        status_icon(ui, status);
                        ui.add_space(2.0);
                        ui.add(egui::Label::new(cell(status.label().to_string())).selectable(false).truncate());
                    });
                    for text in [event, source, target] {
                        row.col(|ui| {
                            if sel {
                                ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, SELECT);
                            }
                                ui.add_space(3.0);
                            ui.add(egui::Label::new(cell(text)).selectable(false).truncate());
                        });
                    }
                    let resp = row.response();
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    if resp.secondary_clicked() {
                        right_clicked = Some(i);
                    }
                    resp.context_menu(|ui| {
                        ui.set_min_width(170.0);
                        // Actions are applied below, after the table borrow ends.
                        let action = menu_buttons(ui);
                        if let Some(a) = action {
                            ui.data_mut(|d| d.insert_temp(egui::Id::unique("menu_action"), a));
                        }
                    });
                });
            });

        // Horizontal grid lines at every row boundary, down through the empty
        // part of the list too, like a list view with grid lines.
        let body = output.inner_rect;
        let painter = ui.painter().with_clip_rect(body);
        let offset = output.state.unclamped_offset().y;
        let mut y = body.top() - offset.rem_euclid(ROW_H) + ROW_H - 0.5;
        while y < body.bottom() {
            painter.line_segment([Pos2::new(body.left(), y), Pos2::new(body.right(), y)], Stroke::new(1.0, GRID));
            y += ROW_H;
        }
        // Column lines, exactly under the header's divider lines.
        for x in divider_xs.iter().take(divider_xs.len().saturating_sub(1)) {
            painter.line_segment([Pos2::new(*x, body.top()), Pos2::new(*x, body.bottom())], Stroke::new(1.0, GRID));
        }

        if let Some(i) = clicked {
            self.select_click(i, ctrl, shift);
        }
        if let Some(i) = right_clicked {
            // Right-click on an unselected row selects it alone (as in a list view).
            if !self.rows[i].selected {
                self.select_click(i, false, false);
            }
        }
        let action: Option<u8> = ui.data_mut(|d| d.remove_temp(egui::Id::unique("menu_action")));
        if let Some(a) = action {
            self.run_menu_action(a);
        }
    }

    fn run_menu_action(&mut self, a: u8) {
        match a {
            0 => self.view_selected(false),
            1 => self.view_selected(true),
            2 => self.rows.retain(|r| r.status != Status::Error),
            3 => self.rows.retain(|r| r.status != Status::Succeed),
            4 => self.rows.retain(|r| !r.selected),
            _ => self.rows.clear(),
        }
    }

    fn prompt_window(&mut self, ctx: &egui::Context) {
        let mut answer: Option<bool> = None;
        if let Some(p) = self.prompt.as_mut() {
            egui::Modal::new(egui::Id::unique("password_prompt")).show(ctx, |ui| {
                ui.set_width(300.0);
                ui.label(RichText::new(APP_NAME).strong());
                ui.add_space(6.0);
                ui.label("PDF Filename:");
                let mut name = p.source.display().to_string();
                ui.add(egui::TextEdit::singleline(&mut name).interactive(false).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.label("User Password:");
                let user = ui.add(egui::TextEdit::singleline(&mut p.user).password(true).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.label("Owner Password:");
                let owner = ui.add(egui::TextEdit::singleline(&mut p.owner).password(true).desired_width(f32::INFINITY));
                if !p.focus_done {
                    user.request_focus();
                    p.focus_done = true;
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(62.0);
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(72.0, 24.0))).clicked() {
                        answer = Some(true);
                    }
                    ui.add_space(14.0);
                    if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(72.0, 24.0))).clicked() {
                        answer = Some(false);
                    }
                });
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter && (user.lost_focus() || owner.lost_focus() || !(user.has_focus() || owner.has_focus())) {
                    answer = Some(true);
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(false);
                }
            });
        }
        if let Some(ok) = answer {
            self.answer_prompt(ok);
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let resp = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(48.0), egui::Sense::hover());
                if let Some(icon) = &self.about_icon {
                    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                    ui.painter().image(icon.id(), r, uv, Color32::WHITE);
                }
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    bold_label(ui, TITLE);
                    ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
                    ui.label("Removes passwords and restrictions from PDF files.");
                    ui.label("Supports RC4 40/128-bit and AES 128/256-bit encryption.");
                    ui.add_space(6.0);
                    ui.label("Linux Edition. Free and open-source Rust application.");
                    ui.add_space(6.0);
                    ui.label(COPYRIGHT);
                    ui.label("Released under the MIT License.");
                });
            });
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.add(egui::Button::new("OK").min_size(Vec2::new(72.0, 24.0))).clicked())
                .inner
        });
        if resp.inner || resp.should_close() {
            self.show_about = false;
        }
    }

    /// Help opens in its own system window, so it can be moved and resized
    /// freely, outside the main window.
    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("Help - {APP_NAME}"))
            .with_inner_size([560.0, 520.0])
            .with_min_inner_size([320.0, 240.0])
            .with_app_id("pdf-password-buster");
        if let Some((width, height, rgba)) = crate::decode_png(crate::ICON_64) {
            builder = builder.with_icon(Arc::new(egui::IconData { rgba, width, height }));
        }
        let closed = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("help_window"), builder, |ui, _class| {
            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, help_contents);
            });
            ui.ctx().input(|i| i.viewport().close_requested())
        });
        if closed {
            self.show_help = false;
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(s) = self.screenshot.as_mut() else { return };
        ctx.request_repaint();
        s.frames += 1;
        if s.frames >= 120 && !s.requested {
            s.requested = true;
            let slot = s.image.clone();
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                *slot.lock().unwrap() = Some(img);
                ctx2.request_repaint();
            });
        }
        let img = s.image.lock().unwrap().take();
        if let Some(img) = img {
            if let Err(e) = save_png(&s.path, &img) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn menu_buttons(ui: &mut egui::Ui) -> Option<u8> {
    let items = [
        "View Source Document",
        "View Target Document",
        "Delete \"ERROR\" Items",
        "Delete \"SUCCEED\" Items",
        "Delete Selected Items",
        "Delete All Items",
    ];
    let mut out = None;
    for (i, label) in items.iter().enumerate() {
        if ui.button(*label).clicked() {
            out = Some(i as u8);
            ui.close();
        }
    }
    out
}

impl eframe::App for RemoverApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Files dropped onto the window start a batch, like Open PDF(s).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| crate::cli::is_pdf(p)).collect()
        });
        if !dropped.is_empty() {
            self.start(dropped);
        }

        if self.batch.is_some() && self.prompt.is_none() {
            self.step();
            ctx.request_repaint();
        }

        let busy = self.batch.is_some();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, CornerRadius::ZERO, FACE);

        let margin = 10.0;
        let content = full.shrink(margin);
        let bar_h = 16.0;
        let btn_h = 56.0;
        let gap = 7.0;
        let list_rect = Rect::from_min_max(content.min, Pos2::new(content.max.x, content.max.y - bar_h - btn_h - 2.0 * gap));
        let btn_row = Rect::from_min_size(Pos2::new(content.min.x, list_rect.max.y + gap), Vec2::new(content.width(), btn_h));
        let bar_rect = Rect::from_min_max(Pos2::new(content.min.x, content.max.y - bar_h), content.max);

        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
        self.list(&mut list_ui);

        // Buttons: Open PDF(s), Help, About, Close.
        let labels = ["Open PDF(s)", "Help", "About", "Close"];
        let bw = (btn_row.width() - gap * (labels.len() as f32 - 1.0)) / labels.len() as f32;
        for (k, label) in labels.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(btn_row.min.x + k as f32 * (bw + gap), btn_row.min.y), Vec2::new(bw, btn_h));
            let enabled = !(busy && k == 0);
            let resp = ui.put(r, egui::Button::new(RichText::new(*label).size(14.0).color(INK)).fill(FACE_LIGHT));
            let resp = if enabled { resp } else { resp.on_hover_text("Working...") };
            if resp.clicked() && enabled {
                match k {
                    0 => {
                        let picked = rfd::FileDialog::new()
                            .set_title("Open")
                            .add_filter("PDF Files(*.pdf)", &["pdf", "PDF"])
                            .pick_files();
                        if let Some(files) = picked {
                            self.start(files);
                        }
                    }
                    1 => {
                        if self.show_help {
                            ctx.send_viewport_cmd_to(egui::ViewportId::from_hash_of("help_window"), egui::ViewportCommand::Focus);
                        }
                        self.show_help = true;
                    }
                    2 => self.show_about = true,
                    _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }

        progress_bar(ui.painter(), bar_rect, self.progress);

        self.prompt_window(&ctx);
        self.about_window(&ctx);
        self.help_window(&ctx);
        self.handle_screenshot(&ctx);
    }
}

fn help_contents(ui: &mut egui::Ui) {
    ui.heading(APP_NAME);
    ui.label(
        "Removes the \"owner\" password that stops a PDF from being printed, edited, \
         copied or annotated, and the \"user\" password that stops it from opening \
         when you know it. The new copy opens in any PDF viewer with no limits.",
    );
    ui.add_space(6.0);
    ui.strong("How to use");
    ui.label("1. Click Open PDF(s), or drag PDF files onto the window.");
    ui.label("2. One file: choose a name for the new copy. Several files: choose an output folder.");
    ui.label("3. If a file needs its password, type the user or owner password.");
    ui.label("4. Right-click an entry to view the original or the new document, or to clear the list.");
    ui.add_space(6.0);
    ui.strong("Results");
    ui.label("SUCCEED - the password was removed.");
    ui.label("ERROR - the file is not encrypted or could not be read.");
    ui.label("FAILED - the password was not correct.");
    ui.add_space(6.0);
    ui.strong("Command line");
    ui.monospace(crate::cli::USAGE);
}

// ----- painting helpers -------------------------------------------------------

/// Body-size text drawn in bold. The built-in font has no bold face, so the
/// glyphs are painted twice, a fraction of a pixel apart.
fn bold_label(ui: &mut egui::Ui, text: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, INK);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(1.0, 0.0), egui::Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), INK);
    ui.painter().galley(rect.min + Vec2::new(0.7, 0.0), galley, INK);
}

fn apply_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.global_style_mut(|style| {
        let v = &mut style.visuals;
        v.panel_fill = FACE;
        v.window_fill = FACE;
        v.extreme_bg_color = Color32::WHITE;
        v.override_text_color = Some(INK);
        v.selection.bg_fill = SELECT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.window_corner_radius = CornerRadius::same(4);
        let w = &mut v.widgets;
        w.inactive.weak_bg_fill = FACE_LIGHT;
        w.inactive.bg_fill = FACE_LIGHT;
        w.inactive.bg_stroke = Stroke::new(1.0, SHADOW);
        w.inactive.corner_radius = CornerRadius::same(4);
        w.hovered.weak_bg_fill = Color32::from_rgb(0xF4, 0xF2, 0xEE);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x50, 0x50, 0x50));
        w.hovered.corner_radius = CornerRadius::same(4);
        w.active.weak_bg_fill = Color32::from_rgb(0xC0, 0xBC, 0xB4);
        w.active.corner_radius = CornerRadius::same(4);
        w.noninteractive.bg_stroke = Stroke::new(1.0, SHADOW);
        style.spacing.button_padding = Vec2::new(8.0, 3.0);
    });
}


fn sunken_border(p: &egui::Painter, r: Rect) {
    let dark = Stroke::new(1.0, SHADOW);
    let light = Stroke::new(1.0, Color32::WHITE);
    p.line_segment([r.left_top(), r.right_top()], dark);
    p.line_segment([r.left_top(), r.left_bottom()], dark);
    p.line_segment([r.left_bottom(), r.right_bottom()], light);
    p.line_segment([r.right_top(), r.right_bottom()], light);
}

fn raised_border(p: &egui::Painter, r: Rect) {
    let dark = Stroke::new(1.0, SHADOW);
    let light = Stroke::new(1.0, Color32::WHITE);
    p.line_segment([r.left_top(), r.right_top()], light);
    p.line_segment([r.left_top(), r.left_bottom()], light);
    p.line_segment([r.left_bottom(), r.right_bottom()], dark);
    p.line_segment([r.right_top() - Vec2::new(1.0, 0.0), r.right_bottom() - Vec2::new(1.0, 0.0)], dark);
}

fn progress_bar(p: &egui::Painter, r: Rect, value: f32) {
    p.rect_filled(r, CornerRadius::ZERO, FACE);
    sunken_border(p, r);
    let inner = r.shrink(2.0);
    let filled = inner.width() * value.clamp(0.0, 1.0);
    // Segmented blocks, like a classic progress control.
    let block: f32 = 9.0;
    let mut x = inner.min.x;
    while x < inner.min.x + filled - 1.0 {
        let w = (block - 2.0).min(inner.min.x + filled - x);
        p.rect_filled(Rect::from_min_size(Pos2::new(x, inner.min.y), Vec2::new(w, inner.height())), CornerRadius::ZERO, SELECT);
        x += block;
    }
}

fn status_icon(ui: &mut egui::Ui, status: Status) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(15.0), egui::Sense::hover());
    let p = ui.painter();
    let c = r.center();
    let rad = 6.5;
    match status {
        Status::Succeed => {
            p.circle_filled(c, rad, Color32::from_rgb(0x2D, 0x5F, 0xC8));
            p.circle_stroke(c, rad, Stroke::new(1.0, Color32::from_rgb(0x1A, 0x3C, 0x8A)));
            p.circle_filled(c + Vec2::new(0.0, -3.0), 1.1, Color32::WHITE);
            p.line_segment([c + Vec2::new(0.0, -0.8), c + Vec2::new(0.0, 3.6)], Stroke::new(1.8, Color32::WHITE));
        }
        _ => {
            p.circle_filled(c, rad, Color32::from_rgb(0xD8, 0x1E, 0x1E));
            p.circle_stroke(c, rad, Stroke::new(1.0, Color32::from_rgb(0x8A, 0x10, 0x10)));
            let s = Stroke::new(1.8, Color32::WHITE);
            p.line_segment([c + Vec2::new(-2.8, -2.8), c + Vec2::new(2.8, 2.8)], s);
            p.line_segment([c + Vec2::new(2.8, -2.8), c + Vec2::new(-2.8, 2.8)], s);
        }
    }
}


fn save_png(path: &Path, img: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = img.size;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())
}
