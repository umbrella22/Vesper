pluginManagement {
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
                        useModule("com.android.tools.build:gradle:${requested.version}")
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
include(":vesper-player-kit-compose")

project(":vesper-player-kit").projectDir = file("../../lib/android/vesper-player-kit")
project(":vesper-player-kit-compose").projectDir = file("../../lib/android/vesper-player-kit-compose")
