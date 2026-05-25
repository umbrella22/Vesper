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
val isFlutterSplitPerAbiBuild =
    providers.gradleProperty("split-per-abi")
        .map(String::toBoolean)
        .orElse(false)

val workspaceRootDir = rootProject.layout.projectDirectory.dir("../../..")
val playerFfmpegPluginJniLibsDir = layout.buildDirectory.dir("generated/playerFfmpeg/jniLibs")
val playerFfmpegPluginJniLibsDirFile = playerFfmpegPluginJniLibsDir.get().asFile
val playerSourceNormalizerPluginJniLibsDir =
    layout.buildDirectory.dir("generated/playerSourceNormalizerFfmpeg/jniLibs")
val playerSourceNormalizerPluginJniLibsDirFile =
    playerSourceNormalizerPluginJniLibsDir.get().asFile
val playerFrameProcessorDiagnosticPluginJniLibsDir =
    layout.buildDirectory.dir("generated/playerFrameProcessorDiagnostic/jniLibs")
val playerFrameProcessorDiagnosticPluginJniLibsDirFile =
    playerFrameProcessorDiagnosticPluginJniLibsDir.get().asFile
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

        if (!isFlutterSplitPerAbiBuild.get()) {
            ndk {
                abiFilters += configuredAndroidAbis
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    sourceSets {
        getByName("main").jniLibs.directories.add(playerFfmpegPluginJniLibsDirFile.absolutePath)
        getByName("main").jniLibs.directories.add(
            playerSourceNormalizerPluginJniLibsDirFile.absolutePath,
        )
        getByName("main").jniLibs.directories.add(
            playerFrameProcessorDiagnosticPluginJniLibsDirFile.absolutePath,
        )
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
    implementation(project(":vesper-player-kit-external-playback"))
    implementation(project(":vesper-player-kit-source-normalizer-ffmpeg"))
    implementation(project(":vesper-player-kit-frame-processor-diagnostic"))
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

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerFfmpegPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
            "--profile",
            "default",
        )
    }
}

val buildPlayerSourceNormalizerFfmpegAndroidPlugin by tasks.registering(Exec::class) {
    description = "Builds the Android player-source-normalizer-ffmpeg plugin libraries used by the Flutter host."
    group = "vesper"

    val scriptFile = workspaceRootDir.file("scripts/android/build-player-source-normalizer-ffmpeg-plugin.sh")

    inputs.file(scriptFile)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.file(workspaceRootDir.file("scripts/source-normalizer-profiles.toml"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-source-normalizer-ffmpeg"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin-loader"))
    inputs.dir(workspaceRootDir.dir("third_party/ffmpeg/android"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)
    outputs.dir(playerSourceNormalizerPluginJniLibsDirFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerSourceNormalizerPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
            "--profile",
            "default",
        )
    }
}

val buildPlayerFrameProcessorDiagnosticAndroidPlugin by tasks.registering(Exec::class) {
    description = "Builds the Android player-frame-processor-diagnostic plugin libraries used by the Flutter host."
    group = "vesper"

    val scriptFile = workspaceRootDir.file("scripts/android/build-player-frame-processor-diagnostic-plugin.sh")

    inputs.file(scriptFile)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-frame-processor-diagnostic"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)
    outputs.dir(playerFrameProcessorDiagnosticPluginJniLibsDirFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerFrameProcessorDiagnosticPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
        )
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    dependsOn(buildPlayerFrameProcessorDiagnosticAndroidPlugin)
}

tasks.matching { task ->
    (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
        (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model")) ||
        (task.name.startsWith("lint") && task.name.contains("Analyze"))
}.configureEach {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    dependsOn(buildPlayerFrameProcessorDiagnosticAndroidPlugin)
}

val ffmpegRuntimeProject = rootProject.project(":vesper-player-kit-ffmpeg-runtime")
ffmpegRuntimeProject.plugins.withId("com.android.library") {
    ffmpegRuntimeProject.tasks.matching { task ->
        (task.name.startsWith("merge") &&
            (task.name.endsWith("Assets") || task.name.endsWith("JniLibFolders"))) ||
            (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") && task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
        dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    }
}

val relayFfmpegProject = rootProject.project(":vesper-player-kit-external-playback")
relayFfmpegProject.plugins.withId("com.android.library") {
    relayFfmpegProject.tasks.matching { task ->
        task.name == "buildRelayFfmpegAndroidJni"
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    }
    relayFfmpegProject.tasks.matching { task ->
        (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
            (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") && task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    }
}
