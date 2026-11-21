//! The activity's current `Configuration` (night mode, smallest screen width), read through JNI.
//!
//! android-activity's own configuration is read from the asset manager the NativeActivity was
//! created with. When the dark theme is switched while the app runs, Android gives the activity's
//! resources a new asset manager, so that configuration keeps reporting the old mode (the
//! emulator test showed `ui_mode_night() == No` for minutes after `cmd uimode night yes`); the
//! same goes for a move to a Samsung DeX screen or into split screen.

use jni::objects::JObject;
use jni::refs::Global;
use jni::{jni_sig, jni_str};

/// `Configuration.UI_MODE_NIGHT_MASK` and `UI_MODE_NIGHT_YES`.
const NIGHT_MASK: i32 = 0x30;
const NIGHT_YES: i32 = 0x20;

/// Whether a failure was logged already (it would repeat every second).
static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the phone layout and the theme need from the configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Current {
    /// The system's dark theme is on.
    pub night: bool,
    /// The window's shorter side in dp, whichever way the device is turned (`smallestScreenWidthDp`):
    /// a phone stays a phone in landscape and with the keyboard open, a DeX window is a desktop.
    pub smallest_width_dp: i32,
}

/// The activity's current configuration; `None` if it cannot be read.
///
/// # Safety
///
/// `activity` must be the running app's activity reference, `AndroidApp::activity_as_ptr()`,
/// and the `AndroidApp` must outlive the call.
pub unsafe fn current(activity: *mut std::ffi::c_void) -> Option<Current> {
    if activity.is_null() {
        return None;
    }
    let vm = jni::JavaVM::singleton().ok()?;
    let raw = activity as jni::sys::jobject;
    let read: jni::errors::Result<(i32, i32)> = vm.attach_current_thread(|env| {
        env.with_local_frame(8, |env| {
            // SAFETY: an unowned global reference that stays valid while the `AndroidApp` lives
            // (the caller's promise); the cast neither owns nor deletes it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let resources = env.call_method(&*activity, jni_str!("getResources"), jni_sig!("()Landroid/content/res/Resources;"), &[])?.l()?;
            let config = env.call_method(&resources, jni_str!("getConfiguration"), jni_sig!("()Landroid/content/res/Configuration;"), &[])?.l()?;
            let ui_mode = env.get_field(&config, jni_str!("uiMode"), jni_sig!("I"))?.i()?;
            let smallest = env.get_field(&config, jni_str!("smallestScreenWidthDp"), jni_sig!("I"))?.i()?;
            Ok((ui_mode, smallest))
        })
    });
    match read {
        Ok((ui_mode, smallest)) => Some(Current { night: ui_mode & NIGHT_MASK == NIGHT_YES, smallest_width_dp: smallest }),
        Err(e) => {
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!("configuration unreadable: {e}");
            }
            None
        }
    }
}

/// Whether Android has begun destroying the activity (`Activity.isDestroyed()`, already true while
/// `onDestroy` runs); `None` if it cannot be read.
///
/// # Safety
///
/// As for [`current`].
pub unsafe fn destroyed(activity: *mut std::ffi::c_void) -> Option<bool> {
    if activity.is_null() {
        return None;
    }
    let vm = jni::JavaVM::singleton().ok()?;
    let raw = activity as jni::sys::jobject;
    let read: jni::errors::Result<bool> = vm.attach_current_thread(|env| {
        env.with_local_frame(4, |env| {
            // SAFETY: as in `current`.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            env.call_method(&*activity, jni_str!("isDestroyed"), jni_sig!("()Z"), &[])?.z()
        })
    });
    read.ok()
}

/// Whether the activity is being destroyed only to be created again for a configuration change
/// (`Activity.isChangingConfigurations()`); `None` if it cannot be read.
///
/// # Safety
///
/// As for [`current`].
pub unsafe fn changing_configurations(activity: *mut std::ffi::c_void) -> Option<bool> {
    if activity.is_null() {
        return None;
    }
    let vm = jni::JavaVM::singleton().ok()?;
    let raw = activity as jni::sys::jobject;
    let read: jni::errors::Result<bool> = vm.attach_current_thread(|env| {
        env.with_local_frame(4, |env| {
            // SAFETY: as in `current`.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            env.call_method(&*activity, jni_str!("isChangingConfigurations"), jni_sig!("()Z"), &[])?.z()
        })
    });
    read.ok()
}

/// Ask Android to start the app's launcher activity afresh in a new task (it starts once this
/// process has ended). True when the request was made.
///
/// # Safety
///
/// As for [`current`].
pub unsafe fn start_again(activity: *mut std::ffi::c_void) -> bool {
    /// `Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK`.
    const FLAGS: i32 = 0x1000_0000 | 0x0000_8000;
    if activity.is_null() {
        return false;
    }
    let Ok(vm) = jni::JavaVM::singleton() else { return false };
    let raw = activity as jni::sys::jobject;
    let started: jni::errors::Result<()> = vm.attach_current_thread(|env| {
        env.with_local_frame(8, |env| {
            // SAFETY: as in `current`.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let context = env.call_method(&*activity, jni_str!("getApplicationContext"), jni_sig!("()Landroid/content/Context;"), &[])?.l()?;
            let packages = env.call_method(&context, jni_str!("getPackageManager"), jni_sig!("()Landroid/content/pm/PackageManager;"), &[])?.l()?;
            let name = env.call_method(&context, jni_str!("getPackageName"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
            let intent = env
                .call_method(
                    &packages,
                    jni_str!("getLaunchIntentForPackage"),
                    jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                    &[jni::JValue::Object(&name)],
                )?
                .l()?;
            env.call_method(&intent, jni_str!("addFlags"), jni_sig!("(I)Landroid/content/Intent;"), &[jni::JValue::Int(FLAGS)])?;
            env.call_method(&context, jni_str!("startActivity"), jni_sig!("(Landroid/content/Intent;)V"), &[jni::JValue::Object(&intent)])?;
            Ok(())
        })
    });
    match started {
        Ok(()) => true,
        Err(e) => {
            log::warn!("cannot start the app again: {e}");
            false
        }
    }
}
