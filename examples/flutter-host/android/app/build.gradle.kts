import com.android.build.gradle.LibraryExtension
import java.io.File

plugins {
    id("com.android.application")
    id("dev.flutter.flutter-gradle-plugin")
}

data class VesperAppPluginRegistryMetadata(
    val taskSegment: String,
    val manifestPath: String,
    val libraryName: String,
    val pluginId: String,
)

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
                taskName.contains("Release", ignoreCase = true) ||
                    taskName.contains("Profile", ignoreCase = true)
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

android {
    namespace = "io.github.umbrella22.vesper.example.flutterhost"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.umbrella22.vesper.example.flutterhost"
        minSdk = 26
        targetSdk = 36
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        if (!isFlutterSplitPerAbiBuild.get()) {
            ndk {
                abiFilters.clear()
                abiFilters.addAll(configuredAndroidAbis)
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
        // Flutter's plugin initially creates profile from debug. Rebase it on
        // release so local profile measurements use optimized dependencies.
        val releaseBuildType = getByName("release")
        maybeCreate("profile").apply {
            initWith(releaseBuildType)
            isMinifyEnabled = false
            isShrinkResources = false
            matchingFallbacks.clear()
            matchingFallbacks.add("release")
        }
    }

    sourceSets {
        getByName("main").jniLibs.directories.add(playerFfmpegPluginJniLibsDirFile.absolutePath)
    }

    packaging {
        jniLibs {
            // Native extraction keeps FFmpeg dependencies available to the internal plugin loader.
            useLegacyPackaging = true
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
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
    implementation(project.dependencies.project(":vesper-player-kit-ffmpeg-runtime"))
    implementation(project.dependencies.project(":vesper-player-kit-external-playback"))
    implementation(project.dependencies.project(":vesper-player-kit-source-normalizer-ffmpeg"))
    implementation(project.dependencies.project(":vesper-player-kit-frame-processor-diagnostic"))
}

val buildPlayerRemuxFfmpegAndroidPlugin = tasks.register<Exec>("buildPlayerRemuxFfmpegAndroidPlugin") {
    description = "Builds the Android player-remux-ffmpeg plugin libraries used by the Flutter host."
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
        )
    }
}

val buildPlayerSourceNormalizerFfmpegAndroidPlugin =
    tasks.register<Exec>("buildPlayerSourceNormalizerFfmpegAndroidPlugin") {
    description = "Builds the Android player-source-normalizer-ffmpeg plugin libraries used by the Flutter host."
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
        )
    }
}

val buildPlayerFrameProcessorDiagnosticAndroidPlugin =
    tasks.register<Exec>("buildPlayerFrameProcessorDiagnosticAndroidPlugin") {
    description = "Builds the Android player-frame-processor-diagnostic plugin libraries used by the Flutter host."
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

tasks.named("preBuild").configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    dependsOn(buildPlayerFrameProcessorDiagnosticAndroidPlugin)
}

val sourceNormalizerPluginProject =
    rootProject.project(":vesper-player-kit-source-normalizer-ffmpeg")
sourceNormalizerPluginProject.plugins.withId("com.android.library") {
    sourceNormalizerPluginProject.extensions
        .getByType(LibraryExtension::class.java)
        .sourceSets
        .getByName("main")
        .jniLibs
        .setSrcDirs(listOf(playerSourceNormalizerPluginJniLibsDirFile))
    sourceNormalizerPluginProject.tasks.matching { task ->
        (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
            (task.name.startsWith("generate") &&
                task.name.contains("Lint") &&
                task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") && task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    }
}

val frameProcessorPluginProject =
    rootProject.project(":vesper-player-kit-frame-processor-diagnostic")
frameProcessorPluginProject.plugins.withId("com.android.library") {
    frameProcessorPluginProject.extensions
        .getByType(LibraryExtension::class.java)
        .sourceSets
        .getByName("main")
        .jniLibs
        .setSrcDirs(listOf(playerFrameProcessorDiagnosticPluginJniLibsDirFile))
    frameProcessorPluginProject.tasks.matching { task ->
        (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
            (task.name.startsWith("generate") &&
                task.name.contains("Lint") &&
                task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") && task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(buildPlayerFrameProcessorDiagnosticAndroidPlugin)
    }
}

val verifyVesperProfileReleaseSelection = tasks.register("verifyVesperProfileReleaseSelection") {
    group = "verification"
    description = "Verifies Flutter Profile uses release Android and native variants."
    doLast {
        val profileBuildType = android.buildTypes.getByName("profile")
        require(profileBuildType.matchingFallbacks == listOf("release")) {
            "Flutter Profile must resolve release Android variants; fallbacks=" +
                profileBuildType.matchingFallbacks
        }
        require(playerFfmpegPluginBuildProfile.get() == "release") {
            "Flutter Profile must use release native plugin profile."
        }
        val profileRuntimeClasspath = requireNotNull(
            configurations.findByName("profileRuntimeClasspath")
        ) {
            "Flutter Profile runtime classpath was not created; cannot verify dependency variants."
        }
        val debugDependencies = profileRuntimeClasspath.incoming.resolutionResult.allDependencies
            .filterIsInstance<org.gradle.api.artifacts.result.ResolvedDependencyResult>()
            .filter { dependency ->
                dependency.resolvedVariant.displayName.contains("debug", ignoreCase = true)
            }
        require(debugDependencies.isEmpty()) {
            "Flutter Profile resolved Debug variants: " +
                debugDependencies.joinToString { dependency ->
                    "${dependency.requested.displayName} -> ${dependency.resolvedVariant.displayName}"
                }
        }
    }
}

tasks.matching { task -> task.name == "preProfileBuild" }.configureEach {
    dependsOn(verifyVesperProfileReleaseSelection)
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
        dependsOn(buildPlayerSourceNormalizerFfmpegAndroidPlugin)
    }
    relayFfmpegProject.tasks.matching { task ->
        (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
            (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model")) ||
            (task.name.startsWith("lint") && task.name.contains("Analyze"))
    }.configureEach {
        dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
    }
}

listOf("debug", "profile", "release").forEach { variant ->
    val variantTitle = variant.replaceFirstChar(Char::uppercaseChar)
    val generatedAssets =
        layout.buildDirectory.dir("generated/vesperPluginRegistryAssets/$variant")
    android.sourceSets.maybeCreate(variant).assets.directories.add(
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
