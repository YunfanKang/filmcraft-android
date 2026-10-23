//! Android runtime permissions through JNI: the NDK has no C API for `Activity.requestPermissions`
//! and `checkSelfPermission`, so this crate calls the Java ones. It is the second crate allowed
//! `unsafe` (AGENTS.md §0, with `crates/platform`): the two `unsafe` blocks only wrap the JavaVM
//! and activity pointers `android-activity` hands out, with `// SAFETY:` comments; everything else
//! goes through the `jni` crate's checked calls, and a Java exception becomes an `Err`, never an
//! abort. Off Android only the pure part (which permissions to ask for) exists.
//!
//! The answer to a request arrives in Java only (`onRequestPermissionsResult`, which
//! `NativeActivity` does not forward), so a host asks [`granted`] again afterwards.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub const READ_MEDIA_VIDEO: &str = "android.permission.READ_MEDIA_VIDEO";
pub const READ_MEDIA_IMAGES: &str = "android.permission.READ_MEDIA_IMAGES";
pub const READ_MEDIA_AUDIO: &str = "android.permission.READ_MEDIA_AUDIO";
/// Before Android 13 (API 33), the one permission for shared storage.
pub const READ_EXTERNAL_STORAGE: &str = "android.permission.READ_EXTERNAL_STORAGE";

/// The permissions that let FilmCraft read footage, photos and music in shared storage on a
/// device at API level `sdk`: the granular media permissions from Android 13 on (as the manifest
/// declares them), the external-storage one before.
pub fn media_permissions(sdk: i32) -> &'static [&'static str] {
    if sdk >= 33 { &[READ_MEDIA_VIDEO, READ_MEDIA_IMAGES, READ_MEDIA_AUDIO] } else { &[READ_EXTERNAL_STORAGE] }
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod jni_calls;
#[cfg(target_os = "android")]
pub use jni_calls::{granted, request};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granular_media_permissions_from_android_13() {
        assert_eq!(media_permissions(34), [READ_MEDIA_VIDEO, READ_MEDIA_IMAGES, READ_MEDIA_AUDIO]);
        assert_eq!(media_permissions(33), [READ_MEDIA_VIDEO, READ_MEDIA_IMAGES, READ_MEDIA_AUDIO]);
        assert_eq!(media_permissions(32), [READ_EXTERNAL_STORAGE]);
        assert_eq!(media_permissions(26), [READ_EXTERNAL_STORAGE]);
    }
}
