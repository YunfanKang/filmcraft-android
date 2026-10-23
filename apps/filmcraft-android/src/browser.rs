//! The file browser that stands in for the native file dialogs (`rfd` has no Android backend):
//! an egui window over the app that answers a [`FileDialog`] with paths, through
//! `HostHooks::run_dialog`. Rows are finger-sized. Listing is a pure function; the window shows
//! why a folder can't be read (shared storage needs the media permission).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use filmcraft_ui_egui::i18n::t;
use filmcraft_ui_egui::native_dialogs::FileDialog;

/// What a dialog chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Any number of files.
    Files,
    /// One file.
    File,
    /// One folder.
    Folder,
    /// Where to save one file.
    Save,
}

/// A dialog to show, from the UI's request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub kind: Kind,
    /// The window's title, translated.
    pub title: String,
    /// The extensions admitted (without dots; empty: every file).
    pub exts: Vec<String>,
    /// [`Kind::Save`]: the suggested file name.
    pub file_name: String,
    /// Where to open, when the request says.
    pub start: Option<PathBuf>,
}

impl Request {
    pub fn from_dialog(dialog: FileDialog) -> Request {
        let req = |kind, title: &str, exts: Vec<String>, file_name: String| Request { kind, title: t(title).to_string(), exts, file_name, start: None };
        match dialog {
            FileDialog::OpenFiles { exts } => req(Kind::Files, "Import", exts, String::new()),
            FileDialog::OpenFile { filter, exts } => req(Kind::File, &filter, exts, String::new()),
            FileDialog::OpenProject => req(Kind::File, "Open Project", vec!["fcproj".to_string()], String::new()),
            FileDialog::SaveProject { name } => req(Kind::Save, "Save Project", vec!["fcproj".to_string()], name),
            FileDialog::SaveAs { filter, exts, name } => req(Kind::Save, &filter, exts, name),
            FileDialog::Folder { at } => Request { start: at.map(PathBuf::from), ..req(Kind::Folder, "Choose a Folder", Vec::new(), String::new()) },
            FileDialog::Relink { exts, .. } => req(Kind::File, "Locate File", exts, String::new()),
        }
    }
}

/// One row of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Whether a file name has one of `exts` (case-insensitive; an empty list admits every file).
pub fn admits(name: &str, exts: &[String]) -> bool {
    if exts.is_empty() {
        return true;
    }
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    exts.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
}

/// The folders, then the admitted files of `dir`, each sorted by name (case-insensitive); hidden
/// entries (`.name`) left out. `files` `false` lists folders only.
pub fn list(dir: &Path, exts: &[String], files: bool) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| read_error(dir, &e))?;
    let mut dirs = Vec::new();
    let mut found = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let is_dir = if kind.is_symlink() { entry.path().is_dir() } else { kind.is_dir() };
        if is_dir {
            dirs.push(Entry { name, path: entry.path(), is_dir: true });
        } else if files && admits(&name, exts) {
            found.push(Entry { name, path: entry.path(), is_dir: false });
        }
    }
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name));
    dirs.sort_by(by_name);
    found.sort_by(by_name);
    dirs.append(&mut found);
    Ok(dirs)
}

/// Why a shared-storage folder shows its subfolders but none of its files: Android (11 and later)
/// lists a folder to an app without the media permission, but hides every file in it that the
/// app didn't create itself, instead of refusing the folder. Shown when `dir` is under `shared`
/// and the listing found no file.
pub fn hidden_files_hint(dir: &Path, shared: Option<&Path>, entries: &[Entry]) -> Option<String> {
    let under_shared = shared.is_some_and(|s| dir.starts_with(s));
    (under_shared && !entries.iter().any(|e| !e.is_dir)).then(|| {
        "No files here? Android hides videos, photos and music from FilmCraft until it has the Photos and videos (and Music and audio) permission: Android Settings ▸ Apps ▸ FilmCraft ▸ Permissions, then come back.".to_string()
    })
}

/// Why `dir` can't be listed, with what to do about it.
pub fn read_error(dir: &Path, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "FilmCraft can't read {}. Allow Photos and videos (or Files and media) for FilmCraft in Android Settings ▸ Apps ▸ FilmCraft ▸ Permissions, then come back.",
            dir.display()
        )
    } else {
        format!("{}: {e}", dir.display())
    }
}

/// A file dialog that is up.
pub struct Browser {
    request: Request,
    dir: PathBuf,
    entries: Result<Vec<Entry>, String>,
    selected: BTreeSet<PathBuf>,
    /// [`Kind::Save`]: the file name.
    file_name: String,
    places: Vec<(String, PathBuf)>,
    /// Shared storage, where Android hides files from an app without the media permission.
    shared: Option<PathBuf>,
}

