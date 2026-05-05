import com.android.Version
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

// AGP 9+ 已内建 Kotlin 支持；Flutter 侧仍可能通过 AGP 8.x 引入本模块。
if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

val repoRoot = projectDir.resolve("../../..").canonicalFile
val rustAndroidBuildScript = repoRoot.resolve("scripts/android/build-vesper-player-kit-jni.sh")
val rustAndroidAbis = providers.gradleProperty("vesper.player.android.abis").orNull

require(rustAndroidBuildScript.isFile) {
    "Rust Android build script not found: ${rustAndroidBuildScript.absolutePath}"
}

val buildRustAndroidHostDebug by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds debug Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(rustAndroidBuildScript.absolutePath, "debug")
    if (!rustAndroidAbis.isNullOrBlank()) {
        environment("RUST_ANDROID_ABIS", rustAndroidAbis)
    }
}

val buildRustAndroidHostRelease by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds release Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(rustAndroidBuildScript.absolutePath, "release")
    if (!rustAndroidAbis.isNullOrBlank()) {
        environment("RUST_ANDROID_ABIS", rustAndroidAbis)
    }
}

android {
    namespace = "io.github.ikaros.vesper.player.android"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

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

    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.media3:media3-exoplayer:$media3Version")
    implementation("androidx.media3:media3-exoplayer-hls:$media3Version")
    implementation("androidx.media3:media3-exoplayer-dash:$media3Version")
    testImplementation("junit:junit:4.13.2")
}

tasks.matching { it.name == "preBuild" }.configureEach {
    dependsOn(buildRustAndroidHostDebug)
}

tasks.matching {
    it.name == "assembleRelease" ||
        it.name == "bundleReleaseAar" ||
        it.name == "publishReleasePublicationToMavenLocal"
}.configureEach {
    dependsOn(buildRustAndroidHostRelease)
}
