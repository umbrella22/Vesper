import com.android.Version
import com.android.build.api.dsl.LibraryExtension

plugins {
    id("com.android.library")
}

val decoderJniLibraries =
    providers
        .environmentVariable("VESPER_ANDROID_DECODER_JNI_LIBS")
        .orElse("src/main/jniLibs")

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

extensions.configure<LibraryExtension>("android") {
    namespace = "io.github.umbrella22.vesper.player.android.decoder.mediacodec"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main").jniLibs.directories.apply {
            clear()
            add(decoderJniLibraries.get())
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}
