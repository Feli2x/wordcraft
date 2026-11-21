// Packages the Rust library (libwordcraft_android.so, built by cargo-ndk into
// src/main/jniLibs/<abi>/) into an APK. The only Java is WordActivity, a NativeActivity (which
// loads the library and calls `android_main`) with a text-input connection for the keyboard.
plugins {
    id("com.android.application")
}

android {
    namespace = "ai.storyteller.wordcraft.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "ai.storyteller.wordcraft.android"
        minSdk = 26
        // 34, not 35: Android 15 forces edge-to-edge drawing for apps targeting 35, which would
        // put WordCraft's ribbon under the status bar (winit has no safe-area insets on Android).
        targetSdk = 34
        // Each CI run numbers its APK, so an install over an older one counts as an update.
        versionCode = (System.getenv("GITHUB_RUN_NUMBER") ?: "1").toInt()
        versionName = "0.18.1-android"
        ndk {
            // arm64 for phones; the emulator test builds x86_64 (gradle -Pabis=x86_64).
            abiFilters += ((project.findProperty("abis") as String?) ?: "arm64-v8a").split(",")
        }
    }

    // One fixed key for every build (android/wordcraft.keystore, kept in the Actions cache by the
    // workflow), so a new APK installs over the old one: Android refuses an update signed with
    // another key, and the runner's debug key is new on every run. A sideloading key, not a store's.
    signingConfigs {
        create("fork") {
            storeFile = rootProject.file("wordcraft.keystore")
            storePassword = "wordcraft-android"
            keyAlias = "wordcraft"
            keyPassword = "wordcraft-android"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName(if (rootProject.file("wordcraft.keystore").exists()) "fork" else "debug")
        }
    }

    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }
}
