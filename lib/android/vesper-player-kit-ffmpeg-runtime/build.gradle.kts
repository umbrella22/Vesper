import com.android.Version
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

val ffmpegRuntimeJniLibraries =
    providers
        .environmentVariable("VESPER_ANDROID_FFMPEG_RUNTIME_JNI_LIBS")
        .orElse("src/main/jniLibs")
val ffmpegRuntimeAssets =
    providers
        .environmentVariable("VESPER_ANDROID_FFMPEG_RUNTIME_ASSETS")
        .orElse("src/main/assets")

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.ikaros.vesper.player.android.ffmpeg.runtime"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main").apply {
            jniLibs.directories.clear()
            jniLibs.directories.add(ffmpegRuntimeJniLibraries.get())
            assets.directories.clear()
            assets.directories.add(ffmpegRuntimeAssets.get())
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

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

extensions.configure<KotlinAndroidProjectExtension>("kotlin") {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}
