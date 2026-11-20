# WordCraft für Android (experimentell)

Dieser Ordner verpackt `apps/wordcraft-android` (Rust, eframe/egui auf einer `NativeActivity`) als APK für arm64-Geräte (Android 8.0+).

## APK über GitHub Actions bauen

1. Repository forken und im Fork unter **Actions** die Workflows aktivieren.
2. Diese Änderungen in den Fork übernehmen (beliebiger Branch): Der Workflow startet von selbst, sobald Dateien unter `android/` oder `apps/wordcraft-android/` gepusht werden; sonst **Actions → Android APK → Run workflow**.
3. Nach dem Lauf (erstmals ca. 30–60 min) unter **Artifacts** `wordcraft-android-apk` herunterladen, entpacken und `app-release.apk` auf dem Gerät installieren („Installation aus unbekannten Quellen“ erlauben).

## Lokal bauen

Voraussetzungen: Rust ≥ 1.95 mit `rustup target add aarch64-linux-android`, Android SDK + NDK, `cargo install cargo-ndk`, JDK 17, Gradle 8.11.

```sh
export ANDROID_NDK_HOME=/pfad/zum/ndk
export CRAFT_I18N_DIR=/pfad/zu/craft-i18n   # Checkout von storytold/craft-i18n @ $(cat craft-i18n.rev)
cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build -p wordcraft-android --release
cd android && gradle assembleRelease
adb install -r app/build/outputs/apk/release/app-release.apk
```

## Nach der Installation

- **Dateizugriff:** Einstellungen › Apps › WordCraft › Berechtigungen › Dateien und Medien › „Verwaltung aller Dateien zulassen“. Danach startet der eingebaute Dateibrowser im gemeinsamen Speicher (`/storage/emulated/0`), sonst nur im App-Ordner `Android/data/ai.storyteller.wordcraft.android/files`.
- **Schriften:** Android hat keine metrikkompatiblen Office-Schriften (Carlito, Liberation, Caladea). Für originalgetreues Layout von .docx-Dateien diese `.ttf`-Dateien in den Ordner `fonts/` im Startordner kopieren.
- **Logs:** `adb logcat -s WordCraft`

## Bekannte Einschränkungen

- Bildschirmtastatur: Eine `NativeActivity` erhält nur einfache Tastenereignisse (keine Autokorrektur, kein Wischen, Umlaute je nach Tastatur-App). Eine Hardware-Tastatur funktioniert vollständig.
- Kein Systemdateidialog (eingebauter Browser stattdessen), kein Drucken, keine Updates, keine Sprachpaket-Downloads.
- Die Oberfläche ist für Maus und großen Bildschirm gebaut; auf Tablets mit Tastatur ist sie brauchbarer als auf Smartphones.
- „ArtCraft“ und „WordCraft“ sind Marken von ArtCraft und nicht von der MIT/Apache-Lizenz gedeckt: für die Weitergabe eines eigenen Builds bitte umbenennen.
