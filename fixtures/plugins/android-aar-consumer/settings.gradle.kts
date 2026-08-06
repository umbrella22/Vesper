pluginManagement {
    val androidGradlePluginVersion = "9.1.0"

    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    resolutionStrategy {
        eachPlugin {
            if (requested.id.id == "com.android.application") {
                useModule(
                    "com.android.tools.build:gradle:${requested.version ?: androidGradlePluginVersion}",
                )
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

rootProject.name = "vesper-android-aar-consumer"

include(":app")
