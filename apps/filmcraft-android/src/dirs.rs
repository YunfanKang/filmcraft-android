//! Where FilmCraft keeps its data on the device, the folders the user can browse, and the
//! interface zoom for the screen. Pure: the host feeds in what the activity reports.

use std::path::{Path, PathBuf};

/// The smallest window the desktop layout is comfortable in, in points: the header (workspace
/// tabs, the menu bar, the title, Discord, the workspace switcher and the icons) needs about
/// 1180 points before its parts run into each other, and the three-row dock about 700 points of
/// height. (The desktop's minimum window, 900 × 560, lets the header overlap.)
pub const COMFORTABLE_SIZE: [f32; 2] = [1180.0, 700.0];

/// The least the interface is shrunk to on a screen narrower than [`COMFORTABLE_SIZE`].
pub const MIN_ZOOM: f32 = 0.8;

/// This install's folders. All in the app's own storage, which needs no permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// The data folder (`engine::autosave`): auto-save journal, crash recovery, `preferences.json`,
    /// graphics-start state. The activity's private data directory.
    pub data: PathBuf,
    /// Where projects are saved by default. In the app's external storage when the device has it
    /// (`Android/data/<package>/files/Projects`, visible over USB), else internal.
    pub projects: PathBuf,
    /// The default export folder, beside the projects.
    pub export: PathBuf,
    /// Shared storage (`/storage/emulated/0`), where DCIM, Movies, Pictures and Download are.
    /// Reading it needs the media permission (granted in Android's app settings; docs/android.md).
    pub shared: Option<PathBuf>,
}

impl Dirs {
    /// From the activity's `internal_data_path` and `external_data_path`.
    pub fn of(internal: &Path, external: Option<&Path>) -> Dirs {
        let files = external.unwrap_or(internal);
        Dirs { data: internal.join("filmcraft"), projects: files.join("Projects"), export: files.join("Export"), shared: external.and_then(shared_storage) }
    }
}

/// `/storage/emulated/0` from `/storage/emulated/0/Android/data/<package>/files`: the folder whose
/// child is `Android`.
pub fn shared_storage(external: &Path) -> Option<PathBuf> {
    let mut dir = external;
    while let Some(parent) = dir.parent() {
        if dir.file_name().is_some_and(|n| n == "Android") {
            return Some(parent.to_path_buf());
        }
        dir = parent;
    }
    None
}

/// The places the file browser offers, in order (the app's own folders first, then shared
/// storage). The browser shows those that exist.
pub fn places(dirs: &Dirs) -> Vec<(String, PathBuf)> {
    let mut v = vec![("Projects".to_string(), dirs.projects.clone()), ("Exports".to_string(), dirs.export.clone())];
    if let Some(s) = &dirs.shared {
        for (name, sub) in [("Camera", "DCIM"), ("Movies", "Movies"), ("Pictures", "Pictures"), ("Downloads", "Download")] {
            v.push((name.to_string(), s.join(sub)));
        }
        v.push(("Storage".to_string(), s.clone()));
    }
    v
}

/// The interface zoom for a screen of `width` × `height` points: bigger controls for fingers as
/// long as the desktop layout stays comfortable ([`COMFORTABLE_SIZE`]), and on a screen too narrow
/// for it, smaller (down to [`MIN_ZOOM`]) so the header's parts don't overlap.
pub fn tablet_zoom(width: f32, height: f32) -> f32 {
    let [min_w, min_h] = COMFORTABLE_SIZE;
    let fits = |z: f32| width / z >= min_w && height / z >= min_h;
    if let Some(z) = [1.25, 1.1, 1.0].into_iter().find(|z| fits(*z)) {
        return z;
    }
    // (two decimals: a zoom the user can read in View ▸ Zoom)
    ((width / min_w).min(height / min_h).clamp(MIN_ZOOM, 1.0) * 100.0).floor() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERNAL: &str = "/data/user/0/ai.storyteller.filmcraft/files";
    const EXTERNAL: &str = "/storage/emulated/0/Android/data/ai.storyteller.filmcraft/files";

    #[test]
    fn projects_and_exports_go_to_external_storage_when_there_is_one() {
        let d = Dirs::of(Path::new(INTERNAL), Some(Path::new(EXTERNAL)));
        assert_eq!(d.data, Path::new("/data/user/0/ai.storyteller.filmcraft/files/filmcraft"), "auto-save and preferences stay internal");
        assert_eq!(d.projects, Path::new(&format!("{EXTERNAL}/Projects")));
        assert_eq!(d.export, Path::new(&format!("{EXTERNAL}/Export")));
        assert_eq!(d.shared.as_deref(), Some(Path::new("/storage/emulated/0")));
    }

    #[test]
    fn without_external_storage_everything_is_internal_and_nothing_is_shared() {
        let d = Dirs::of(Path::new(INTERNAL), None);
        assert_eq!(d.projects, Path::new(&format!("{INTERNAL}/Projects")));
        assert_eq!(d.shared, None);
        assert_eq!(places(&d).len(), 2, "only the app's own folders");
    }

    #[test]
    fn shared_storage_is_the_parent_of_android() {
        assert_eq!(shared_storage(Path::new(EXTERNAL)).as_deref(), Some(Path::new("/storage/emulated/0")));
        assert_eq!(shared_storage(Path::new("/mnt/expand/abc/user/0/pkg/files")), None, "no Android folder above it");
    }

    #[test]
    fn places_list_the_camera_roll_and_movies_after_the_apps_folders() {
        let d = Dirs::of(Path::new(INTERNAL), Some(Path::new(EXTERNAL)));
        let p = places(&d);
        let names: Vec<&str> = p.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Projects", "Exports", "Camera", "Movies", "Pictures", "Downloads", "Storage"]);
        assert_eq!(p[2].1, Path::new("/storage/emulated/0/DCIM"));
    }

    #[test]
    fn zoom_grows_controls_only_while_the_header_stays_comfortable() {
        assert_eq!(tablet_zoom(1280.0, 800.0), 1.0, "a 10-inch tablet at 1.5× density: 1.25 would squeeze the header to 1024 points");
        assert_eq!(tablet_zoom(1600.0, 1000.0), 1.25, "a 13-inch tablet at 1.5× density");
        assert_eq!(tablet_zoom(1340.0, 800.0), 1.1);
    }

    #[test]
    fn a_narrow_screen_shrinks_the_interface_instead_of_overlapping() {
        assert_eq!(tablet_zoom(1024.0, 640.0), 0.86, "1024 / 1180");
        assert_eq!(tablet_zoom(1180.0, 600.0), 0.85, "the height decides: 600 / 700");
        assert_eq!(tablet_zoom(640.0, 400.0), MIN_ZOOM, "never smaller than readable");
        assert_eq!(tablet_zoom(800.0, 1280.0), MIN_ZOOM, "portrait: the layout doesn't fit anyway");
    }
}