/// How the dialog ended.
pub enum Done {
    /// The chosen paths (a save: the one file to write).
    Picked(Vec<String>),
    Cancelled,
}

impl Browser {
    /// A dialog for `request`, opened at `start` (the last folder used, or a place) unless the
    /// request names a folder.
    pub fn open(request: Request, start: PathBuf, places: Vec<(String, PathBuf)>, shared: Option<PathBuf>) -> Browser {
        let start = request.start.clone().filter(|p| p.is_dir()).unwrap_or(start);
        let file_name = request.file_name.clone();
        let mut b = Browser { request, dir: PathBuf::new(), entries: Ok(Vec::new()), selected: BTreeSet::new(), file_name, places, shared };
        b.go(start);
        b
    }

    /// The folder shown.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn go(&mut self, dir: PathBuf) {
        self.entries = list(&dir, &self.request.exts, self.request.kind != Kind::Folder);
        self.dir = dir;
        self.selected.clear();
    }

    fn chosen(&self) -> Vec<String> {
        self.selected.iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }

    /// The save name with the dialog's extension when the user typed none.
    fn save_name(&self) -> String {
        let name = self.file_name.trim().to_string();
        match self.request.exts.first() {
            Some(ext) if !name.contains('.') => format!("{name}.{}", ext.trim_start_matches('.')),
            _ => name,
        }
    }

    /// Draw the dialog; `Some` when it closed.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Done> {
        let mut done = None;
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new(self.request.title.clone())
            .id(egui::Id::new("android.browser"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2((screen.width() * 0.8).min(900.0), (screen.height() * 0.85).min(700.0)))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 36.0;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 8.0);
                ui.horizontal_wrapped(|ui| {
                    for (name, path) in self.places.clone() {
                        if path.is_dir() && ui.selectable_label(self.dir == path, t(&name)).clicked() {
                            self.go(path);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let up = self.dir.parent().map(Path::to_path_buf);
                    if ui.add_enabled(up.is_some(), egui::Button::new("⬆")).clicked()
                        && let Some(up) = up
                    {
                        self.go(up);
                    }
                    ui.label(self.dir.to_string_lossy());
                });
                ui.separator();
                let list_height = ui.available_height() - 56.0;
                egui::ScrollArea::vertical().max_height(list_height.max(120.0)).auto_shrink([false, true]).show(ui, |ui| match &self.entries {
                    Err(e) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    Ok(entries) => {
                        if entries.is_empty() {
                            ui.weak(t("Nothing here"));
                        }
                        if self.request.kind != Kind::Folder
                            && let Some(hint) = hidden_files_hint(&self.dir, self.shared.as_deref(), entries)
                        {
                            ui.colored_label(ui.visuals().warn_fg_color, t(&hint));
                        }
                        let mut go = None;
                        for e in entries.clone() {
                            if e.is_dir {
                                if ui.add(egui::Button::new(format!("🗀  {}", e.name)).frame(false)).clicked() {
                                    go = Some(e.path);
                                }
                            } else {
                                let on = self.selected.contains(&e.path);
                                if ui.selectable_label(on, &e.name).clicked() {
                                    match self.request.kind {
                                        Kind::Files => {
                                            if on {
                                                self.selected.remove(&e.path);
                                            } else {
                                                self.selected.insert(e.path);
                                            }
                                        }
                                        Kind::Save => self.file_name = e.name,
                                        _ => {
                                            self.selected.clear();
                                            self.selected.insert(e.path);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(dir) = go {
                            self.go(dir);
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("Cancel")).clicked() {
                        done = Some(Done::Cancelled);
                    }
                    match self.request.kind {
                        Kind::Files | Kind::File => {
                            let n = self.selected.len();
                            let label = if n > 1 { format!("{} ({n})", t("Choose")) } else { t("Choose").to_string() };
                            if ui.add_enabled(n > 0, egui::Button::new(label)).clicked() {
                                done = Some(Done::Picked(self.chosen()));
                            }
                        }
                        Kind::Folder => {
                            if ui.add_enabled(self.entries.is_ok(), egui::Button::new(t("Choose This Folder"))).clicked() {
                                done = Some(Done::Picked(vec![self.dir.to_string_lossy().into_owned()]));
                            }
                        }
                        Kind::Save => {
                            ui.add(egui::TextEdit::singleline(&mut self.file_name).id(egui::Id::new("android.browser.name")).desired_width(260.0));
                            let name = self.save_name();
                            let ok = !name.starts_with('.') && name.len() > 1 && !name.contains('/') && self.entries.is_ok();
                            if ui.add_enabled(ok, egui::Button::new(t("Save"))).clicked() {
                                done = Some(Done::Picked(vec![self.dir.join(name).to_string_lossy().into_owned()]));
                            }
                        }
                    }
                });
            });
        if !open {
            done = Some(Done::Cancelled);
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fc-android-browser-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn exts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extensions_match_case_insensitively() {
        let media = exts(&["mp4", ".mov"]);
        assert!(admits("CLIP_0001.MP4", &media));
        assert!(admits("a.b.mov", &media), "a dotted filter extension matches too");
        assert!(!admits("notes.txt", &media));
        assert!(!admits("noext", &media));
        assert!(admits("anything", &[]));
    }

    #[test]
    fn folders_come_first_then_admitted_files_sorted_hidden_left_out() {
        let dir = temp("list");
        std::fs::create_dir(dir.join("b-folder")).unwrap();
        std::fs::create_dir(dir.join("A-folder")).unwrap();
        std::fs::create_dir(dir.join(".thumbnails")).unwrap();
        for f in ["zeta.MP4", "alpha.mp4", "notes.txt", ".hidden.mp4"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let names: Vec<String> =
            list(&dir, &exts(&["mp4"]), true).unwrap().into_iter().map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" })).collect();
        assert_eq!(names, ["A-folder/", "b-folder/", "alpha.mp4", "zeta.MP4"]);
        let folders: Vec<String> = list(&dir, &[], false).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(folders, ["A-folder", "b-folder"], "a folder pick lists no files");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_cannot_be_read_says_so() {
        let e = list(Path::new("/nonexistent/DCIM"), &[], true).unwrap_err();
        assert!(e.starts_with("/nonexistent/DCIM: "), "{e}");
        let denied = read_error(Path::new("/storage/emulated/0/DCIM"), &std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(denied.contains("Photos and videos") && denied.contains("/storage/emulated/0/DCIM"), "{denied}");
    }

    #[test]
    fn the_uis_dialogs_map_to_the_browsers_kinds() {
        let r = Request::from_dialog(FileDialog::SaveProject { name: "Untitled".into() });
        assert_eq!((r.kind, r.exts.as_slice(), r.file_name.as_str()), (Kind::Save, &["fcproj".to_string()][..], "Untitled"));
        let r = Request::from_dialog(FileDialog::Folder { at: Some("/storage/emulated/0/Movies".into()) });
        assert_eq!((r.kind, r.start.as_deref()), (Kind::Folder, Some(Path::new("/storage/emulated/0/Movies"))));
        let r = Request::from_dialog(FileDialog::OpenFiles { exts: exts(&["mp4", "wav"]) });
        assert_eq!((r.kind, r.exts.len()), (Kind::Files, 2));
        assert_eq!(Request::from_dialog(FileDialog::OpenProject).kind, Kind::File);
    }

    #[test]
    fn a_shared_folder_without_files_explains_the_missing_permission() {
        let shared = Path::new("/storage/emulated/0");
        let folder = |name: &str| Entry { name: name.into(), path: shared.join("DCIM").join(name), is_dir: true };
        let file = |name: &str| Entry { name: name.into(), path: shared.join("DCIM/Camera").join(name), is_dir: false };
        let hint = hidden_files_hint(&shared.join("DCIM"), Some(shared), &[folder("Camera")]);
        assert!(hint.is_some_and(|h| h.contains("Photos and videos")), "subfolders but no files: Android is hiding them");
        assert_eq!(hidden_files_hint(&shared.join("DCIM/Camera"), Some(shared), &[file("VID_0001.mp4")]), None, "files are shown: no hint");
        assert_eq!(hidden_files_hint(Path::new("/data/user/0/pkg/files/Projects"), Some(shared), &[]), None, "the app's own folder: nothing is hidden there");
        assert_eq!(hidden_files_hint(&shared.join("DCIM"), None, &[]), None, "no shared storage known");
    }

    #[test]
    fn a_save_gets_the_dialogs_extension_when_none_was_typed() {
        let dir = temp("save");
        let mut b = Browser::open(Request::from_dialog(FileDialog::SaveProject { name: "Cut 1".into() }), dir.clone(), Vec::new(), None);
        assert_eq!(b.save_name(), "Cut 1.fcproj");
        b.file_name = "cut.FCPROJ".into();
        assert_eq!(b.save_name(), "cut.FCPROJ", "a typed extension stays");
        assert_eq!(b.dir(), dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
