//! WordCraft on Android.
//!
//! Runs the same [`wordcraft_ui_egui::WordApp`] as the desktop and web apps through eframe's
//! native runner on a `NativeActivity` (no Java code in the APK). Build the shared library with
//! `cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build -p wordcraft-android --release`
//! and package it with the Gradle project in `android/` (see `android/README.md`).
//!
//! Differences from the desktop app:
//! - Files: there is no system file dialog for a NativeActivity, so Open, Save As, Insert ›
//!   Pictures and Export use WordCraft's built-in file browser. It starts in the shared storage
//!   (`/storage/emulated/0`) when the app has "All files access", otherwise in the app's own
//!   folder (`Android/data/ai.storyteller.wordcraft.android/files`).
//! - No printing, no self-update, no language-pack downloads (no network use at all).
//! - Preferences (`ui.json`) and crash recovery live in the app's private data folder;
//!   preferences are written whenever they change, because Android may stop the app without
//!   running its exit path.
//! - System bars: the NativeActivity draws under the status and navigation bars, so the app is
//!   laid out in the activity's content rectangle (which also shrinks for the on-screen keyboard).
//! - Phones get their own layout (see `mobile`): the document wrapped to the screen with a bar of
//!   large buttons above it and a floating toolbar below; tablets keep the desktop interface.
//! - Scale: WordCraft's Auto interface size caps the scale so that 900x460 points fit the screen,
//!   which on a phone means desktop-sized controls on a 6-inch screen. Android's own density is
//!   used instead (the cap is lifted by hiding the monitor size); Options' Interface size still
//!   scales from there.
//! - Dark mode follows Android's (Settings › Display): the interface theme "System" gets Android's
//!   answer, and pages turn dark with it (View › Switch Modes still changes that by hand).
//! - Text input: hardware keyboards work fully; the on-screen keyboard is shown when a text field
//!   or the page takes focus, but a NativeActivity only receives plain key events from it (no
//!   autocorrect or swipe typing).

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod config;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod mobile;

#[cfg(target_os = "android")]
mod android {
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use eframe::egui;
    use serde_json::json;
    use winit::platform::android::activity::AndroidApp;
    use wordcraft_engine::Session;
    use wordcraft_ui_egui::file_dialogs::{FileDialogAnswer, FileDialogRequest};
    use wordcraft_ui_egui::{Services, UiState, WordApp};

    /// Where the app keeps its private state, and where the file browser starts.
    struct Folders {
        prefs: PathBuf,
        recovery: PathBuf,
    }

