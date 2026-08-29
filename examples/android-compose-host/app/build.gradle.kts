import com.android.build.api.dsl.ApplicationExtension
import java.io.File
import org.gradle.api.tasks.Delete

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

data class VesperAppPluginRegistryMetadata(
    val taskSegment: String,
    val manifestPath: String,
    val libraryName: String,
    val pluginId: String,
)

val configuredAndroidAbis =
    providers.gradleProperty("vesper.player.android.abis").orNull
        ?.split(',', ' ')
        ?.map(String::trim)
        ?.filter(String::isNotEmpty)
        ?: listOf("arm64-v8a")

val workspaceRootDir = rootProject.layout.projectDirectory.dir("../..")
val playerFfmpegPluginJniLibsDir = layout.buildDirectory.dir("generated/playerFfmpeg/jniLibs")
val playerFfmpegPluginJniLibsDirFile = playerFfmpegPluginJniLibsDir.get().asFile
val playerFfmpegPluginAssetsDir = layout.buildDirectory.dir("generated/playerFfmpeg/assets")
val playerFfmpegPluginAssetsDirFile = playerFfmpegPluginAssetsDir.get().asFile
val playerFfmpegPluginMetadataDirFile =
    playerFfmpegPluginAssetsDir.get().dir("vesper-remux-ffmpeg").asFile
val playerSourceNormalizerPluginJniLibsDir =
    layout.buildDirectory.dir("generated/playerSourceNormalizerFfmpeg/jniLibs")
val playerSourceNormalizerPluginJniLibsDirFile = playerSourceNormalizerPluginJniLibsDir.get().asFile
val playerSourceNormalizerPluginAssetsDir =
    layout.buildDirectory.dir("generated/playerSourceNormalizerFfmpeg/assets")
val playerSourceNormalizerPluginAssetsDirFile =
    playerSourceNormalizerPluginAssetsDir.get().asFile
val playerSourceNormalizerPluginMetadataDirFile =
    playerSourceNormalizerPluginAssetsDir.get().dir("vesper-source-normalizer-ffmpeg").asFile
val playerDecoderMediaCodecPluginJniLibsDir =
    layout.buildDirectory.dir("generated/playerDecoderMediaCodec/jniLibs")
val playerDecoderMediaCodecPluginJniLibsDirFile =
    playerDecoderMediaCodecPluginJniLibsDir.get().asFile
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
val vesperAppPluginRegistries =
    listOf(
        VesperAppPluginRegistryMetadata(
            taskSegment = "RemuxFfmpeg",
            manifestPath = "plugins/remux-ffmpeg/vesper-plugin.toml",
            libraryName = "vesper_remux_ffmpeg",
            pluginId = "io.github.umbrella22.vesper.remux-ffmpeg",
        ),
        VesperAppPluginRegistryMetadata(
            taskSegment = "DecoderMediaCodec",
            manifestPath = "plugins/decoder-mediacodec/vesper-plugin.toml",
            libraryName = "vesper_decoder_mediacodec",
            pluginId = "io.github.umbrella22.vesper.decoder-mediacodec",
        ),
        VesperAppPluginRegistryMetadata(
            taskSegment = "SourceNormalizerFfmpeg",
            manifestPath = "plugins/source-normalizer-ffmpeg/vesper-plugin.toml",
            libraryName = "vesper_source_normalizer_ffmpeg",
            pluginId = "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
        ),
        VesperAppPluginRegistryMetadata(
            taskSegment = "FrameProcessorDiagnostic",
            manifestPath = "plugins/frame-processor-diagnostic/vesper-plugin.toml",
            libraryName = "vesper_frame_processor_diagnostic",
            pluginId = "dev.vesper.frame-processor-diagnostic",
        ),
    )
val configuredVesperCli = providers.environmentVariable("VESPER_CLI")
val defaultVesperCli = workspaceRootDir.file("target/release/vesper").asFile
val vesperCli =
    configuredVesperCli
        .map { configuredPath ->
            val configuredFile = File(configuredPath)
            if (configuredFile.isAbsolute) {
                configuredFile
            } else {
                workspaceRootDir.file(configuredPath).asFile
            }
        }.orElse(defaultVesperCli)
