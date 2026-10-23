//! The activity: the entry point `android_main`, the eframe app around [`FilmcraftApp`], and the
//! services the shared UI gets on a tablet (the in-app file browser, cpal audio, auto-save).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Sender, channel};

use chrono::TimeZone as _;
use filmcraft_engine::Session;
use filmcraft_engine::autosave::AutosaveConfig;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::native_dialogs::FileDialog;
use winit::platform::android::activity::{AndroidApp, WindowManagerFlags};

use crate::browser::{Browser, Done, Request};
use crate::dirs::{self, Dirs};

/// File dialogs the UI asked for, answered by the browser window one at a time.
type Requests = Rc<RefCell<VecDeque<(Request, Sender<Vec<String>>)>>>;

struct Host {
    app: FilmcraftApp,
    /// The activity, for the usable area of the window.
    android: AndroidApp,
    requests: Requests,
    browser: Option<(Browser, Sender<Vec<String>>)>,
    places: Vec<(String, PathBuf)>,
    /// Where the next dialog opens: the last folder used.
    last_dir: PathBuf,
    /// The interface zoom was set for the screen (once; after that the user's).
    zoomed: bool,
}

fn local_offset(unix: i64) -> i32 {
    chrono::Local.timestamp_opt(unix, 0).single().map(|d| d.offset().local_minus_utc()).unwrap_or(0)
}

struct GpuFrameRenderer(filmcraft_gpu::ExportRenderer);

impl filmcraft_export::FrameRenderer for GpuFrameRenderer {
    fn render(
        &mut self,
        project: &filmcraft_project::Project,
        seq: filmcraft_project::ItemId,
        t: filmcraft_time::Tick,
        opts: filmcraft_render::RenderOptions,
        sources: &dyn filmcraft_render::SourceProvider,
    ) -> Option<filmcraft_render::Image> {
        self.0.render(project, seq, t, opts, sources)
    }
}

/// The session: auto-save and preferences in the data folder, the demo project (or the most
/// recent one, as Settings ▸ General ▸ At Startup says) open.
fn open_session(dirs: &Dirs) -> Session {
    let mut session = Session::default();
    let mut cfg = AutosaveConfig::new(dirs.data.clone());
    cfg.local_offset = local_offset;
    if let Err(e) = session.start_autosave(cfg) {
        log::warn!("auto-save and crash recovery unavailable: {e}");
    }
    // (no voice-over input: the microphone needs the record permission's prompt, which needs JNI)
    let at_startup = session.prefs.general.at_startup.clone();
    if at_startup == "openMostRecent" {
        let recent = session.prefs.general.recent_projects.iter().find(|p| std::path::Path::new(p).exists()).cloned();
        let r = match recent {
            Some(p) => session.execute("file.open", serde_json::json!({"path": p})),
            None => session.execute("file.openDemoProject", serde_json::json!({})),
        };
        if let Err(e) = r {
            log::error!("{e}");
        }
    } else if at_startup != "emptyProject"
        && let Err(e) = session.execute("file.openDemoProject", serde_json::json!({}))
    {
        log::error!("{e}");
    }
    session
}

impl Host {
    fn new(cc: &eframe::CreationContext<'_>, android: AndroidApp, dirs: Dirs) -> Host {
        for d in [&dirs.projects, &dirs.export] {
            if let Err(e) = std::fs::create_dir_all(d) {
                log::warn!("{}: {e}", d.display());
            }
        }
        filmcraft_export::register_frame_renderer(|| {
            filmcraft_gpu::ExportRenderer::new().map(|r| Box::new(GpuFrameRenderer(r)) as Box<dyn filmcraft_export::FrameRenderer>)
        });
        let mut app = FilmcraftApp::new(open_session(&dirs));
        if let Some(rs) = cc.wgpu_render_state.clone() {
            app.set_wgpu(rs);
        }
        // Settings ▸ Audio Hardware is applied on the first frame, as on the desktop
        app.audio = Some(Box::new(filmcraft_audio_cpal::CpalOut::new()));
        let requests: Requests = Rc::new(RefCell::new(VecDeque::new()));
        let queue = requests.clone();
        let repaint = cc.egui_ctx.clone();
        app.hooks.run_dialog = Some(Box::new(move |dialog: FileDialog| {
            let (tx, rx) = channel();
            queue.borrow_mut().push_back((Request::from_dialog(dialog), tx));
            repaint.request_repaint();
            rx
        }));
        let places = dirs::places(&dirs);
        let last_dir = dirs.projects.clone();
        Host { app, android, requests, browser: None, places, last_dir, zoomed: false }
    }
}

