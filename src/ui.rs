use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::engine::{Options, Progress, Report};

#[derive(Clone, Copy, PartialEq)]
enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    surface: Color32,
    inset: Color32,
    line: Color32,
    text: Color32,
    muted: Color32,
    blue: Color32,
    frost: Color32,
    red: Color32,
    warning: Color32,
    error: Color32,
    good: Color32,
    button: Color32,
    active: Color32,
    selected: Color32,
}

impl Theme {
    fn from_saved(value: Option<String>) -> Self {
        if value.as_deref() == Some("light") {
            Self::Light
        } else {
            Self::Dark
        }
    }

    fn saved(self) -> &'static str {
        if self == Self::Light { "light" } else { "dark" }
    }

    fn colors(self) -> Palette {
        if self == Self::Dark {
            Palette {
                background: Color32::from_rgb(24, 31, 41),
                surface: Color32::from_rgb(32, 42, 55),
                inset: Color32::from_rgb(17, 25, 36),
                line: Color32::from_rgb(74, 94, 115),
                text: Color32::from_rgb(232, 239, 246),
                muted: Color32::from_rgb(170, 186, 202),
                blue: Color32::from_rgb(32, 72, 111),
                frost: Color32::from_rgb(164, 211, 244),
                red: Color32::from_rgb(134, 26, 43),
                warning: Color32::from_rgb(238, 192, 112),
                error: Color32::from_rgb(255, 155, 160),
                good: Color32::from_rgb(157, 217, 187),
                button: Color32::from_rgb(39, 54, 71),
                active: Color32::from_rgb(22, 45, 69),
                selected: Color32::from_rgb(32, 72, 111),
            }
        } else {
            Palette {
                background: Color32::from_rgb(235, 239, 243),
                surface: Color32::from_rgb(222, 230, 238),
                inset: Color32::from_rgb(250, 251, 253),
                line: Color32::from_rgb(131, 150, 168),
                text: Color32::from_rgb(28, 41, 57),
                muted: Color32::from_rgb(73, 91, 111),
                blue: Color32::from_rgb(38, 82, 124),
                frost: Color32::from_rgb(33, 84, 129),
                red: Color32::from_rgb(134, 26, 43),
                warning: Color32::from_rgb(122, 77, 10),
                error: Color32::from_rgb(160, 26, 42),
                good: Color32::from_rgb(21, 103, 67),
                button: Color32::from_rgb(221, 230, 239),
                active: Color32::from_rgb(188, 208, 226),
                selected: Color32::from_rgb(199, 221, 240),
            }
        }
    }
}

pub fn run(initial_files: Vec<PathBuf>) -> eframe::Result {
    let icon = image::load_from_memory(include_bytes!("../assets/icon/flattener.png"))
        .expect("the embedded application icon is a validated PNG")
        .to_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Flattener")
            .with_icon(Arc::new(icon))
            .with_inner_size([930.0, 660.0])
            .with_min_inner_size([760.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Flattener",
        native,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            let theme =
                Theme::from_saved(cc.storage.and_then(|storage| storage.get_string("theme")));
            apply_style(&cc.egui_ctx, theme);
            Ok(Box::new(Flattener::new(initial_files, theme)))
        }),
    )
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let name = "Libre Baskerville".to_owned();
    fonts.font_data.insert(
        name.clone(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/LibreBaskerville.ttf")).into(),
    );
    fonts
        .families
        .insert(egui::FontFamily::Proportional, vec![name.clone()]);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, vec![name]);
    ctx.set_fonts(fonts);
}

fn apply_style(ctx: &egui::Context, theme: Theme) {
    let colors = theme.colors();
    ctx.set_theme(if theme == Theme::Dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if theme == Theme::Dark {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::Light
    }));
    let mut style = (*ctx.style()).clone();
    style.visuals = if theme == Theme::Dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.text);
    style.visuals.panel_fill = colors.background;
    style.visuals.window_fill = colors.surface;
    style.visuals.extreme_bg_color = colors.inset;
    style.visuals.faint_bg_color = colors.surface;
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.window_stroke = Stroke::new(1.0, colors.line);
    style.visuals.selection.bg_fill = colors.selected;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.frost);
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::ZERO;
        widget.bg_stroke = Stroke::new(1.0, colors.line);
        widget.fg_stroke = Stroke::new(1.0, colors.text);
    }
    style.visuals.widgets.inactive.bg_fill = colors.button;
    style.visuals.widgets.inactive.weak_bg_fill = colors.button;
    style.visuals.widgets.hovered.bg_fill = colors.selected;
    style.visuals.widgets.hovered.weak_bg_fill = colors.selected;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, colors.frost);
    style.visuals.widgets.active.bg_fill = colors.active;
    style.visuals.widgets.active.weak_bg_fill = colors.active;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, colors.frost);
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(23.0));
    ctx.set_style(style);
}

