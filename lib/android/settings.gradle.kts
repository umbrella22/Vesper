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

rootProject.name = "vesper-android-lib"

include(":vesper-player-kit")
include(":vesper-player-kit-ffmpeg-runtime")
include(":vesper-player-kit-source-normalizer-ffmpeg")
include(":vesper-player-kit-frame-processor-diagnostic")
include(":vesper-player-kit-external-playback")
include(":vesper-player-kit-compose")
include(":vesper-player-kit-compose-ui")
