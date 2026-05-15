import com.android.Version
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

val workspaceRootDir = layout.projectDirectory.dir("../../..")
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
val relayFfmpegJniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")
val relayFfmpegBuildProfile =
    providers.gradleProperty("vesper.player.android.external.nativeBuildProfile")
        .orElse(
            providers.provider {
                if (gradle.startParameter.taskNames.any { taskName ->
                        taskName.contains("Release", ignoreCase = true)
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

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.ikaros.vesper.player.android.external"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
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
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    testImplementation("junit:junit:4.13.2")
}

val buildRelayFfmpegAndroidJni by tasks.registering(Exec::class) {
    description = "Builds the Android relay FFmpeg JNI library for external playback."
    group = "vesper"

    val scriptFile = workspaceRootDir.file("scripts/android/internal/build-external-playback-relay-ffmpeg-jni.sh")

    inputs.file(scriptFile)
    inputs.file(workspaceRootDir.file("Cargo.toml"))
    inputs.file(workspaceRootDir.file("Cargo.lock"))
    inputs.dir(workspaceRootDir.dir("crates/platform/jni/player-relay-ffmpeg-android"))
    inputs.dir(workspaceRootDir.dir("third_party/ffmpeg/android"))
    inputs.property("abis", configuredAndroidAbis)
    inputs.property("buildProfile", relayFfmpegBuildProfile)
    inputs.property("ffmpegProfile", relayFfmpegProfile)
    outputs.dir(relayFfmpegJniLibsDir)

    workingDir = workspaceRootDir.asFile
    environment("RUST_ANDROID_ABIS", configuredAndroidAbis.joinToString(","))

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            relayFfmpegBuildProfile.get(),
            "--profile",
            relayFfmpegProfile.get(),
        )
    }
}

tasks.matching { task ->
    (task.name.startsWith("merge") && task.name.endsWith("JniLibFolders")) ||
        (task.name.startsWith("generate") && task.name.contains("Lint") && task.name.endsWith("Model"))
}.configureEach {
    dependsOn(buildRelayFfmpegAndroidJni)
}