#[derive(Clone, Copy, PartialEq)]
enum Quality {
    Compact,
    Balanced,
    Detail,
    Custom,
}

impl Quality {
    fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact - 150 DPI",
            Self::Balanced => "Balanced - 200 DPI",
            Self::Detail => "Detail - 300 DPI",
            Self::Custom => "Custom",
        }
    }

    fn dpi(self) -> Option<u32> {
        match self {
            Self::Compact => Some(150),
            Self::Balanced => Some(200),
            Self::Detail => Some(300),
            Self::Custom => None,
        }
    }
}

enum State {
    Waiting,
    Running,
    Complete(Report),
    Failed(String),
    Cancelled,
}

struct Entry {
    input: PathBuf,
    output: Option<PathBuf>,
    state: State,
}

impl Entry {
    fn status(&self, colors: Palette) -> (&str, Color32) {
        match &self.state {
            State::Waiting => ("Waiting", colors.muted),
            State::Running => ("Converting", colors.frost),
            State::Complete(report) if !report.warnings.is_empty() => {
                ("Complete - review", colors.warning)
            }
            State::Complete(_) => ("Complete", colors.good),
            State::Failed(_) => ("Failed", colors.error),
            State::Cancelled => ("Cancelled", colors.muted),
        }
    }
}

enum Event {
    Started(usize),
    Progress(usize, Progress),
    Finished(usize, Result<Report, String>),
    Cancelled(usize),
    Done,
}

struct Flattener {
    theme: Theme,
    show_licenses: bool,
    files: Vec<Entry>,
    selected: Option<usize>,
    destination: Option<PathBuf>,
    quality: Quality,
    dpi: u32,
    jpeg_quality: u8,
    lossless: bool,
    searchable: bool,
    events: Option<mpsc::Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    progress: Option<Progress>,
    active: Option<usize>,
    started: Option<Instant>,
    elapsed: Duration,
    notice: Option<String>,
    close_after_cancel: bool,
}

impl Flattener {
    fn new(initial_files: Vec<PathBuf>, theme: Theme) -> Self {
        let defaults = Options::default();
        let mut app = Self {
            theme,
            show_licenses: false,
            files: Vec::new(),
            selected: None,
            destination: None,
            quality: match defaults.dpi {
                150 => Quality::Compact,
                200 => Quality::Balanced,
                300 => Quality::Detail,
                _ => Quality::Custom,
            },
            dpi: defaults.dpi,
            jpeg_quality: defaults.jpeg_quality,
            lossless: defaults.lossless,
            searchable: defaults.searchable,
            events: None,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
            active: None,
            started: None,
            elapsed: Duration::ZERO,
            notice: None,
            close_after_cancel: false,
        };
        app.add_paths(initial_files);
        app
    }

    fn busy(&self) -> bool {
        self.events.is_some()
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut seen: HashSet<PathBuf> = self
            .files
            .iter()
            .map(|entry| identity(&entry.input))
            .collect();
        let mut skipped = 0;
        for path in paths {
            if path.is_dir() {
                match std::fs::read_dir(&path) {
                    Ok(children) => {
                        let mut files = Vec::new();
                        for child in children {
                            match child {
                                Ok(child) => {
                                    let path = child.path();
                                    if is_pdf(&path) && path.is_file() {
                                        files.push(path);
                                    }
                                }
                                Err(_) => skipped += 1,
                            }
                        }
                        files.sort();
                        self.add_paths(files);
                        seen = self
                            .files
                            .iter()
                            .map(|entry| identity(&entry.input))
                            .collect();
                    }
                    Err(_) => skipped += 1,
                }
            } else if is_pdf(&path) && path.is_file() {
                if seen.insert(identity(&path)) {
                    self.files.push(Entry {
                        input: path,
                        output: None,
                        state: State::Waiting,
                    });
                }
            } else {
                skipped += 1;
            }
        }
        if self.selected.is_none() && !self.files.is_empty() {
            self.selected = Some(0);
        }
        if skipped > 0 {
            self.notice = Some(format!(
                "{skipped} item(s) could not be added. Choose readable PDF files; folders include only their direct PDF files."
            ));
        }
    }

