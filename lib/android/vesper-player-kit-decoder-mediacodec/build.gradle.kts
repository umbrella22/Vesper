import com.android.Version

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

android {
    namespace = "io.github.ikaros.vesper.player.android.decoder.mediacodec"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main").jniLibs.setSrcDirs(listOf(decoderJniLibraries.get()))
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}