val buildVesperPluginCli =
    tasks.register<Exec>("buildVesperPluginCli") {
        group = "vesper"
        description = "Builds the Rust CLI used to generate app plugin registry fragments."
        onlyIf { !configuredVesperCli.isPresent }
        workingDir = workspaceRootDir.asFile
        commandLine("cargo", "build", "-p", "player-cli", "--bin", "vesper", "--release")
        outputs.file(defaultVesperCli)
        outputs.upToDateWhen { false }
    }

val androidExtension = extensions.getByType(ApplicationExtension::class.java)

extensions.configure<ApplicationExtension>("android") {
    namespace = "io.github.umbrella22.vesper.example.androidcomposehost"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.umbrella22.vesper.example.androidcomposehost"
        minSdk = 26
        targetSdk = 36
        versionCode = 500
        versionName = "0.5.0"

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
            playerDecoderMediaCodecPluginJniLibsDirFile.absolutePath,
        )
        getByName("main").jniLibs.directories.add(
            playerFrameProcessorDiagnosticPluginJniLibsDirFile.absolutePath,
        )
        getByName("main").assets.directories.add(playerFfmpegPluginAssetsDirFile.absolutePath)
        getByName("main").assets.directories.add(
            playerSourceNormalizerPluginAssetsDirFile.absolutePath,
        )
    }

    packaging {
        jniLibs {
            // Native extraction keeps FFmpeg dependencies available to the internal plugin loader.
            useLegacyPackaging = true
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.06.01")

    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.fragment:fragment:1.9.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation(project.dependencies.project(":vesper-player-kit-compose-ui"))
    implementation(project.dependencies.project(":vesper-player-kit-external-playback"))
    implementation(project.dependencies.project(":vesper-player-kit-ffmpeg-runtime"))
    testImplementation("junit:junit:4.13.2")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

val buildPlayerRemuxFfmpegAndroidPlugin = tasks.register<Exec>("buildPlayerRemuxFfmpegAndroidPlugin") {
    description = "Builds the Android player-remux-ffmpeg plugin libraries used by the example host."
    group = "vesper"

    val vesperCli = workspaceRootDir.file("scripts/vesper")

    inputs.file(vesperCli)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/plugin-remux/player-remux-ffmpeg"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin"))
    inputs.dir(workspaceRootDir.dir("third_party/ffmpeg/android"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)
    outputs.dir(playerFfmpegPluginJniLibsDirFile)
    outputs.dir(playerFfmpegPluginMetadataDirFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            vesperCli.asFile.absolutePath,
            "android",
            "remux-plugin",
            playerFfmpegPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
            "--profile",
            "default",
            "--metadata-dir",
            playerFfmpegPluginMetadataDirFile.absolutePath,
        )
    }
}

val buildPlayerSourceNormalizerFfmpegAndroidPlugin =
    tasks.register<Exec>("buildPlayerSourceNormalizerFfmpegAndroidPlugin") {
    description = "Builds the Android player-source-normalizer-ffmpeg plugin libraries used by the example host."
    group = "vesper"

    val vesperCli = workspaceRootDir.file("scripts/vesper")

    inputs.file(vesperCli)
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
    outputs.dir(playerSourceNormalizerPluginMetadataDirFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            vesperCli.asFile.absolutePath,
            "android",
            "source-normalizer-plugin",
            playerSourceNormalizerPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
            "--profile",
            "default",
            "--metadata-dir",
            playerSourceNormalizerPluginMetadataDirFile.absolutePath,
        )
    }
}

val buildPlayerFrameProcessorDiagnosticAndroidPlugin =
    tasks.register<Exec>("buildPlayerFrameProcessorDiagnosticAndroidPlugin") {
    description = "Builds the Android player-frame-processor-diagnostic plugin libraries used by the example host."
    group = "vesper"

    val vesperCli = workspaceRootDir.file("scripts/vesper")

    inputs.file(vesperCli)
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
            vesperCli.asFile.absolutePath,
            "android",
            "frame-processor-plugin",
            playerFrameProcessorDiagnosticPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
        )
    }
}

