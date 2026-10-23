//! The header's compact menu (`FilmcraftApp::compact_menu`, the tablet host): one ☰ button whose
//! popup lists the top-level menus as submenus. The real `FilmcraftApp` runs under `egui_kittest`.
//!
//! Set `FILMCRAFT_UI_SNAPSHOT_DIR=<dir>` to render the window offscreen with wgpu and write
//! `compact-menu-*.png` there.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use filmcraft_engine::Session;
use filmcraft_ui_egui::FilmcraftApp;
use filmcraft_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, FilmcraftApp>,
    tx: Sender<ControlRequest>,
    snapshots: Option<std::path::PathBuf>,
}

impl Driver {
    fn demo(compact: bool) -> Self {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("demo project");
        let (tx, rx) = channel();
        let mut app = FilmcraftApp::new(session).with_control(rx);
        app.compact_menu = compact;
        let snapshots = std::env::var_os("FILMCRAFT_UI_SNAPSHOT_DIR").map(std::path::PathBuf::from);
        let mut b = Harness::builder().with_size(egui::vec2(1280.0, 800.0)).with_max_steps(10_000);
        if snapshots.is_some() {
            b = b.wgpu();
        }
        let harness = b.build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx, snapshots };
        d.frames(4);
        d
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                return v;
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
        v["result"].clone()
    }

    fn ids(&mut self, prefix: &str) -> Vec<String> {
        let v = self.ok("ui.elements", json!({"prefix": prefix}));
        v.as_array().unwrap().iter().filter_map(|e| e["id"].as_str().map(str::to_string)).collect()
    }

    fn snapshot(&mut self, name: &str) {
        let Some(dir) = self.snapshots.clone() else { return };
        self.frames(2);
        match self.harness.render() {
            Ok(img) => {
                std::fs::create_dir_all(&dir).unwrap();
                let path = dir.join(format!("{name}.png"));
                img.save(&path).unwrap();
                eprintln!("snapshot: {}", path.display());
            }
            Err(e) => eprintln!("snapshot {name} skipped: {e}"),
        }
    }
}

#[test]
fn the_compact_menu_replaces_the_bar_and_opens_the_top_level_menus() {
    let mut d = Driver::demo(true);
    let header = d.ids("header.menu");
    assert_eq!(header, ["header.menu"], "the ☰ button is in the header");
    assert!(d.ids("menu.").is_empty(), "no menu bar titles while the popup is closed");
    d.snapshot("compact-menu-closed");
    d.ok("ui.click", json!({"id": "header.menu"}));
    d.frames(2);
    let menus = d.ids("menu.");
    for top in ["menu.File", "menu.Edit", "menu.Sequence", "menu.Help"] {
        assert!(menus.iter().any(|m| m == top), "{top} in the popup: {menus:?}");
    }
    d.snapshot("compact-menu-open");
    // the tablet host's zoom on a 1280 × 800-point screen: the header, with the bar gone, fits it
    d.ok("ui.click", json!({"id": "header.menu"}));
    d.harness.ctx.set_zoom_factor(1.25);
    d.frames(3);
    d.snapshot("compact-menu-zoom125");
}

#[test]
fn without_the_compact_menu_the_bar_is_there() {
    let mut d = Driver::demo(false);
    assert!(d.ids("header.menu").is_empty());
    assert!(d.ids("menu.").iter().any(|m| m == "menu.File"), "the bar's titles are registered");
}
