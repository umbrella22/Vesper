import com.android.Version

plugins {
    id("com.android.library")
}

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.ikaros.vesper.player.android.frame.processor.diagnostic"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        val releaseBuildType = getByName("release")
        maybeCreate("profile").apply {
            initWith(releaseBuildType)
            matchingFallbacks.clear()
            matchingFallbacks.add("release")
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}
