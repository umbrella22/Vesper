import com.android.Version

plugins {
    id("com.android.library")
}

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.ikaros.vesper.player.android.source.normalizer.ffmpeg"
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

dependencies {
    api(project(":vesper-player-kit-ffmpeg-runtime"))
}

val checkNoBundledFfmpegRuntimeLibraries = tasks.register("checkNoBundledFfmpegRuntimeLibraries") {
    group = "verification"
    description = "Fails if the SourceNormalizer AAR bundles FFmpeg/runtime shared libraries."
    val nativeLibraries = fileTree("src/main/jniLibs") {
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
                "SourceNormalizer FFmpeg AAR must not bundle FFmpeg/runtime libraries:\n" +
                    bundledRuntimeLibraries.joinToString(separator = "\n"),
            )
        }
    }
}

tasks.named("check").configure {
    dependsOn(checkNoBundledFfmpegRuntimeLibraries)
}
