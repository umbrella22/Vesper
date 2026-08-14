import java.io.File

plugins {
    id("com.android.application")
}

val vesperReleaseDir =
    providers.gradleProperty("vesper.releaseDir").orNull
        ?.let(::File)
        ?.canonicalFile
        ?: throw GradleException(
            "Set -Pvesper.releaseDir to a directory produced by `vesper android stage-release`.",
        )

val vesperAarNames =
    listOf(
        "VesperPlayerKit-android-arm64-v8a.aar",
        "VesperPlayerKitFfmpegRuntime-android-arm64-v8a.aar",
        "VesperPlayerKitSourceNormalizerFfmpeg-android-arm64-v8a.aar",
        "VesperPlayerKitDecoderMediaCodec-android-arm64-v8a.aar",
        "VesperPlayerKitFrameProcessorDiagnostic-android-arm64-v8a.aar",
    )
val vesperAars = vesperAarNames.map(vesperReleaseDir::resolve)
val missingVesperAars = vesperAars.filterNot(File::isFile)
if (missingVesperAars.isNotEmpty()) {
    throw GradleException(
        "The staged Vesper release is incomplete:\n" +
            missingVesperAars.joinToString(separator = "\n") { file -> "- ${file.absolutePath}" },
    )
}

android {
    namespace = "io.github.umbrella22.vesper.fixture.androidaarconsumer"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.umbrella22.vesper.fixture.androidaarconsumer"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    testBuildType = "release"

    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs.useLegacyPackaging = true
    }

    sourceSets
        .getByName("androidTest")
        .assets
        .directories
        .add(rootProject.file("../../media").absolutePath)
}

dependencies {
    implementation(files(vesperAars))

    val media3Version = "1.11.0"
    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.media3:media3-exoplayer:$media3Version")
    implementation("androidx.media3:media3-exoplayer-hls:$media3Version")
    implementation("androidx.media3:media3-exoplayer-dash:$media3Version")
    implementation("androidx.media3:media3-session:$media3Version")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")

    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("androidx.test:rules:1.7.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
}
