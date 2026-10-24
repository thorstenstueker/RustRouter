//! RustRouter GUI: load a Specctra DSN design, watch the autorouter and export the SES session.
//! The layout follows the Freerouting board frame (menu bar, toolbar, board view, status bar).

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod model;
mod view;

use eframe::egui;
use model::DrawModel;
use rustrouter::autoroute::pipeline::{run_pipeline, stats, JobProgress, JobStage};
use rustrouter::board::Board;
use rustrouter::datastructures::StopFlag;
use rustrouter::io::specctra::{self, DsnReadResult};
use rustrouter::job::{create_routing_job, JobOptions};
use rustrouter::settings::RouterSettings;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use view::View;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Minimum time between two board updates sent to the GUI while routing.
const UPDATE_INTERVAL: Duration = Duration::from_millis(300);
const MAX_LOG_LINES: usize = 5000;

// ------------------------------------------------------------------------------ logging

/// Log messages for the log window and the status bar (also written to stderr).
static LOG_LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

struct GuiLogger {
    level: log::LevelFilter,
}

impl log::Log for GuiLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level && metadata.target().starts_with("rustrouter")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{:<5} {}", record.level(), record.args());
        eprintln!("{line}");
        if let Ok(mut l) = LOG_LINES.lock() {
            l.push_back(line);
            while l.len() > MAX_LOG_LINES {
                l.pop_front();
            }
        }
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    }

    fn flush(&self) {}
}

