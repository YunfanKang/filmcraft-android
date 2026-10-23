//! FilmCraft on Android: the same egui UI as the desktop app (`crates/ui-egui`), hosted in a
//! `NativeActivity` through eframe's winit backend and drawn with wgpu (Vulkan). A tablet in
//! landscape fits the desktop layout; the host adds what a tablet lacks around it:
//! - auto-save, crash recovery and preferences in the app's own storage, projects and exports in a
//!   folder visible over USB ([`dirs`]);
//! - a file browser in place of the native dialogs, for Import, Open, Save and Export
//!   ([`browser`]); shared storage (the camera roll, Movies) needs the media permission;
//! - audio output through cpal's AAudio backend (`filmcraft-audio-cpal`, as on the desktop);
//! - finger-sized controls (an interface zoom chosen for the screen), the soft keyboard (winit
//!   shows it for text fields), the status bar hidden and the usable area the activity reports
//!   honoured ([`insets`]: no control ends up under a system bar);
//! - the `log` crate on logcat (tag `filmcraft`).
//!
//! `cargo xtask android` checks that the crate compiles for `aarch64-linux-android`;
//! `cargo xtask apk` builds the APK (docs/android.md). What is still missing (hardware decoding
//! through MediaCodec, the permission prompt, a share sheet, a phone layout) is listed there.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod browser;
pub mod dirs;
pub mod insets;

#[cfg(target_os = "android")]
mod host;

/// The one-line reason the window can't open, for logcat.
pub fn start_failure(e: &dyn std::fmt::Display) -> String {
    format!("FilmCraft could not open its window: {e}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn start_failure_names_the_cause() {
        assert_eq!(super::start_failure(&"no Vulkan device"), "FilmCraft could not open its window: no Vulkan device");
    }
}