    fn start(&mut self) {
        if self.busy() {
            return;
        }
        self.notice = None;
        let mut reserved = HashSet::new();
        let mut jobs = Vec::new();
        for (index, entry) in self.files.iter_mut().enumerate() {
            if !matches!(
                entry.state,
                State::Waiting | State::Failed(_) | State::Cancelled
            ) {
                continue;
            }
            match output_path(&entry.input, self.destination.as_deref(), &mut reserved) {
                Ok(output) => {
                    entry.output = Some(output.clone());
                    entry.state = State::Waiting;
                    jobs.push((index, entry.input.clone(), output));
                }
                Err(error) => entry.state = State::Failed(error),
            }
        }
        if jobs.is_empty() {
            return;
        }
        let options = Options {
            dpi: self.dpi,
            jpeg_quality: self.jpeg_quality,
            lossless: self.lossless,
            searchable: self.searchable,
            max_page_pixels: 40_000_000,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        self.events = Some(receiver);
        self.started = Some(Instant::now());
        self.elapsed = Duration::ZERO;
        self.progress = None;
        self.active = None;
        let spawned = std::thread::Builder::new()
            .name("flattener-batch".into())
            .spawn(move || {
                for (index, input, output) in jobs {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = sender.send(Event::Cancelled(index));
                        continue;
                    }
                    let _ = sender.send(Event::Started(index));
                    let result =
                        crate::worker::convert(&input, &output, &options, &cancel, |progress| {
                            let _ = sender.send(Event::Progress(index, progress));
                        });
                    // A completed output remains completed even when Cancel was clicked just
                    // after publication. Failed work during cancellation is marked separately.
                    match result {
                        Ok(report) => {
                            let _ = sender.send(Event::Finished(index, Ok(report)));
                        }
                        Err(_) if cancel.load(Ordering::Relaxed) => {
                            let _ = sender.send(Event::Cancelled(index));
                        }
                        Err(error) => {
                            let _ = sender.send(Event::Finished(index, Err(format!("{error:#}"))));
                        }
                    }
                }
                let _ = sender.send(Event::Done);
            });
        if let Err(error) = spawned {
            self.events = None;
            self.started = None;
            self.notice = Some(format!(
                "Could not start conversion: {error}. Close other applications and retry."
            ));
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut ended = false;
        let mut disconnected = false;
        if let Some(receiver) = &self.events {
            loop {
                match receiver.try_recv() {
                    Ok(Event::Started(index)) => {
                        self.active = Some(index);
                        self.progress = None;
                        self.files[index].state = State::Running;
                    }
                    Ok(Event::Progress(index, progress)) if self.active == Some(index) => {
                        self.progress = Some(progress);
                    }
                    Ok(Event::Progress(_, _)) => {}
                    Ok(Event::Finished(index, result)) => {
                        self.files[index].state = match result {
                            Ok(report) => State::Complete(report),
                            Err(error) => State::Failed(error),
                        };
                    }
                    Ok(Event::Cancelled(index)) => self.files[index].state = State::Cancelled,
                    Ok(Event::Done) => {
                        ended = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ended = true;
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if let Some(started) = self.started {
            self.elapsed = started.elapsed();
        }
        if ended {
            self.events = None;
            self.active = None;
            self.progress = None;
            self.started = None;
            if disconnected {
                for entry in &mut self.files {
                    match entry.state {
                        State::Running => entry.state = State::Failed("The conversion worker ended unexpectedly. Retry this file; existing copies remain available.".into()),
                        State::Waiting => entry.state = State::Cancelled,
                        _ => {}
                    }
                }
                self.notice = Some(
                    "Conversion stopped unexpectedly. Review the file statuses before retrying."
                        .into(),
                );
            }
            if self.close_after_cancel {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), egui::Sense::hover());
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.left_top(), colors.frost);
        mesh.colored_vertex(rect.right_top(), Color32::from_rgb(73, 109, 144));
        mesh.colored_vertex(rect.left_bottom(), colors.blue);
        mesh.colored_vertex(rect.right_bottom(), colors.inset);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(2, 1, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.add_space(7.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Flattener").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if self.theme == Theme::Dark {
                        "Light mode"
                    } else {
                        "Dark mode"
                    })
                    .on_hover_text("Switch appearance; your choice is remembered.")
                    .clicked()
                {
                    self.theme = if self.theme == Theme::Dark {
                        Theme::Light
                    } else {
                        Theme::Dark
                    };
                    apply_style(ui.ctx(), self.theme);
                    ui.ctx().request_repaint();
                }
                if ui.button("Licenses").clicked() {
                    self.show_licenses = true;
                }
                ui.label(
                    RichText::new("Adobe Acrobat 7 / PDF 1.6")
                        .size(12.0)
                        .color(colors.muted),
                );
            });
        });
        ui.label(
            RichText::new("Make modern PDFs easier to open and navigate.").color(colors.muted),
        );
        ui.add_space(5.0);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        let busy = self.busy();
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("Add PDFs")
                    .on_hover_text("Choose one or more PDFs (Ctrl+O)")
                    .clicked()
                    && let Some(paths) = rfd::FileDialog::new()
                        .add_filter("PDF documents", &["pdf"])
                        .pick_files()
                {
                    self.add_paths(paths);
                }
                if ui
                    .button("Add folder")
                    .on_hover_text("Add PDFs directly inside a folder; subfolders are excluded.")
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    self.add_paths(vec![path]);
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Remove"))
                    .clicked()
                    && let Some(index) = self.selected.take()
                {
                    self.files.remove(index);
                    self.selected = if self.files.is_empty() {
                        None
                    } else {
                        Some(index.min(self.files.len() - 1))
                    };
                }
                if ui
                    .add_enabled(!self.files.is_empty(), egui::Button::new("Clear list"))
                    .clicked()
                {
                    self.files.clear();
                    self.selected = None;
                    self.notice = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} file(s)", self.files.len())).color(colors.muted),
                    );
                });
            });
        });
    }

    fn queue(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        egui::Frame::new()
            .fill(colors.inset)
            .stroke(Stroke::new(1.0, colors.line))
            .inner_margin(8.0)
            .show(ui, |ui| {
                let height = (ui.available_height() - 440.0).clamp(100.0, 260.0);
                egui::ScrollArea::vertical()
                    .id_salt("file_queue")
                    .min_scrolled_height(height)
                    .max_height(height)
                    .show(ui, |ui| {
                        ui.set_min_height(height);
                        if self.files.is_empty() {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Drop PDFs here")
                                        .size(18.0)
                                        .color(colors.frost),
                                );
                                ui.label(
                                    RichText::new("or use Add PDFs to select your documents")
                                        .color(colors.muted),
                                );
                            });
                            return;
                        }
                        let width = ui.available_width();
                        egui::Grid::new("queue_grid")
                            .num_columns(3)
                            .striped(true)
                            .spacing([12.0, 7.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("Document").small().color(colors.muted));
                                ui.label(RichText::new("Output").small().color(colors.muted));
                                ui.label(RichText::new("Status").small().color(colors.muted));
                                ui.end_row();
                                for (index, entry) in self.files.iter().enumerate() {
                                    let name = entry
                                        .input
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy();
                                    if ui
                                        .add_sized(
                                            [width - 300.0, 26.0],
                                            egui::Button::selectable(
                                                self.selected == Some(index),
                                                RichText::new(name).color(colors.text),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(entry.input.display().to_string())
                                        .clicked()
                                    {
                                        self.selected = Some(index);
                                    }
                                    let size = match &entry.state {
                                        State::Complete(report) => format!(
                                            "{} pages / {}",
                                            report.output_pages,
                                            bytes(report.output_bytes)
                                        ),
                                        _ => "-".into(),
                                    };
                                    ui.label(RichText::new(size).size(12.0).color(colors.muted));
                                    let (status, color) = entry.status(colors);
                                    ui.label(RichText::new(status).color(color));
                                    ui.end_row();
                                }
                            });
                    });
            });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        let busy = self.busy();
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Save copies").strong());
                let destination = self.destination.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Beside each original".into());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.destination.is_some() && ui.button("Use source folder").clicked() {
                        self.destination = None;
                    }
                    if ui.button("Choose folder").clicked() && let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.destination = Some(path);
                    }
                    ui.add(egui::Label::new(RichText::new(destination).color(colors.muted)).truncate());
                });
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.label("Quality");
                let before = self.quality;
                egui::ComboBox::from_id_salt("quality").width(170.0).selected_text(self.quality.label()).show_ui(ui, |ui| {
                    for quality in [Quality::Compact, Quality::Balanced, Quality::Detail, Quality::Custom] {
                        ui.selectable_value(&mut self.quality, quality, quality.label());
                    }
                });
                if before != self.quality && let Some(dpi) = self.quality.dpi() {
                    self.dpi = dpi;
                }
                if self.quality == Quality::Custom {
                    ui.add(egui::DragValue::new(&mut self.dpi).range(72..=600).suffix(" DPI"));
                }
                ui.checkbox(&mut self.lossless, "Lossless images").on_hover_text("Keeps rendered pixel detail without JPEG compression; output files can be much larger.");
                ui.add_enabled_ui(!self.lossless, |ui| {
                    ui.label("JPEG");
                    ui.add(egui::DragValue::new(&mut self.jpeg_quality).range(50..=100).suffix("%"));
                });
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.searchable, "Searchable text (OCR)").on_hover_text("Uses offline Latin-script OCR. Fine print, handwriting and numeric values can be misread; verify critical text against the visible page.");
                ui.label(RichText::new("Add searchable text to each copy.").small().color(colors.muted));
            });
        });
        ui.label(
            RichText::new(
                "Compact is faster; higher DPI preserves more fine print and uses more memory.",
            )
            .small()
            .color(colors.muted),
        );
        ui.label(RichText::new("Copies preserve page appearance; forms, layers, links and signatures become static. Originals are kept.").size(12.0).color(colors.muted));
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        let busy = self.busy();
        let eligible = self
            .files
            .iter()
            .filter(|entry| {
                matches!(
                    entry.state,
                    State::Waiting | State::Failed(_) | State::Cancelled
                )
            })
            .count();
        ui.horizontal(|ui| {
            if busy {
                let stopping = self.cancel.load(Ordering::Relaxed);
                if ui
                    .add_enabled(
                        !stopping,
                        egui::Button::new(if stopping { "Cancelling..." } else { "Cancel" })
                            .min_size(Vec2::new(112.0, 32.0)),
                    )
                    .clicked()
                {
                    self.cancel.store(true, Ordering::Relaxed);
                }
                ui.label(
                    RichText::new(if stopping {
                        "Finishing the current operation safely."
                    } else {
                        "Converting copies"
                    })
                    .color(colors.frost),
                );
            } else if ui
                .add_enabled(
                    eligible > 0,
                    egui::Button::new(
                        RichText::new(format!("Convert {eligible} file(s)"))
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(colors.red)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(197, 111, 126)))
                    .min_size(Vec2::new(145.0, 32.0)),
                )
                .clicked()
            {
                self.start();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let complete = self
                    .files
                    .iter()
                    .filter(|e| matches!(e.state, State::Complete(_)))
                    .count();
                let failed = self
                    .files
                    .iter()
                    .filter(|e| matches!(e.state, State::Failed(_)))
                    .count();
                let cancelled = self
                    .files
                    .iter()
                    .filter(|e| matches!(e.state, State::Cancelled))
                    .count();
                ui.label(
                    RichText::new(format!(
                        "{complete} complete / {failed} failed / {cancelled} cancelled"
                    ))
                    .small()
                    .color(colors.muted),
                );
            });
        });
        if busy {
            if let Some(entry) = self.active.and_then(|index| self.files.get(index)) {
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            entry
                                .input
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                        )
                        .small()
                        .color(colors.frost),
                    )
                    .truncate(),
                );
            }
            if let Some(progress) = &self.progress {
                let fraction = if progress.total_pages == 0 {
                    0.0
                } else {
                    (progress.page as f32 / progress.total_pages as f32).clamp(0.0, 1.0)
                };
                let label = if progress.total_pages == 0 {
                    progress.message.clone()
                } else {
                    format!(
                        "{} / {} pages complete",
                        progress.page, progress.total_pages
                    )
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .fill(colors.selected)
                        .corner_radius(0)
                        .animate(progress.total_pages == 0)
                        .text(label),
                );
                if progress.total_pages > 0 {
                    ui.label(RichText::new(&progress.message).small().color(colors.muted));
                }
            } else {
                ui.add(
                    egui::ProgressBar::new(0.0)
                        .animate(true)
                        .corner_radius(0)
                        .text("Preparing document..."),
                );
            }
        } else if self.elapsed > Duration::ZERO {
            ui.label(
                RichText::new(format!(
                    "Last run: {}m {:02}s",
                    self.elapsed.as_secs() / 60,
                    self.elapsed.as_secs() % 60
                ))
                .small()
                .color(colors.muted),
            );
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme.colors();
        if let Some(notice) = &self.notice {
            ui.label(RichText::new(notice).color(colors.warning));
        }
        let Some(entry) = self.selected.and_then(|index| self.files.get(index)) else {
            return;
        };
        egui::Frame::new()
            .fill(colors.surface)
            .inner_margin(9.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("selected_details")
                    .max_height(100.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(
                                entry
                                    .input
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                            )
                            .strong(),
                        );
                        match &entry.state {
                            State::Waiting => {
                                ui.label(
                                    RichText::new("Ready for conversion.").color(colors.muted),
                                );
                            }
                            State::Running => {
                                ui.label(
                                    RichText::new("Conversion is in progress.").color(colors.frost),
                                );
                            }
                            State::Cancelled => {
                                ui.label(
                                    RichText::new(
                                        "Cancelled. Select Convert to try this file again.",
                                    )
                                    .color(colors.muted),
                                );
                            }
                            State::Failed(error) => {
                                ui.label(RichText::new(error).color(colors.error));
                            }
                            State::Complete(report) => {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} input pages / {} output pages",
                                            report.input_pages, report.output_pages
                                        ))
                                        .color(colors.muted),
                                    );
                                    if let Some(output) = &entry.output {
                                        if ui.button("Open copy").clicked()
                                            && let Err(error) = open(output)
                                        {
                                            self.notice = Some(error);
                                        }
                                        if ui.button("Open folder").clicked()
                                            && let Some(parent) = output.parent()
                                            && let Err(error) = open(parent)
                                        {
                                            self.notice = Some(error);
                                        }
                                    }
                                });
                                if let Some(output) = &entry.output {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(output.display().to_string())
                                                .small()
                                                .color(colors.muted),
                                        )
                                        .wrap(),
                                    );
                                }
                                for warning in &report.warnings {
                                    ui.label(
                                        RichText::new(format!("Review: {warning}"))
                                            .color(colors.warning),
                                    );
                                }
                            }
                        }
                    });
            });
    }

    fn licenses(&mut self, ctx: &egui::Context) {
        if !self.show_licenses {
            return;
        }
        let colors = self.theme.colors();
        let available = ctx.screen_rect().size();
        let response = egui::Modal::new(egui::Id::new("licenses_modal"))
            .frame(
                egui::Frame::new()
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0, colors.line))
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                ui.set_width((available.x - 64.0).clamp(240.0, 740.0));
                ui.heading("Licenses");
                ui.label(
                    RichText::new("Software, libraries and text recognition models")
                        .color(colors.muted),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("license_text")
                    .max_height((available.y - 210.0).clamp(120.0, 500.0))
                    .show(ui, |ui| {
                        let mut text: &str = crate::THIRD_PARTY_LICENSES;
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Copy notices").clicked() {
                        ctx.copy_text(crate::THIRD_PARTY_LICENSES.to_owned());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            ui.close();
                        }
                    });
                });
            });
        if response.should_close() {
            self.show_licenses = false;
        }
    }
}