fn init_logging() {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(log::LevelFilter::Info);
    if log::set_boxed_logger(Box::new(GuiLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}

// ------------------------------------------------------------------------------ app state

/// A loaded design.
struct Design {
    path: PathBuf,
    name: String,
    dsn: Arc<DsnReadResult>,
    /// The board shown and exported: the loaded board or the result of the last routing run.
    board: Board,
    settings: RouterSettings,
}

/// State shared with the loader and routing threads.
#[derive(Default)]
struct Shared {
    model: Option<Arc<DrawModel>>,
    progress: Option<JobProgress>,
    loaded: Option<Result<Design, String>>,
    routed: Option<(Board, RouterSettings)>,
    routing: bool,
    loading: bool,
}

/// The values of the settings window.
struct OptionsUi {
    max_passes: i32,
    max_threads: i32,
    improvement_threshold: f32,
    job_timeout: String,
    optimizer: bool,
    fanout: bool,
}

impl Default for OptionsUi {
    fn default() -> Self {
        let d = RouterSettings::default();
        OptionsUi {
            max_passes: d.autorouter.max_passes,
            max_threads: d.optimizer.max_threads,
            improvement_threshold: d.optimizer.optimization_improvement_threshold,
            job_timeout: d.job_timeout_string.clone(),
            optimizer: true,
            fanout: true,
        }
    }
}

impl OptionsUi {
    fn job_options(&self) -> JobOptions {
        JobOptions {
            max_passes: Some(self.max_passes.max(0)),
            max_threads: Some(self.max_threads.max(1)),
            improvement_threshold: Some(self.improvement_threshold),
            job_timeout: (!self.job_timeout.trim().is_empty()).then(|| self.job_timeout.trim().to_string()),
            no_optimizer: !self.optimizer,
            no_fanout: !self.fanout,
            no_router: false,
        }
    }
}

struct RustRouterApp {
    shared: Arc<Mutex<Shared>>,
    design: Option<Design>,
    model: Option<Arc<DrawModel>>,
    view: View,
    fit_pending: bool,
    active_layer: usize,
    layer_visible: Vec<bool>,
    show_airlines: bool,
    show_log: bool,
    show_settings: bool,
    show_about: bool,
    options: OptionsUi,
    stop: Option<StopFlag>,
    route_started: Option<Instant>,
    route_elapsed: Option<Duration>,
    message: String,
    pending_open: Option<PathBuf>,
    /// Test hook (env RR_GUI_SHOT=<file.ppm>, RR_GUI_AUTOROUTE=1): save a screenshot after
    /// loading (and routing) the design given on the command line, then exit.
    test_shot: Option<PathBuf>,
    test_autoroute: bool,
    test_frames: u32,
}

impl RustRouterApp {
    fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let _ = REPAINT.set(cc.egui_ctx.clone());
        RustRouterApp {
            shared: Arc::new(Mutex::new(Shared::default())),
            design: None,
            model: None,
            view: View::default(),
            fit_pending: false,
            active_layer: 0,
            layer_visible: Vec::new(),
            show_airlines: true,
            show_log: false,
            show_settings: false,
            show_about: false,
            options: OptionsUi::default(),
            stop: None,
            route_started: None,
            route_elapsed: None,
            message: "Open a Specctra DSN design (File > Open).".to_string(),
            pending_open: open,
            test_shot: std::env::var_os("RR_GUI_SHOT").map(PathBuf::from),
            test_autoroute: std::env::var_os("RR_GUI_AUTOROUTE").is_some(),
            test_frames: 0,
        }
    }

    fn is_routing(&self) -> bool {
        self.shared.lock().unwrap().routing
    }

    fn is_busy(&self) -> bool {
        let s = self.shared.lock().unwrap();
        s.routing || s.loading
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open design")
            .add_filter("Specctra DSN", &["dsn", "DSN"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.load(path);
        }
    }

    /// Loads a design in a background thread.
    fn load(&mut self, path: PathBuf) {
        if self.is_busy() {
            return;
        }
        self.message = format!("Loading '{}'...", path.display());
        log::info!("Loading board file '{}'...", path.display());
        let shared = self.shared.clone();
        shared.lock().unwrap().loading = true;
        std::thread::spawn(move || {
            let result = (|| -> Result<Design, String> {
                let dsn = specctra::read_dsn(&path).map_err(|e| format!("Error while reading DSN file: {e:?}"))?;
                for w in &dsn.warnings {
                    log::warn!("{w}");
                }
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| dsn.design_name.clone().unwrap_or_default());
                // The board as the routing job sees it (with the rules of the DSN applied).
                let job = create_routing_job(&dsn, &JobOptions::default());
                Ok(Design { path, name, dsn: Arc::new(dsn), board: job.board, settings: job.settings })
            })();
            let mut s = shared.lock().unwrap();
            if let Ok(d) = &result {
                s.model = Some(Arc::new(DrawModel::from_board(&d.board, &d.settings, true)));
            }
            s.loaded = Some(result);
            s.loading = false;
            drop(s);
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        });
    }

    /// Starts the autorouter (routing pipeline) on the loaded design in a background thread.
    fn start_routing(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(design) = &self.design else {
            return;
        };
        let mut job = create_routing_job(&design.dsn, &self.options.job_options());
        self.stop = Some(job.thread.clone());
        let shared = self.shared.clone();
        let observer_settings = job.settings.clone();
        let last_update = Mutex::new(Instant::now() - UPDATE_INTERVAL);
        let observer_shared = shared.clone();
        job.observer = Some(Arc::new(move |board: &Board, progress: &JobProgress| {
            let mut last = last_update.lock().unwrap();
            if !progress.pass_finished && last.elapsed() < UPDATE_INTERVAL {
                return;
            }
            let model = DrawModel::from_board(board, &observer_settings, progress.pass_finished);
            *last = Instant::now();
            let mut s = observer_shared.lock().unwrap();
            // Keep the airlines and statistics of the last pass end between pass ends.
            let model = match (&s.model, progress.pass_finished) {
                (Some(old), false) => DrawModel { airlines: old.airlines.clone(), stats: old.stats.clone(), ..model },
                _ => model,
            };
            s.model = Some(Arc::new(model));
            s.progress = Some(*progress);
            drop(s);
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        }));
        {
            let mut s = shared.lock().unwrap();
            s.routing = true;
            s.progress = None;
        }
        self.route_started = Some(Instant::now());
        self.route_elapsed = None;
        self.message = "Autorouter started.".to_string();
        log::info!("Starting the autorouter on '{}'...", design.name);
        std::thread::spawn(move || {
            let started = Instant::now();
            run_pipeline(&mut job);
            let st = stats::board_stats(&job.board, &job.settings);
            log::info!(
                "Routing finished in {:.2} seconds: {} unrouted connection(s), {} clearance violation(s), {} via(s), router score {:.2}.",
                started.elapsed().as_secs_f64(),
                st.incomplete_count,
                st.clearance_violation_count,
                st.via_count,
                st.router_score
            );
            let model = DrawModel::from_board(&job.board, &job.settings, true);
            let mut s = shared.lock().unwrap();
            s.model = Some(Arc::new(model));
            s.routed = Some((job.board, job.settings));
            s.routing = false;
            drop(s);
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        });
    }

    fn stop_routing(&mut self) {
        if let Some(s) = &self.stop {
            s.request_stop();
            self.message = "Stopping the autorouter...".to_string();
        }
    }

    fn export_dialog(&mut self) {
        let Some(design) = &self.design else {
            return;
        };
        let default_name = format!("{}.ses", design.name);
        let dir = design.path.parent().map(|p| p.to_path_buf());
        let mut dialog = rfd::FileDialog::new()
            .set_title("Export Specctra session")
            .add_filter("Specctra session", &["ses"])
            .set_file_name(&default_name);
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        if let Some(path) = dialog.save_file() {
            match specctra::write_ses_file(&design.board, &design.name, &path) {
                Ok(()) => {
                    log::info!("Session file written to '{}'.", path.display());
                    self.message = format!("Session file written to '{}'.", path.display());
                }
                Err(e) => {
                    log::error!("Unable to write session file '{}': {e:?}", path.display());
                    self.message = format!("Unable to write session file: {e:?}");
                }
            }
        }
    }

    /// Takes over the results of the background threads.
    fn poll_shared(&mut self) {
        let mut s = self.shared.lock().unwrap();
        if let Some(m) = &s.model {
            if self.model.as_ref().map(|o| !Arc::ptr_eq(o, m)).unwrap_or(true) {
                self.model = Some(m.clone());
            }
        }
        if let Some(loaded) = s.loaded.take() {
            match loaded {
                Ok(d) => {
                    let n = d.board.layer_structure.layers.len();
                    self.layer_visible = vec![true; n];
                    self.active_layer = 0;
                    self.message = format!("Design '{}' loaded.", d.name);
                    self.design = Some(d);
                    self.fit_pending = true;
                    self.route_elapsed = None;
                }
                Err(e) => {
                    log::error!("{e}");
                    self.message = e;
                }
            }
        }
        if let Some((board, settings)) = s.routed.take() {
            if let Some(d) = &mut self.design {
                d.board = board;
                d.settings = settings;
            }
            self.route_elapsed = self.route_started.map(|t| t.elapsed());
            self.stop = None;
            self.message = "Autorouter finished.".to_string();
        }
    }

    // -------------------------------------------------------------------------- UI parts

    fn run_test_hook(&mut self, ctx: &egui::Context) {
        let Some(path) = self.test_shot.clone() else {
            return;
        };
        if self.design.is_none() || self.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if self.test_autoroute {
            self.test_autoroute = false;
            self.start_routing();
            return;
        }
        self.test_frames += 1;
        if self.test_frames == 1 {
            if let (Some(ses), Some(d)) = (std::env::var_os("RR_GUI_SES"), &self.design) {
                let _ = specctra::write_ses_file(&d.board, &d.name, PathBuf::from(ses));
            }
        }
        if self.test_frames == 5 {
            let c = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                for p in &img.pixels {
                    out.extend_from_slice(&[p.r(), p.g(), p.b()]);
                }
                let _ = std::fs::write(&path, out);
                c.send_viewport_cmd(egui::ViewportCommand::Close);
                c.request_repaint();
            })));
        }
        ctx.request_repaint();
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let busy = self.is_busy();
        let routing = self.is_routing();
        let has_design = self.design.is_some();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add_enabled(!busy, egui::Button::new("Open design...")).clicked() {
                    ui.close();
                    self.open_dialog();
                }
                if ui
                    .add_enabled(has_design && !routing, egui::Button::new("Export Specctra session file..."))
                    .clicked()
                {
                    ui.close();
                    self.export_dialog();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Display", |ui| {
                if ui.button("Zoom all").clicked() {
                    ui.close();
                    self.fit_pending = true;
                }
                ui.checkbox(&mut self.show_airlines, "Incompletes (rats nest)");
                ui.separator();
                for (i, name) in self.model.as_ref().map(|m| m.layers.clone()).unwrap_or_default().iter().enumerate() {
                    if let Some(v) = self.layer_visible.get_mut(i) {
                        ui.checkbox(v, format!("Layer {name}"));
                    }
                }
            });
            ui.menu_button("Autorouter", |ui| {
                if ui.add_enabled(has_design && !busy, egui::Button::new("Start autorouter")).clicked() {
                    ui.close();
                    self.start_routing();
                }
                if ui.add_enabled(routing, egui::Button::new("Stop")).clicked() {
                    ui.close();
                    self.stop_routing();
                }
                ui.separator();
                if ui.button("Settings...").clicked() {
                    ui.close();
                    self.show_settings = true;
                }
            });
            ui.menu_button("Info", |ui| {
                if ui.button("Log").clicked() {
                    ui.close();
                    self.show_log = true;
                }
                if ui.button("About RustRouter").clicked() {
                    ui.close();
                    self.show_about = true;
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let busy = self.is_busy();
        let routing = self.is_routing();
        let has_design = self.design.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Open")).clicked() {
                self.open_dialog();
            }
            if ui
                .add_enabled(has_design && !busy, egui::Button::new("Autorouter"))
                .on_hover_text("Route the design (fanout, autorouter and optimizer)")
                .clicked()
            {
                self.start_routing();
            }
            if ui.add_enabled(routing, egui::Button::new("Stop")).clicked() {
                self.stop_routing();
            }
            if ui.add_enabled(has_design && !routing, egui::Button::new("Export SES")).clicked() {
                self.export_dialog();
            }
            ui.separator();
            if ui.button("Zoom all").clicked() {
                self.fit_pending = true;
            }
            ui.toggle_value(&mut self.show_airlines, "Incompletes");
            ui.separator();
            if let Some(m) = &self.model {
                ui.label("Layer:");
                let current = m.layers.get(self.active_layer).cloned().unwrap_or_default();
                egui::ComboBox::from_id_salt("active_layer").selected_text(current).show_ui(ui, |ui| {
                    for (i, name) in m.layers.iter().enumerate() {
                        ui.selectable_value(&mut self.active_layer, i, name);
                    }
                });
                for i in 0..m.layers.len().min(self.layer_visible.len()) {
                    let c = view::layer_color(i, 255);
                    ui.checkbox(&mut self.layer_visible[i], egui::RichText::new(&m.layers[i]).color(c));
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let (progress, routing, loading) = {
            let s = self.shared.lock().unwrap();
            (s.progress, s.routing, s.loading)
        };
        let last_log = LOG_LINES.lock().ok().and_then(|l| l.back().cloned());
        ui.horizontal(|ui| {
            let msg = if routing || loading {
                last_log.map(|l| l.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string()).unwrap_or_default()
            } else {
                self.message.clone()
            };
            if routing || loading {
                ui.spinner();
            }
            ui.add(egui::Label::new(msg).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(st) = self.model.as_ref().and_then(|m| m.stats.clone()) {
                    ui.label(format!(
                        "Unrouted: {}   Violations: {}   Vias: {}   Score: {:.2}",
                        st.unrouted, st.violations, st.vias, st.score
                    ));
                    ui.separator();
                }
                let elapsed = if routing { self.route_started.map(|t| t.elapsed()) } else { self.route_elapsed };
                if let Some(e) = elapsed {
                    ui.label(format!("{:02}:{:02}", e.as_secs() / 60, e.as_secs() % 60));
                }
                if let (Some(p), true) = (progress, routing) {
                    let stage = match p.stage {
                        JobStage::Routing => "Routing",
                        JobStage::Optimizing => "Optimizing",
                    };
                    ui.label(format!("{stage}, pass {}", p.pass));
                    ui.separator();
                }
            });
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if self.show_settings {
            let mut open = true;
            let defaults = RouterSettings::default();
            egui::Window::new("Autorouter settings").open(&mut open).resizable(false).show(ctx, |ui| {
                egui::Grid::new("settings").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    ui.label("Maximum passes (0 = no limit)");
                    ui.add(egui::DragValue::new(&mut self.options.max_passes).range(0..=10000));
                    ui.end_row();
                    ui.label("Optimizer threads");
                    ui.add(egui::DragValue::new(&mut self.options.max_threads).range(1..=256));
                    ui.end_row();
                    ui.label("Run fanout");
                    ui.checkbox(&mut self.options.fanout, "");
                    ui.end_row();
                    ui.label("Run optimizer");
                    ui.checkbox(&mut self.options.optimizer, "");
                    ui.end_row();
                    ui.label("Optimizer improvement threshold (%)");
                    ui.add(egui::DragValue::new(&mut self.options.improvement_threshold).range(0.0..=100.0).speed(0.1));
                    ui.end_row();
                    ui.label("Job timeout (HH:MM:SS)");
                    ui.text_edit_singleline(&mut self.options.job_timeout);
                    ui.end_row();
                });
                ui.add_space(6.0);
                if ui.button("Reset to defaults").clicked() {
                    self.options = OptionsUi::default();
                }
                ui.label(
                    egui::RichText::new(format!(
                        "Settings in the DSN file override these defaults (default passes: {}).",
                        defaults.autorouter.max_passes
                    ))
                    .small()
                    .weak(),
                );
            });
            self.show_settings = open;
        }
        if self.show_log {
            let mut open = true;
            egui::Window::new("Log").open(&mut open).default_size([700.0, 400.0]).show(ctx, |ui| {
                let lines = LOG_LINES.lock().map(|l| l.iter().cloned().collect::<Vec<_>>()).unwrap_or_default();
                egui::ScrollArea::both().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    for l in lines {
                        ui.monospace(l);
                    }
                });
            });
            self.show_log = open;
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new("About RustRouter").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
                ui.heading(format!("RustRouter {}", env!("CARGO_PKG_VERSION")));
                ui.label("PCB autorouter for Specctra DSN designs.");
                ui.label("A Rust port of the Freerouting autorouter.");
                ui.hyperlink("https://github.com/freerouting/freerouting");
                ui.label("License: GNU GPL v3");
            });
            self.show_about = open;
        }
    }

    fn board_view(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, view::BACKGROUND);
        let Some(model) = self.model.clone() else {
            let busy = self.is_busy();
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if busy { "Loading..." } else { "No design loaded" },
                egui::FontId::proportional(18.0),
                egui::Color32::GRAY,
            );
            return;
        };
        if self.fit_pending {
            self.view.fit(&model, rect);
            self.fit_pending = false;
        }
        // Pan with any mouse button, zoom with the wheel or pinch around the pointer.
        if response.dragged() {
            self.view.pan(response.drag_delta());
        }
        if let Some(pos) = response.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = zoom * (scroll / 200.0).exp();
            if (factor - 1.0).abs() > 1e-4 {
                self.view.zoom_at(rect, pos, factor);
            }
        }
        if response.double_clicked() {
            self.fit_pending = true;
        }
        view::paint(&painter, rect, &model, &self.view, self.active_layer, &self.layer_visible, self.show_airlines);
        if let Some(pos) = response.hover_pos() {
            let b = self.view.to_board(rect, pos);
            let text = match &self.design {
                Some(d) => {
                    let c = &d.board.communication;
                    format!("x {:.3}  y {:.3} mm", board_to_mm(c, b[0] as f64), board_to_mm(c, b[1] as f64))
                }
                None => format!("{:.0}, {:.0}", b[0], b[1]),
            };
            painter.text(
                rect.right_bottom() + egui::vec2(-8.0, -6.0),
                egui::Align2::RIGHT_BOTTOM,
                text,
                egui::FontId::monospace(12.0),
                egui::Color32::from_gray(170),
            );
        }
    }
}

