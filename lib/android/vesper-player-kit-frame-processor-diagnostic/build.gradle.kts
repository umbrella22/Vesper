import com.android.Version
import com.android.build.api.dsl.LibraryExtension

plugins {
    id("com.android.library")
}

val frameProcessorJniLibraries =
    providers
        .environmentVariable("VESPER_ANDROID_FRAME_PROCESSOR_JNI_LIBS")
        .orElse(providers.gradleProperty("vesper.player.android.frameProcessorDiagnosticJniLibs"))
        .orElse("src/main/jniLibs")

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

extensions.configure<LibraryExtension>("android") {
    namespace = "io.github.umbrella22.vesper.player.android.frame.processor.diagnostic"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main").jniLibs.directories.apply {
            clear()
            add(frameProcessorJniLibraries.get())
        }
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