impl eframe::App for Flattener {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", self.theme.saved().into());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let colors = self.theme.colors();
        self.poll(ctx);
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
            if ctx.input(|input| !input.raw.dropped_files.is_empty()) {
                self.notice =
                    Some("Conversion is running. Add more PDFs after this batch finishes.".into());
            }
            if ctx.input(|input| input.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.cancel.store(true, Ordering::Relaxed);
                self.close_after_cancel = true;
            }
        } else {
            let dropped = ctx.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect::<Vec<_>>()
            });
            if !dropped.is_empty() {
                self.add_paths(dropped);
            }
            if !self.show_licenses
                && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::O))
                && let Some(paths) = rfd::FileDialog::new()
                    .add_filter("PDF documents", &["pdf"])
                    .pick_files()
            {
                self.add_paths(paths);
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("main")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.header(ui);
                        self.toolbar(ui);
                        self.queue(ui);
                        ui.add_space(5.0);
                        self.settings(ui);
                        ui.add_space(5.0);
                        ui.separator();
                        self.controls(ui);
                        self.details(ui);
                    });
            });
        if !ctx.input(|input| input.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_hint"),
            ));
            let rect = ctx.screen_rect().shrink(5.0);
            painter.rect_stroke(
                rect,
                0,
                Stroke::new(2.0, colors.frost),
                egui::StrokeKind::Inside,
            );
        }
        self.licenses(ctx);
    }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn identity(path: &Path) -> PathBuf {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    let path = PathBuf::from(path.as_os_str().to_string_lossy().to_lowercase());
    path
}