/// A board coordinate in millimeters.
fn board_to_mm(c: &rustrouter::board::structure::Communication, v: f64) -> f64 {
    let dsn = match &c.coordinate_transform {
        Some(t) => t.board_to_dsn(v),
        None => v / c.resolution as f64,
    };
    rustrouter::board::structure::Unit::scale(dsn, c.unit, rustrouter::board::structure::Unit::Mm)
}

impl eframe::App for RustRouterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(p) = self.pending_open.take() {
            self.load(p);
        }
        self.poll_shared();
        let ctx = ui.ctx().clone();
        // Dropped DSN files are opened.
        let dropped: Option<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| { let p = f.path(); (!p.as_os_str().is_empty()).then(|| p.to_path_buf()) }));
        if let Some(p) = dropped {
            self.load(p);
        }
        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui);
        });
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.toolbar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            self.status_bar(ui);
        });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.board_view(ui);
        });
        self.windows(&ctx);
        self.run_test_hook(&ctx);
        if self.is_routing() {
            // Keep the elapsed time ticking.
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

fn main() -> eframe::Result {
    init_logging();
    let open = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustRouter")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true)
            .with_app_id("rustrouter")
            .with_icon(Arc::new(egui::IconData {
                rgba: include_bytes!("../../assets/rustrouter-64.rgba").to_vec(),
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };
    eframe::run_native("RustRouter", options, Box::new(move |cc| Ok(Box::new(RustRouterApp::new(cc, open)))))
}
