import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("kotlin-android")
    id("dev.flutter.flutter-gradle-plugin")
}

val configuredAndroidAbis =
    sequenceOf(
        "vesper.player.android.app.abis",
        "vesper.player.android.abis",
    ).mapNotNull { propertyName ->
        providers.gradleProperty(propertyName).orNull
    }.firstOrNull()
        ?.split(',', ' ')
        ?.map(String::trim)
        ?.filter(String::isNotEmpty)
        ?: listOf("arm64-v8a")

val workspaceRootDir = rootProject.layout.projectDirectory.dir("../../..")
val ffmpegRuntimeProfileHashFile =
    workspaceRootDir.file("lib/android/vesper-player-kit-ffmpeg-runtime/src/main/assets/vesper-ffmpeg-runtime/profile-hash.txt")
val playerFfmpegPluginJniLibsDir = layout.buildDirectory.dir("generated/playerFfmpeg/jniLibs")
val playerFfmpegPluginJniLibsDirFile = playerFfmpegPluginJniLibsDir.get().asFile
val playerFfmpegPluginBuildProfile =
    providers.provider {
        if (gradle.startParameter.taskNames.any { taskName ->
                taskName.contains("Release", ignoreCase = true)
            }
        ) {
            "release"
        } else {
            "debug"
        }
    }

android {
    namespace = "io.github.ikaros.vesper.example.flutterhost"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.ikaros.vesper.example.flutterhost"
        minSdk = 26
        targetSdk = 36
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        ndk {
            abiFilters += configuredAndroidAbis
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    sourceSets {
        getByName("main").jniLibs.directories.add(playerFfmpegPluginJniLibsDirFile.absolutePath)
    }

    packaging {
        jniLibs {
            // The example exposes the remux plugin at a stable file path for the dynamic plugin loader.
            useLegacyPackaging = true
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

tasks.register("unitTestClasses") {
    description = "Compatibility alias for IDE tooling expecting the legacy unitTestClasses task."
    dependsOn(
        tasks.matching {
            it.name == "compileDebugUnitTestKotlin" ||
                it.name == "compileDebugUnitTestJavaWithJavac" ||
                it.name == "compileDebugJavaWithJavac"
        }
    )
}

flutter {
    source = "../.."
}

dependencies {
    implementation(project(":vesper-player-kit-ffmpeg-runtime"))
    implementation(project(":vesper-player-kit-relay-ffmpeg"))
}

val buildPlayerRemuxFfmpegAndroidPlugin by tasks.registering(Exec::class) {
    description = "Builds the Android player-remux-ffmpeg plugin libraries used by the Flutter host."
    group = "vesper"

    val scriptFile = workspaceRootDir.file("scripts/android/build-player-remux-ffmpeg-plugin.sh")

    inputs.file(scriptFile)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/plugin-remux/player-remux-ffmpeg"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin"))
    inputs.dir(workspaceRootDir.dir("third_party/ffmpeg/android"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)
    outputs.dir(playerFfmpegPluginJniLibsDirFile)
    outputs.file(ffmpegRuntimeProfileHashFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))
    environment("VESPER_ANDROID_FFMPEG_CONSUMERS", "download-remux relay-remux")

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerFfmpegPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
        )
    }
}

val buildRelayFfmpegAndroidJni by tasks.registering(Exec::class) {
    description = "Builds the Android relay FFmpeg JNI library used by DLNA remux fallback."
    group = "vesper"

    val scriptFile = workspaceRootDir.file("scripts/android/build-relay-ffmpeg-jni.sh")

    inputs.file(scriptFile)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/platform/jni/player-relay-ffmpeg-android"))
    inputs.dir(workspaceRootDir.dir("third_party/ffmpeg/android"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))
    environment("VESPER_ANDROID_FFMPEG_CONSUMERS", "download-remux relay-remux")
    environment("VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD", "1")

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
        )
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildRelayFfmpegAndroidJni)
}

buildRelayFfmpegAndroidJni.configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
}
