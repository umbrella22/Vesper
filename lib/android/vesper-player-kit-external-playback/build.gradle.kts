import com.android.Version
import java.io.File
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

val workspaceRootDir = layout.projectDirectory.dir("../../..")
val vesperCli = workspaceRootDir.file("scripts/vesper")

fun resolveWorkspaceFile(path: String): File {
    val configuredFile = File(path)
    return if (configuredFile.isAbsolute) configuredFile else workspaceRootDir.asFile.resolve(path)
}

val configuredAndroidAbis =
    sequenceOf(
        "vesper.player.android.external.abis",
        "vesper.player.android.app.abis",
        "vesper.player.android.abis",
    ).mapNotNull { propertyName ->
        providers.gradleProperty(propertyName).orNull
    }.firstOrNull()
        ?.split(',', ' ')
        ?.map(String::trim)
        ?.filter(String::isNotEmpty)
        ?: listOf("arm64-v8a")
val relayFfmpegJniLibsDir =
    providers
        .environmentVariable("VESPER_ANDROID_EXTERNAL_RELAY_JNI_LIBS")
        .orElse(layout.projectDirectory.dir("src/main/jniLibs").asFile.absolutePath)
val relayFfmpegAssetsDir =
    providers
        .environmentVariable("VESPER_ANDROID_EXTERNAL_RELAY_ASSETS")
        .orElse(layout.projectDirectory.dir("src/main/assets").asFile.absolutePath)
val relayFfmpegBuildProfile =
    providers.gradleProperty("vesper.player.android.external.nativeBuildProfile")
        .orElse(
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
            },
        )
        .map { profile ->
            require(profile == "debug" || profile == "release") {
                "vesper.player.android.external.nativeBuildProfile must be debug or release."
            }
            profile
        }
val relayFfmpegProfile =
    providers.gradleProperty("vesper.player.android.external.ffmpegProfile")
        .orElse(providers.gradleProperty("vesper.player.android.ffmpegProfile"))
        .orElse("default")
val skipFfmpegRuntime =
    providers.environmentVariable("VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD")
        .map { it == "1" || it.equals("true", ignoreCase = true) }
        .orElse(false)
val androidFfmpegRuntimeDir =
    providers.environmentVariable("VESPER_ANDROID_FFMPEG_OUTPUT_DIR")
        .orElse(providers.environmentVariable("VESPER_FFMPEG_OUTPUT_DIR"))
        .map(::resolveWorkspaceFile)
        .orElse(workspaceRootDir.dir("third_party/ffmpeg/android").asFile)
val ffmpegProfileConfigFile =
    providers.environmentVariable("VESPER_FFMPEG_PROFILE_CONFIG_PATH")
        .map(::resolveWorkspaceFile)
        .orElse(workspaceRootDir.file("scripts/ffmpeg-profiles.toml").asFile)
val ffmpegSourcePolicyFile =
    providers.environmentVariable("VESPER_FFMPEG_SOURCE_POLICY_FILE")
        .map(::resolveWorkspaceFile)
        .orElse(workspaceRootDir.file("scripts/ffmpeg-source-policy.toml").asFile)

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.umbrella22.vesper.player.android.external"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
    }


    sourceSets {
        getByName("main").apply {
            jniLibs.directories.clear()
            jniLibs.directories.add(relayFfmpegJniLibsDir.get())
            assets.directories.clear()
            assets.directories.add(relayFfmpegAssetsDir.get())
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

dependencies {
    val media3Version = "1.9.3"

    api(project(":vesper-player-kit"))
    api(project(":vesper-player-kit-ffmpeg-runtime"))
    api("androidx.appcompat:appcompat:1.6.1")
    api("androidx.media3:media3-cast:$media3Version")
    api("androidx.mediarouter:mediarouter:1.8.1")
    api("com.google.android.gms:play-services-cast-framework:22.3.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    testImplementation("junit:junit:4.13.2")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    androidTestImplementation("com.squareup.okhttp3:okhttp-tls:4.12.0")
}

val buildRelayFfmpegAndroidJni = tasks.register<Exec>("buildRelayFfmpegAndroidJni") {
    description = "Builds the Android relay FFmpeg JNI library for external playback."
    group = "vesper"

    inputs.file(vesperCli)
    inputs.dir(workspaceRootDir.dir("crates/tools/player-cli"))
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/platform/jni/player-relay-ffmpeg-android"))
    inputs.file(ffmpegProfileConfigFile)
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("buildProfile", relayFfmpegBuildProfile)
    inputs.property("ffmpegProfile", relayFfmpegProfile)
    inputs.property("skipFfmpegRuntime", skipFfmpegRuntime)
    if (skipFfmpegRuntime.get()) {
        inputs.dir(androidFfmpegRuntimeDir)
    } else {
        inputs.file(ffmpegSourcePolicyFile)
        localState.register(androidFfmpegRuntimeDir)
    }
    outputs.dir(relayFfmpegJniLibsDir)
    outputs.dir(relayFfmpegAssetsDir)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        val arguments = mutableListOf<Any>(
            vesperCli.asFile.absolutePath,
            "android",
            "external-playback-jni",
            relayFfmpegJniLibsDir.get(),
            "--assets-directory",
            relayFfmpegAssetsDir.get(),
            "--profile",
            relayFfmpegBuildProfile.get(),
            "--ffmpeg-profile",
            relayFfmpegProfile.get(),
        )
        if (skipFfmpegRuntime.get()) {
            arguments += "--skip-ffmpeg-runtime"
        }
        commandLine(
            arguments,
        )
    }
}

tasks.matching { task -> task.name == "verifyVesperNativeBinaryNames" }.configureEach {
    dependsOn(buildRelayFfmpegAndroidJni)
}

tasks.matching { task ->
    (task.name.startsWith("merge") &&
        (task.name.endsWith("JniLibFolders") || task.name.endsWith("Assets"))) ||
        task.name.startsWith("lint", ignoreCase = true) ||
        (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model"))
}.configureEach {
    dependsOn(buildRelayFfmpegAndroidJni)
}
