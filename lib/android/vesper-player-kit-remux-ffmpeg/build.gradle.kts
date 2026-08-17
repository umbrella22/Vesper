import com.android.Version
import com.android.build.api.dsl.LibraryExtension

plugins {
    id("com.android.library")
}

val remuxJniLibraries =
    providers
        .environmentVariable("VESPER_ANDROID_REMUX_JNI_LIBS")
        .orElse(providers.gradleProperty("vesper.player.android.remuxJniLibs"))
        .orElse("src/main/jniLibs")
val remuxAssets =
    providers
        .environmentVariable("VESPER_ANDROID_REMUX_ASSETS")
        .orElse("src/main/assets")

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

extensions.configure<LibraryExtension>("android") {
    namespace = "io.github.umbrella22.vesper.player.android.remux.ffmpeg"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main").apply {
            jniLibs.directories.apply {
                clear()
                add(remuxJniLibraries.get())
            }
            assets.directories.apply {
                clear()
                add(remuxAssets.get())
            }
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

dependencies {
    api(project.dependencies.project(":vesper-player-kit"))
    api(project.dependencies.project(":vesper-player-kit-ffmpeg-runtime"))
}

val checkNoBundledFfmpegRuntimeLibraries = tasks.register("checkNoBundledFfmpegRuntimeLibraries") {
    group = "verification"
    description = "Fails if the remux AAR bundles FFmpeg runtime shared libraries."
    val nativeLibraries = fileTree(remuxJniLibraries.get()) {
        include("**/*.so")
    }
    inputs.files(nativeLibraries)

    doLast {
        val forbiddenNames =
            listOf(
                "libavcodec.so",
                "libavdevice.so",
                "libavfilter.so",
                "libavformat.so",
                "libavutil.so",
                "libpostproc.so",
                "libswresample.so",
                "libswscale.so",
                "libxml2.so",
                "libssl.so",
                "libcrypto.so",
            )
        val bundledRuntimeLibraries =
            nativeLibraries.files
                .filter { file -> file.name in forbiddenNames }
                .map { file -> file.relativeTo(projectDir).path }
                .sorted()
        if (bundledRuntimeLibraries.isNotEmpty()) {
            throw GradleException(
                "Remux FFmpeg AAR must not bundle FFmpeg runtime libraries:\n" +
                    bundledRuntimeLibraries.joinToString(separator = "\n"),
            )
        }
    }
}

tasks.named("check").configure {
    dependsOn(checkNoBundledFfmpegRuntimeLibraries)
}