fn output_path(
    input: &Path,
    destination: Option<&Path>,
    reserved: &mut HashSet<PathBuf>,
) -> Result<PathBuf, String> {
    let folder = destination
        .or_else(|| input.parent())
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !folder.is_dir() {
        return Err(
            "The output folder is unavailable. Choose another folder and try again.".into(),
        );
    }
    let stem = input.file_stem().ok_or("The input filename is missing.")?;
    for counter in 0..10_000 {
        let mut name = stem.to_os_string();
        if counter == 0 {
            name.push("_Acrobat7.pdf");
        } else {
            name.push(format!("_Acrobat7_{counter}.pdf"));
        }
        let path = folder.join(name);
        if !path.exists() && reserved.insert(identity(&path)) {
            return Ok(path);
        }
    }
    Err("No unused output filename could be found. Choose a different output folder.".into())
}

fn bytes(value: u64) -> String {
    if value >= 1_048_576 {
        format!("{:.1} MB", value as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", value as f64 / 1024.0)
    }
}

fn open(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let result = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_round_trip_and_unknown_setting_use_safe_default() {
        #[derive(Default)]
        struct Settings(std::collections::HashMap<String, String>);
        impl eframe::Storage for Settings {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.into(), value);
            }
            fn flush(&mut self) {}
        }
        use eframe::{App, Storage};
        for theme in [Theme::Dark, Theme::Light] {
            let mut app = Flattener::new(Vec::new(), theme);
            let mut settings = Settings::default();
            app.save(&mut settings);
            let restored = Theme::from_saved(settings.get_string("theme"));
            assert!(restored == theme);
            let ctx = egui::Context::default();
            apply_style(&ctx, restored);
            assert_eq!(
                ctx.theme(),
                if theme == Theme::Dark {
                    egui::Theme::Dark
                } else {
                    egui::Theme::Light
                }
            );
            assert_eq!(ctx.style().visuals.panel_fill, theme.colors().background);
        }
        assert!(Theme::from_saved(None) == Theme::Dark);
        assert!(Theme::from_saved(Some("unsupported".into())) == Theme::Dark);
    }

    #[test]
    fn both_palettes_keep_small_text_and_statuses_readable() {
        fn luminance(color: Color32) -> f64 {
            let channel = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            channel(color.r()) * 0.2126 + channel(color.g()) * 0.7152 + channel(color.b()) * 0.0722
        }
        for theme in [Theme::Dark, Theme::Light] {
            let colors = theme.colors();
            for foreground in [
                colors.text,
                colors.muted,
                colors.frost,
                colors.warning,
                colors.error,
                colors.good,
            ] {
                for background in [colors.background, colors.surface, colors.inset] {
                    let a = luminance(foreground);
                    let b = luminance(background);
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(
                        contrast >= 4.5,
                        "{} text contrast was {contrast}",
                        theme.saved()
                    );
                }
            }
        }
    }

    #[test]
    fn existing_copy_and_same_named_batch_inputs_get_distinct_outputs() {
        let folder = tempfile::tempdir().unwrap();
        let existing = folder.path().join("drawing_Acrobat7.pdf");
        std::fs::write(&existing, b"keep this output").unwrap();
        let mut reserved = HashSet::new();
        let first = output_path(
            Path::new("one/drawing.pdf"),
            Some(folder.path()),
            &mut reserved,
        )
        .unwrap();
        let second = output_path(
            Path::new("two/drawing.pdf"),
            Some(folder.path()),
            &mut reserved,
        )
        .unwrap();
        assert_eq!(first.file_name().unwrap(), "drawing_Acrobat7_1.pdf");
        assert_eq!(second.file_name().unwrap(), "drawing_Acrobat7_2.pdf");
        assert_eq!(std::fs::read(existing).unwrap(), b"keep this output");
    }

    #[test]
    fn unavailable_destination_does_not_fall_back_to_source_folder() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("removed");
        assert!(
            output_path(
                &folder.path().join("source.pdf"),
                Some(&missing),
                &mut HashSet::new()
            )
            .is_err()
        );
        assert!(!missing.exists());
    }
}
