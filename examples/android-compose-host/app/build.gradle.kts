plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val configuredAndroidAbis =
    providers.gradleProperty("vesper.player.android.abis").orNull
        ?.split(',', ' ')
        ?.map(String::trim)
        ?.filter(String::isNotEmpty)
        ?: listOf("arm64-v8a")

val workspaceRootDir = rootProject.layout.projectDirectory.dir("../..")
val playerFfmpegPluginJniLibsDir = layout.buildDirectory.dir("generated/playerFfmpeg/jniLibs")
val playerFfmpegPluginJniLibsDirFile = playerFfmpegPluginJniLibsDir.get().asFile
val playerSourceNormalizerPluginJniLibsDir =
    layout.buildDirectory.dir("generated/playerSourceNormalizerFfmpeg/jniLibs")
val playerSourceNormalizerPluginJniLibsDirFile = playerSourceNormalizerPluginJniLibsDir.get().asFile
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
    namespace = "io.github.ikaros.vesper.example.androidcomposehost"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.ikaros.vesper.example.androidcomposehost"
        minSdk = 26
        targetSdk = 36
        versionCode = 3
        versionName = "0.3.0"

        ndk {
            abiFilters += configuredAndroidAbis
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
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

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.02.01")

    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.fragment:fragment:1.7.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation(project(":vesper-player-kit-compose-ui"))
    implementation(project(":vesper-player-kit-external-playback"))
    implementation(project(":vesper-player-kit-ffmpeg-runtime"))
    implementation(project(":vesper-player-kit-source-normalizer-ffmpeg"))
    implementation(project(":vesper-player-kit-frame-processor-diagnostic"))
    testImplementation("junit:junit:4.13.2")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

val buildPlayerRemuxFfmpegAndroidPlugin by tasks.registering(Exec::class) {
    description = "Builds the Android player-remux-ffmpeg plugin libraries used by the example host."
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
    description = "Builds the Android player-source-normalizer-ffmpeg plugin libraries used by the example host."
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
    description = "Builds the Android player-frame-processor-diagnostic plugin libraries used by the example host."
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
        (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model"))
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
            (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model"))
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
        dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    }
}
