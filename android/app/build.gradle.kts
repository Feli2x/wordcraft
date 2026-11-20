// Packages the Rust library (libwordcraft_android.so, built by cargo-ndk into
// src/main/jniLibs/<abi>/) into an APK. There is no Java or Kotlin code: the manifest starts
// Android's own NativeActivity, which loads the library and calls `android_main`.
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
        versionCode = 1
        versionName = "0.18.1-android"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with the debug key so the APK installs directly (sideloading, not the Play Store).
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }
}
