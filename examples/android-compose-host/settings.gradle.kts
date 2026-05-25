pluginManagement {
    val androidGradlePluginVersion = "9.1.0"

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
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
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "player-android-compose-host"

include(":app")

include(":vesper-player-kit")
include(":vesper-player-kit-external-playback")
include(":vesper-player-kit-ffmpeg-runtime")
include(":vesper-player-kit-source-normalizer-ffmpeg")
include(":vesper-player-kit-frame-processor-diagnostic")
include(":vesper-player-kit-compose")
include(":vesper-player-kit-compose-ui")

project(":vesper-player-kit").projectDir = file("../../lib/android/vesper-player-kit")
project(":vesper-player-kit-external-playback").projectDir = file("../../lib/android/vesper-player-kit-external-playback")
project(":vesper-player-kit-ffmpeg-runtime").projectDir = file("../../lib/android/vesper-player-kit-ffmpeg-runtime")
project(":vesper-player-kit-source-normalizer-ffmpeg").projectDir =
    file("../../lib/android/vesper-player-kit-source-normalizer-ffmpeg")
project(":vesper-player-kit-frame-processor-diagnostic").projectDir =
    file("../../lib/android/vesper-player-kit-frame-processor-diagnostic")
project(":vesper-player-kit-compose").projectDir = file("../../lib/android/vesper-player-kit-compose")
project(":vesper-player-kit-compose-ui").projectDir = file("../../lib/android/vesper-player-kit-compose-ui")
