plugins {
    id("com.android.library")
}

val repoRoot = rootDir.parentFile.parentFile
val rustAndroidBuildScript = repoRoot.resolve("scripts/build-android-vesper-player-kit-jni.sh")

val buildRustAndroidHostDebug by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds debug Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(rustAndroidBuildScript.absolutePath, "debug")
}

val buildRustAndroidHostRelease by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds release Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(rustAndroidBuildScript.absolutePath, "release")
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

    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
}

dependencies {
    val media3Version = "1.9.3"

    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.media3:media3-exoplayer:$media3Version")
    implementation("androidx.media3:media3-exoplayer-hls:$media3Version")
    implementation("androidx.media3:media3-exoplayer-dash:$media3Version")
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
