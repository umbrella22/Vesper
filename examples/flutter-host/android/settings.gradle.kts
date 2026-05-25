pluginManagement {
    val androidGradlePluginVersion = "8.11.1"

    val flutterSdkPath =
        run {
            val properties = java.util.Properties()
            file("local.properties").inputStream().use { properties.load(it) }
            val flutterSdkPath = properties.getProperty("flutter.sdk")
            require(flutterSdkPath != null) { "flutter.sdk not set in local.properties" }
            flutterSdkPath
        }

    includeBuild("$flutterSdkPath/packages/flutter_tools/gradle")

    resolutionStrategy {
        eachPlugin {
            when (requested.id.id) {
                "com.android.application",
                "com.android.library",
                "com.android.test",
                "com.android.dynamic-feature", ->
                        useModule("com.android.tools.build:gradle:${requested.version ?: androidGradlePluginVersion}")
            }
        }
    }

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    id("dev.flutter.flutter-plugin-loader") version "1.0.0"
    id("com.android.application") version "8.11.1" apply false
    id("com.android.library") version "8.11.1" apply false
    id("org.jetbrains.kotlin.android") version "2.2.20" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.20" apply false
}

include(":app")
include(":vesper-player-kit")
include(":vesper-player-kit-ffmpeg-runtime")
include(":vesper-player-kit-external-playback")
include(":vesper-player-kit-source-normalizer-ffmpeg")
include(":vesper-player-kit-frame-processor-diagnostic")

project(":vesper-player-kit").projectDir = file("../../../lib/android/vesper-player-kit")
project(":vesper-player-kit-ffmpeg-runtime").projectDir = file("../../../lib/android/vesper-player-kit-ffmpeg-runtime")
project(":vesper-player-kit-external-playback").projectDir = file("../../../lib/android/vesper-player-kit-external-playback")
project(":vesper-player-kit-source-normalizer-ffmpeg").projectDir =
    file("../../../lib/android/vesper-player-kit-source-normalizer-ffmpeg")
project(":vesper-player-kit-frame-processor-diagnostic").projectDir =
    file("../../../lib/android/vesper-player-kit-frame-processor-diagnostic")