    /// Point HOME, TMPDIR and the XDG folders at places an Android app may write to, before
    /// anything reads them. Returns the private folders the shell itself uses.
    fn set_up_folders(app: &AndroidApp) -> Folders {
        let internal = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp/wordcraft"));
        let external = app.external_data_path();
        let tmp = internal.join("tmp");
        let config = internal.join("config");
        let data = internal.join("data");
        for d in [&tmp, &config, &data] {
            if let Err(e) = std::fs::create_dir_all(d) {
                log::warn!("cannot create {}: {e}", d.display());
            }
        }
        // The built-in file browser starts at HOME: the shared storage when "All files access"
        // is granted (Settings › Apps › WordCraft › Permissions), else the app's own folder,
        // which a USB connection or a file manager can still reach.
        let shared = Path::new("/storage/emulated/0");
        let home = if std::fs::read_dir(shared).is_ok() { shared.to_path_buf() } else { external.clone().unwrap_or_else(|| internal.clone()) };
        if let Some(ext) = &external {
            let _ = std::fs::create_dir_all(ext);
        }
        // SAFETY of set_var: called at the very start of android_main, before eframe or any of
        // our threads start, so nothing reads the environment concurrently.
        // (std::env::set_var is `unsafe` in edition 2024.)
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("TMPDIR", &tmp);
            std::env::set_var("XDG_CONFIG_HOME", &config);
            std::env::set_var("XDG_DATA_HOME", &data);
            std::env::set_var("XDG_CACHE_HOME", &tmp);
        }
        log::info!("HOME={} internal={}", home.display(), internal.display());
        Folders { prefs: config.join("ui.json"), recovery: internal.join("recovery") }
    }

    /// winit ignores Android's `onDestroy`, and android-activity's `onDestroy` waits until
    /// `android_main` has returned: any restart of the activity (a change of the system's colours
    /// or fonts, leaving with Back) froze the app until Android closed it as not responding
    /// (emulator run 29). This thread notices the destruction and ends the process instead; the
    /// recovery copies (every 10 s) keep the work, and Android starts the app again if it restarts
    /// the activity. `stop` ends the watch when `android_main` returns by itself.
    fn watch_for_destroy(android: AndroidApp, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        let spawned = std::thread::Builder::new().name("wordcraft-destroy".into()).spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(250));
                // SAFETY: `android` (a clone of the app's AndroidApp) is alive in this thread.
                if unsafe { crate::config::destroyed(android.activity_as_ptr()) } == Some(true) {
                    log::warn!("the activity is being destroyed and the event loop cannot end: ending the process");
                    // A restart for a configuration change (not leaving the app): start it again.
                    // SAFETY: as above.
                    if unsafe { crate::config::changing_configurations(android.activity_as_ptr()) } == Some(true) {
                        let asked = unsafe { crate::config::start_again(android.activity_as_ptr()) };
                        log::info!("restart for a configuration change; started again: {asked}");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    std::process::exit(0);
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("no destroy watch: {e}");
        }
    }

    /// A system property (`getprop`), empty when unset.
    fn prop(name: &str) -> String {
        std::process::Command::new("/system/bin/getprop")
            .arg(name)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    /// The Android emulator's Vulkan (software rendering through the host) hangs wgpu at start-up;
    /// its OpenGL ES works. Phones keep wgpu's own choice (Vulkan). `adb shell setprop
    /// debug.wordcraft.wgpu gl` (or `vulkan`) overrides the choice on any device.
    fn choose_graphics_backend() {
        let wanted = prop("debug.wordcraft.wgpu");
        let emulator = prop("ro.kernel.qemu") == "1" || prop("ro.boot.qemu") == "1" || prop("ro.hardware").contains("ranchu");
        let backend = if !wanted.is_empty() {
            Some(wanted)
        } else if emulator {
            Some("gl".to_string())
        } else {
            None
        };
        if let Some(backend) = backend {
            log::info!("graphics backend: {backend} (emulator: {emulator})");
            // SAFETY: as in set_up_folders, before eframe or any of our threads start.
            unsafe { std::env::set_var("WGPU_BACKEND", backend) };
        }
    }

    /// A NativeActivity has no system file dialog: answer every request with "unavailable", so
    /// the app shows its built-in file browser (#576), which then finishes the job.
    fn builtin_browser_hook() -> Box<dyn Fn(FileDialogRequest) -> mpsc::Receiver<FileDialogAnswer>> {
        Box::new(|_req| {
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(Err("Android has no system file dialog for this app".to_string()));
            rx
        })
    }

    fn services() -> Services {
        Services { file_dialog: Some(builtin_browser_hook()), ..Services::default() }
    }

    fn load_prefs(app: &mut WordApp, path: &Path) {
        if let Ok(bytes) = std::fs::read(path)
            && let Some(ui) = UiState::from_json(&bytes)
        {
            app.apply_prefs(ui);
        }
    }

    struct Shell {
        app: WordApp,
        /// The activity, for its content rectangle (the area the system bars leave free).
        android: AndroidApp,
        /// The phone layout around the app.
        mobile: crate::mobile::Mobile,
        /// Android's night mode as last applied, to switch the pages' dark mode when it changes.
        night: Option<bool>,
        /// The activity's current configuration (see `config.rs`), and when it was last read.
        current: Option<crate::config::Current>,
        last_night_check: f64,
        prefs_path: PathBuf,
        /// The preferences as last written, to write only when they change.
        saved_prefs: Option<String>,
        last_prefs_check: f64,
    }

    impl Shell {
        /// The part of `full` (the whole window, in points) the status bar, the navigation bar
        /// and the on-screen keyboard leave free: the activity's content rectangle (in pixels).
        fn safe_area(&self, full: egui::Rect, pixels_per_point: f32) -> egui::Rect {
            let r = self.android.content_rect();
            if r.right <= r.left || r.bottom <= r.top || pixels_per_point <= 0.0 {
                return full;
            }
            let ppp = pixels_per_point;
            let (w, h) = (full.width() * ppp, full.height() * ppp);
            let left = (r.left as f32).clamp(0.0, w) / ppp;
            let top = (r.top as f32).clamp(0.0, h) / ppp;
            let right = (w - r.right as f32).clamp(0.0, w) / ppp;
            let bottom = (h - r.bottom as f32).clamp(0.0, h) / ppp;
            let inner = egui::Rect::from_min_max(full.min + egui::vec2(left, top), full.max - egui::vec2(right, bottom));
            if inner.width() < 100.0 || inner.height() < 100.0 { full } else { inner }
        }

        /// For testing: one line with the build and what the app measures (scale, width, night
        /// mode, theme), so a screenshot shows which APK runs and why it looks as it does.
        fn diagnostics(&self, ui: &egui::Ui, inner: egui::Rect) {
            let ctx = ui.ctx().clone();
            let text = format!(
                "Build #{} | {:.2}x (System {:.2}x) | {:.0} pt breit | {} | {:?}",
                option_env!("GITHUB_RUN_NUMBER").unwrap_or("lokal"),
                ctx.pixels_per_point(),
                ctx.native_pixels_per_point().unwrap_or(0.0),
                inner.width(),
                if self.night == Some(true) { "Android: Nacht" } else { "Android: Tag" },
                ctx.theme(),
            );
            egui::Area::new(egui::Id::unique("android_diagnostics"))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .fixed_pos(inner.left_bottom() + egui::vec2(4.0, -16.0))
                .show(&ctx, |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .size(10.0)
                            .color(egui::Color32::from_rgb(200, 0, 0))
                            .background_color(egui::Color32::from_white_alpha(220)),
                    );
                });
        }

        fn save_prefs_if_changed(&mut self) {
            let Ok(json) = serde_json::to_string_pretty(&self.app.prefs()) else { return };
            if self.saved_prefs.as_deref() == Some(json.as_str()) {
                return;
            }
            let tmp = self.prefs_path.with_extension("json.tmp");
            let written = std::fs::write(&tmp, json.as_bytes()).and_then(|()| std::fs::rename(&tmp, &self.prefs_path));
            match written {
                Ok(()) => self.saved_prefs = Some(json),
                Err(e) => log::warn!("preferences: cannot save {}: {e}", self.prefs_path.display()),
            }
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.app.logic(ctx);
            // Android may kill a backgrounded app without `on_exit`: keep ui.json current.
            let now = ctx.input(|i| i.time);
            if now - self.last_prefs_check > 2.0 {
                self.last_prefs_check = now;
                self.save_prefs_if_changed();
                // For the emulator test: frames still run, and what Android's configuration says.
                if self.mobile.autotest {
                    log::info!("autotest alive night={:?} java={:?} applied={:?}", self.android.config().ui_mode_night(), self.current, self.night);
                }
            }
        }

        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            // Without a monitor size WordCraft's Auto size keeps Android's density (see above).
            if let Some(viewport) = raw.viewports.get_mut(&raw.viewport_id) {
                viewport.monitor_size = None;
            }
            // winit reports no theme on Android: give egui (and the "System" theme) Android's.
            let now = raw.time.unwrap_or(0.0);
            if self.current.is_none() || now - self.last_night_check > 1.0 {
                self.last_night_check = now;
                // SAFETY: the activity of `self.android`, which the Shell keeps alive.
                self.current = unsafe { crate::config::current(self.android.activity_as_ptr()) }.or(self.current);
            }
            let night = self.current.map_or_else(|| format!("{:?}", self.android.config().ui_mode_night()) == "Yes", |c| c.night);
            raw.system_theme = Some(if night { egui::Theme::Dark } else { egui::Theme::Light });
            if self.night != Some(night) {
                if self.mobile.autotest {
                    log::info!("autotest night {night}");
                }
                self.night = Some(night);
                let _ = self.app.run("view.darkMode", json!({"value": night}));
            }
            self.app.raw_input_hook(ctx, raw);
        }

        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let full = ui.max_rect();
            let inner = self.safe_area(full, ui.ctx().pixels_per_point());
            if inner != full {
                // Under the system bars: the window's own colour, so their icons stay legible.
                ui.painter().rect_filled(full, 0.0, ui.visuals().panel_fill);
            }
            // dp are egui points here (WordCraft's scale follows Android's density, see above).
            let smallest = self.current.map(|c| c.smallest_width_dp as f32);
            self.mobile.ui(&mut self.app, ui, inner, smallest);
            if self.mobile.autotest {
                self.diagnostics(ui, inner);
            }
            // The content rectangle changes without an input event (keyboard shown, rotation).
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }

        fn on_exit(&mut self) {
            self.app.session.finish_recovery();
            self.save_prefs_if_changed();
        }
    }

    #[unsafe(no_mangle)]
    fn android_main(android: AndroidApp) {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("WordCraft"));
        log::info!("WordCraft Android build #{}", option_env!("GITHUB_RUN_NUMBER").unwrap_or("local"));
        choose_graphics_backend();
        let folders = set_up_folders(&android);
        // `adb shell setprop debug.wordcraft.autotest 1` (android/test) logs what the test checks.
        let autotest = prop("debug.wordcraft.autotest") == "1";
        let activity = android.clone();
        let stop_watch = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        watch_for_destroy(android.clone(), stop_watch.clone());
        let options = eframe::NativeOptions { android_app: Some(android), ..Default::default() };
        log::info!("start-up: starting eframe");
        let result = eframe::run_native(
            "WordCraft",
            options,
            Box::new(move |cc| {
                log::info!("start-up: window and graphics ready, creating the app");
                let region = wordcraft_ui_egui::i18n::system_region();
                let mut session = Session::new(wordcraft_engine::blank_document(&region));
                session.system_region = region;
                let mut app = WordApp::new(session, services());
                log::info!("start-up: app created");
                // No network use on Android: the update check's TLS verifier needs Java-side
                // set-up a NativeActivity doesn't have.
                app.disable_self_update();
                load_prefs(&mut app, &folders.prefs);
                if app.session.prefs.region.is_some() {
                    let doc = app.session.blank_document();
                    app.replace_startup_document(doc);
                }
                let ctx = cc.egui_ctx.clone();
                if let Err(e) = app.session.enable_recovery(folders.recovery.clone(), std::sync::Arc::new(move || ctx.request_repaint())) {
                    log::warn!("local recovery unavailable: {e}");
                }
                app.start_screen();
                let saved_prefs = serde_json::to_string_pretty(&app.prefs()).ok();
                Ok(Box::new(Shell {
                    app,
                    android: activity,
                    mobile: crate::mobile::Mobile::new(autotest),
                    night: None,
                    current: None,
                    last_night_check: 0.0,
                    prefs_path: folders.prefs.clone(),
                    saved_prefs,
                    last_prefs_check: 0.0,
                }))
            }),
        );
        stop_watch.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = result {
            log::error!("WordCraft could not start: {e}");
        }
    }
}