val buildPlayerDecoderMediaCodecAndroidPlugin =
    tasks.register<Exec>("buildPlayerDecoderMediaCodecAndroidPlugin") {
    description = "Builds the Android player-decoder-mediacodec plugin libraries used by the example host."
    group = "vesper"

    val vesperCli = workspaceRootDir.file("scripts/vesper")

    inputs.file(vesperCli)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/plugin-decoder/player-decoder-mediacodec"))
    inputs.dir(workspaceRootDir.dir("crates/plugin/player-plugin"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("profile", playerFfmpegPluginBuildProfile)
    outputs.dir(playerDecoderMediaCodecPluginJniLibsDirFile)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            vesperCli.asFile.absolutePath,
            "android",
            "decoder-mediacodec-plugin",
            playerDecoderMediaCodecPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
        )
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    dependsOn(buildPlayerDecoderMediaCodecAndroidPlugin)
    dependsOn(buildPlayerFrameProcessorDiagnosticAndroidPlugin)
}

tasks.matching { task ->
    (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
        (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model"))
}.configureEach {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    dependsOn(buildPlayerDecoderMediaCodecAndroidPlugin)
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

val relayFfmpegProject = rootProject.project(":vesper-player-kit-external-playback")
relayFfmpegProject.plugins.withId("com.android.library") {
    relayFfmpegProject.tasks.matching { task ->
        task.name == "buildRelayFfmpegAndroidJni"
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
        dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    }
}

listOf("debug", "release").forEach { variant ->
    val variantTitle = variant.replaceFirstChar(Char::uppercaseChar)
    val generatedAssets =
        layout.buildDirectory.dir("generated/vesperPluginRegistryAssets/$variant")
    val cleanRegistryAssets =
        tasks.register<Delete>("clean${variantTitle}VesperPluginRegistryAssets") {
            delete(generatedAssets)
        }
    androidExtension.sourceSets.maybeCreate(variant).assets.directories.add(
        generatedAssets.get().asFile.absolutePath,
    )
    val stripTaskName = "strip${variantTitle}DebugSymbols"
    val registryTasks =
        vesperAppPluginRegistries.map { metadata ->
            val pluginManifest = workspaceRootDir.file(metadata.manifestPath)
            val strippedPlugin =
                layout.buildDirectory.file(
                    "intermediates/stripped_native_libs/$variant/$stripTaskName/out/" +
                        "lib/arm64-v8a/lib${metadata.libraryName}.so",
                )
            val registryFragment =
                generatedAssets.map { directory ->
                    directory.file(
                        "vesper/plugins/arm64-v8a/${metadata.pluginId}.json",
                    )
                }
            tasks.register<Exec>(
                "generate${variantTitle}Vesper${metadata.taskSegment}PluginRegistry",
            ) {
                group = "vesper"
                description =
                    "Generates the $variant ${metadata.pluginId} registry from final stripped bytes."
                dependsOn(cleanRegistryAssets)
                dependsOn(stripTaskName)
                dependsOn(buildVesperPluginCli)
                inputs.file(vesperCli)
                inputs.file(pluginManifest)
                inputs.file(strippedPlugin)
                inputs.property("target", "aarch64-linux-android")
                inputs.property("architecture", "arm64-v8a")
                inputs.property("minimumOs", "26")
                inputs.property("locatorName", metadata.libraryName)
                outputs.file(registryFragment)

                doFirst {
                    registryFragment.get().asFile.parentFile.mkdirs()
                    commandLine(
                        vesperCli.get().absolutePath,
                        "plugin",
                        "registry-fragment",
                        pluginManifest.asFile.absolutePath,
                        "--platform",
                        "android",
                        "--target",
                        "aarch64-linux-android",
                        "--architecture",
                        "arm64-v8a",
                        "--minimum-os",
                        "26",
                        "--locator-name",
                        metadata.libraryName,
                        "--artifact",
                        strippedPlugin.get().asFile.absolutePath,
                        "--output",
                        registryFragment.get().asFile.absolutePath,
                    )
                }
            }
        }
    tasks.matching { task ->
        task.name == "merge${variantTitle}Assets" ||
            (task.name.startsWith("generate$variantTitle") &&
                task.name.contains("Lint") &&
                task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") &&
                task.name.contains(variantTitle) &&
                task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(registryTasks)
    }
}
