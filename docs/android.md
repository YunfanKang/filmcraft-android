# FilmCraft on Android

`apps/filmcraft-android` hosts the same egui UI as the desktop app (`crates/ui-egui`) in an
Android `NativeActivity`, through eframe's winit backend, drawn with wgpu on Vulkan. Everything
below the window (engine, decoders, render, export, project files) is the desktop's code,
unchanged: the browser build already proved it has no operating-system assumptions, and Android
keeps native threads, so there is no worker layer like the web host's.

**Status: a tablet build.** Landscape tablets (a smallest width of 600 dp, the manifest's
requirement) fit the desktop layout; the host adds auto-save and preferences on the device, a file
browser in place of the native dialogs, cpal audio output, finger-sized controls and logcat
logging. Video decoding is FilmCraft's own software decoders (hardware decoding through
MediaCodec is listed under [Status](#status)). There is no phone layout.

## Check it compiles

```sh
rustup target add aarch64-linux-android
cargo xtask android        # cargo check --target aarch64-linux-android -p filmcraft-android --lib
```

This is part of `cargo xtask ci`. A check links nothing, so no NDK is needed: it catches
`cfg(target_os)` gaps, dependencies that do not build for Android, and uses of APIs the platform
lacks, on any machine. The target is 64-bit ARM only (every phone and tablet since 2019; the one
ABI Google Play accepts on its own).

## Build the APK

```sh
cargo xtask apk              # → <target>/android/filmcraft-<version>-arm64-v8a.apk
cargo xtask apk --install    # … and `adb install -r` it on the connected device
cargo xtask apk --dev        # debug profile (faster to build, slow to run)
```

It needs the Android SDK with a platform, build-tools and an NDK, and a JDK's `keytool` on `PATH`
(for the debug signing key it makes once, in `<target>/android/debug.keystore`). One-time setup
with the SDK's command-line tools:

```sh
export ANDROID_HOME=$HOME/android-sdk
sdkmanager --sdk_root=$ANDROID_HOME "platforms;android-34" "build-tools;34.0.0" "ndk;27.2.12479018"
```

`ANDROID_NDK_HOME` names an NDK elsewhere; without it the newest under `<sdk>/ndk/` is used. The
build is `cargo build --target aarch64-linux-android --release` with the NDK's clang as the linker
(nothing of ours is C: the NDK only links against the platform's `libc`, `libandroid`, `liblog`
and `libaaudio`), then the SDK's own `aapt2`, `aapt`, `zipalign` and `apksigner` around
`packaging/android/AndroidManifest.xml` and the launcher icon from `assets/app-icon/`; the library
goes in stripped (`llvm-strip`; the symbols stay in `target/` for a logcat trace). No Gradle, no
Java of our own. `CRAFT_FONTS_DIR` works as for every other build.

**First run on the device.** The app opens on the demo project. To import your own footage, allow
*Photos and videos* (older Android: *Files and media*) for FilmCraft in Android Settings ▸ Apps ▸
FilmCraft ▸ Permissions: the host declares the permission but can't show the prompt yet (that
needs JNI, below). Then File ▸ Import browses Camera (DCIM), Movies, Pictures and Download.

## Where things are on the device

| | Path | Needs permission |
|---|---|---|
| Auto-save journal, crash recovery, `preferences.json` | `/data/user/0/ai.storyteller.filmcraft/files/filmcraft/` (the activity's `internal_data_path`) | no |
| Projects (Save's default folder) | `/storage/emulated/0/Android/data/ai.storyteller.filmcraft/files/Projects/` (`external_data_path`; internal storage on a device without it) | no; visible over USB |
| Exports (default folder) | `…/files/Export/` beside the projects | no; visible over USB |
| Camera roll, Movies, Pictures, Download | `/storage/emulated/0/DCIM` etc. | *Photos and videos* |

Imported media is referenced where it is, as on the desktop (a project links to its files; nothing
is copied). Uninstalling the app removes the projects and exports (Android deletes app-specific
storage), so copy an export out over USB or with a file manager that can open `Android/data`.

## How the host differs from the desktop

| | Desktop (`apps/filmcraft`) | Android (`apps/filmcraft-android`) |
|---|---|---|
| Entry | `fn main`, `eframe::run_native` | `android_main(AndroidApp)` (called by `android-activity` on its own thread), `eframe::run_native` with `NativeOptions::android_app` |
| Window backend | winit + Wayland/X11/AppKit/Win32 | winit's `android-native-activity` (pure Rust; the GameActivity backend compiles C++ and is not used) |
| Accessibility | AccessKit | none: AccessKit has no NativeActivity adapter, and eframe rejects `accesskit` with it |
| Data folder | `~/.local/share/filmcraft` etc. (`engine::autosave::default_data_dir`) | the activity's private data directory, given to `AutosaveConfig::new` (Android has no `$HOME`) |
| Audio output | cpal (ALSA, CoreAudio, WASAPI) | the same `filmcraft-audio-cpal` crate; cpal's Android backend is AAudio |
| Voice-over input | cpal | none (the microphone permission's prompt needs JNI) |
| Hardware decoding | `filmcraft-platform` (VideoToolbox, Media Foundation, VA-API, NVDEC) | none yet: software decoders |
| File dialogs | `rfd`, on a thread on Linux | the host's own egui window (`browser.rs`) over the app, answering `HostHooks::run_dialog`; it offers Projects, Exports, Camera, Movies, Pictures, Downloads and Storage |
| Controls | pointer-sized | an interface zoom chosen for the screen: larger (up to 1.25×) only while at least 1180 × 700 points remain, which the header needs; smaller (down to 0.8×) on a narrower screen so its parts don't overlap; the soft keyboard opens for text fields (winit) |
| System bars | the window manager keeps the window clear of them | the status bar is hidden (`FULLSCREEN` window flag: devices that lay the window out edge to edge put the menu bar under the clock) and the screen stays on while FilmCraft is up; the usable area the activity reports (`content_rect`, e.g. above the soft keyboard) is what egui lays out in |
| Logging | file + stderr (`logging.rs`) | logcat, tag `filmcraft` (`android_logger`) |
| Menus, shortcuts | native menu bar (macOS), keyboard | the in-app menu bar; J/K/L and the other shortcuts work with a hardware keyboard |

## Status

What has to land before the tablet build is comfortable, roughly in order:

1. **Hardware decoding.** Software H.264 at 1080p plays; HEVC and 4K are heavy for a tablet CPU.
   Android's MediaCodec has a C API in the NDK (`AMediaCodec`, `libmediandk`), which the `ndk`
   crate binds in pure Rust, so an Android backend belongs in `crates/platform` next to the others,
   behind the same `codecs::VideoDecoder` with transparent fallback. Needs a device to test on.
2. **Memory.** Decoders, the frame cache and the GPU compositor sized for a desktop; measure on a
   device before raising defaults, and expect to lower the play-ahead.
3. **The permission prompt, the share sheet and system-bar insets** need JNI
   (`Activity.requestPermissions`, `ACTION_SEND`, MediaStore inserts so exports show in Gallery,
   `WindowInsets` so the status bar can stay visible with the interface laid out below it). The
   `jni` crate is pure Rust, but calling into the JVM takes `unsafe`, so it goes into
   `crates/platform` (`// SAFETY:` on every block) and needs a device: a wrong signature aborts
   the JVM.
4. **A compact layout for phones.** The UI assumes 900 × 560 points with panels, timeline and
   menu bar; a phone in portrait is about 400 points wide.
5. **Release packaging**: a release signing key, `release.yml`, a Play listing.
