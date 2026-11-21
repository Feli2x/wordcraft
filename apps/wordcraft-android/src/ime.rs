//! The on-screen keyboard's text input, between `WordActivity.java` and WordCraft's own input-method
//! code.
//!
//! A plain NativeActivity gives the keyboard no `InputConnection`, so it sends bare key events (no
//! swipe typing, suggestions or autocorrection). `WordActivity` has one, and records what the
//! keyboard does (compose, commit, delete). Each frame this module takes those operations and
//! turns them into the [`egui::ImeEvent`]s WordCraft handles for every other platform's input
//! method, and reports the text around the caret back, so the keyboard sees the document's text.

use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{jni_sig, jni_str};

/// One thing the keyboard did, with the number `WordActivity` gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub seq: i32,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The word being composed (replaces the earlier one).
    Preedit(String),
    /// Composition ends with this text.
    Commit(String),
    /// Delete characters before and after the caret.
    Delete { before: usize, after: usize },
    /// The system's Back: WordCraft's Escape.
    Back,
}

impl Op {
    /// The events WordCraft handles for it.
    pub fn events(&self) -> Vec<egui::Event> {
        let ime = |e| vec![egui::Event::Ime(e)];
        match &self.kind {
            Kind::Preedit(text) => ime(egui::ImeEvent::Preedit { text: text.clone(), active_range_chars: None }),
            Kind::Commit(text) => ime(egui::ImeEvent::Commit(text.clone())),
            Kind::Delete { before, after } => ime(egui::ImeEvent::DeleteSurrounding { before_chars: *before, after_chars: *after }),
            Kind::Back => [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect(),
        }
    }
}

/// Parse what `WordActivity.imeDrain` returned: operations separated by U+0002, each
/// `kind U+0001 number U+0001 before U+0001 after U+0001 text`.
pub fn parse(encoded: &str) -> Vec<Op> {
    encoded
        .split('\u{2}')
        .filter(|s| !s.is_empty())
        .filter_map(|op| {
            let mut f = op.splitn(5, '\u{1}');
            let kind = f.next()?.parse::<u8>().ok()?;
            let seq = f.next()?.parse().ok()?;
            let a = f.next()?.parse().ok()?;
            let b = f.next()?.parse().ok()?;
            let text = f.next()?.to_string();
            let kind = match kind {
                0 => Kind::Preedit(text),
                1 => Kind::Commit(text),
                2 => Kind::Delete { before: a, after: b },
                3 => Kind::Back,
                _ => return None,
            };
            Some(Op { seq, kind })
        })
        .collect()
}

/// `s[..byte]` in UTF-16 units, as Java counts.
pub fn utf16_offset(s: &str, byte: usize) -> i32 {
    s.get(..byte).map_or(0, |p| p.encode_utf16().count() as i32)
}

/// Run `f` with the activity object and a JNI environment; `None` if anything fails.
///
/// # Safety
///
/// `activity` must be the running app's activity, `AndroidApp::activity_as_ptr()`, and the
/// `AndroidApp` must outlive the call.
unsafe fn with_activity<T>(
    activity: *mut std::ffi::c_void,
    f: impl FnOnce(&mut jni::Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Option<T> {
    if activity.is_null() {
        return None;
    }
    let vm = jni::JavaVM::singleton().ok()?;
    let raw = activity as jni::sys::jobject;
    let result: jni::errors::Result<T> = vm.attach_current_thread(|env| {
        env.with_local_frame(16, |env| {
            // SAFETY: an unowned global reference that stays valid while the `AndroidApp` lives
            // (the caller's promise); the cast neither owns nor deletes it.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            f(env, &activity)
        })
    });
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!("keyboard bridge failed: {e}");
            }
            None
        }
    }
}

static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The operations the keyboard made since the last call.
///
/// # Safety
///
/// As for [`with_activity`].
pub unsafe fn drain(activity: *mut std::ffi::c_void) -> Vec<Op> {
    // SAFETY: the caller's promise.
    let encoded = unsafe {
        with_activity(activity, |env, a| {
            let s = env.call_method(a, jni_str!("imeDrain"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
            let s = env.cast_local::<JString>(s)?;
            s.try_to_string(env)
        })
    };
    encoded.map(|e| parse(&e)).unwrap_or_default()
}

/// Tell the keyboard the text around the caret (`cursor` and `anchor` in UTF-16 units) and the last
/// operation applied.
///
/// # Safety
///
/// As for [`with_activity`].
pub unsafe fn set_state(activity: *mut std::ffi::c_void, text: &str, cursor: i32, anchor: i32, ack: i32) -> bool {
    // SAFETY: the caller's promise.
    unsafe {
        with_activity(activity, |env, a| {
            let t = env.new_string(text)?;
            env.call_method(
                a,
                jni_str!("imeState"),
                jni_sig!("(Ljava/lang/String;III)V"),
                &[jni::JValue::Object(&t), jni::JValue::Int(cursor), jni::JValue::Int(anchor), jni::JValue::Int(ack)],
            )?;
            Ok(())
        })
    }
    .is_some()
}

/// Show or hide the keyboard.
///
/// # Safety
///
/// As for [`with_activity`].
pub unsafe fn set_visible(activity: *mut std::ffi::c_void, show: bool) {
    // SAFETY: the caller's promise.
    unsafe {
        with_activity(activity, |env, a| {
            env.call_method(a, jni_str!("imeVisible"), jni_sig!("(Z)V"), &[jni::JValue::Bool(show)])?;
            Ok(())
        })
    };
}

/// Let the emulator test play keyboard operations (`WordActivity.imeEnableTest`).
///
/// # Safety
///
/// As for [`with_activity`].
pub unsafe fn enable_test(activity: *mut std::ffi::c_void) {
    // SAFETY: the caller's promise.
    unsafe {
        with_activity(activity, |env, a| {
            env.call_method(a, jni_str!("imeEnableTest"), jni_sig!("()V"), &[])?;
            Ok(())
        })
    };
}

/// The context `WordActivity.nativeWake` repaints: a keyboard operation is waiting.
pub static WAKE: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

/// `WordActivity.nativeWake()`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_wordcraft_android_WordActivity_nativeWake(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void) {
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_round_trip_in_order() {
        let ops = parse("0\u{1}1\u{1}0\u{1}0\u{1}Hal\u{2}1\u{1}2\u{1}0\u{1}0\u{1}Hallo \u{2}2\u{1}3\u{1}1\u{1}0\u{1}");
        assert_eq!(ops.len(), 3);
        assert_eq!(ops[0].kind, Kind::Preedit("Hal".into()));
        assert_eq!(ops[1], Op { seq: 2, kind: Kind::Commit("Hallo ".into()) });
        assert_eq!(ops[2].kind, Kind::Delete { before: 1, after: 0 });
        assert_eq!(parse("3\u{1}4\u{1}0\u{1}0\u{1}")[0].kind, Kind::Back);
        assert!(parse("").is_empty());
        assert!(parse("garbage").is_empty());
    }

    #[test]
    fn offsets_count_utf16_units() {
        // "é" is two bytes but one unit; an emoji is four bytes and two units.
        assert_eq!(utf16_offset("aé😀b", 1 + 2 + 4), 4);
    }
}