impl eframe::App for Host {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.zoomed {
            self.zoomed = true;
            let size = ctx.content_rect().size();
            let zoom = dirs::tablet_zoom(size.x, size.y);
            log::info!("screen {}×{} points: interface zoom {zoom}", size.x, size.y);
            ctx.set_zoom_factor(zoom);
        }
        if self.browser.is_none()
            && let Some((req, reply)) = self.requests.borrow_mut().pop_front()
        {
            let start = if self.last_dir.is_dir() { self.last_dir.clone() } else { self.places.first().map(|(_, p)| p.clone()).unwrap_or_default() };
            self.browser = Some((Browser::open(req, start, self.places.clone()), reply));
        }
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // the window can lie under the system bars or the soft keyboard: lay out in what's left
        let r = self.android.content_rect();
        if let Some(window) = raw.screen_rect
            && let Some(ppp) = raw.viewports.get(&raw.viewport_id).and_then(|v| v.native_pixels_per_point)
            && let Some(area) = crate::insets::content_area(window, (r.left, r.top, r.right, r.bottom), ppp)
        {
            raw.screen_rect = Some(area);
        }
        eframe::App::raw_input_hook(&mut self.app, ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        if let Some((browser, _)) = self.browser.as_mut()
            && let Some(done) = browser.show(ui.ctx())
        {
            let Some((browser, reply)) = self.browser.take() else { return };
            self.last_dir = browser.dir().to_path_buf();
            let paths = match done {
                Done::Picked(paths) => paths,
                Done::Cancelled => Vec::new(),
            };
            // the command's dialog went away with the command: nobody to tell
            let _ = reply.send(paths);
            ui.ctx().request_repaint();
        }
    }

    fn on_exit(&mut self) {
        eframe::App::on_exit(&mut self.app);
    }
}

/// The activity's entry point (`android-activity` calls it on its own thread once the
/// `NativeActivity` is created). Returning ends the activity; a failure is logged, never a panic.
// The symbol name is what `android-activity` resolves (`extern "Rust" { fn android_main(..) }`),
// so it must not be mangled; the lint counts the attribute as unsafe code, not any block here.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("filmcraft"));
    let Some(internal) = app.internal_data_path() else {
        log::error!("no internal data path: FilmCraft can't keep projects or settings here");
        return;
    };
    let dirs = Dirs::of(&internal, app.external_data_path().as_deref());
    log::info!("data {}, projects {}, shared storage {:?}", dirs.data.display(), dirs.projects.display(), dirs.shared);
    // Full screen: the window is laid out under the status bar on devices that draw edge to
    // edge, and the menu bar would be under the clock. Hidden, as in most editing apps; the
    // insets a window could ask for instead need JNI (docs/android.md → Status).
    app.set_window_flags(WindowManagerFlags::FULLSCREEN | WindowManagerFlags::KEEP_SCREEN_ON, WindowManagerFlags::empty());
    let host_app = app.clone();
    let options = eframe::NativeOptions {
        android_app: Some(app),
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("FilmCraft"),
        ..Default::default()
    };
    let result = eframe::run_native("FilmCraft", options, Box::new(move |cc| Ok(Box::new(Host::new(cc, host_app, dirs)))));
    if let Err(e) = result {
        log::error!("{}", crate::start_failure(&e));
    }
}
