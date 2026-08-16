import com.android.Version
import com.android.build.api.dsl.LibraryExtension
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

// AGP 9+ has built-in Kotlin support; Flutter hosts may still bring this module in through AGP 8.x.
if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

val repoRoot = projectDir.resolve("../../..").canonicalFile
val vesperCli = repoRoot.resolve("scripts/vesper")
// The Rust CLI deliberately restricts test JNI publication to this module-owned tree.
// Keep that boundary stable when Flutter rehomes Gradle build directories.
val androidTestJniLibsDir = layout.projectDirectory.dir("build/generated/androidTestJniLibs")
val rustAndroidAbis = providers.gradleProperty("vesper.player.android.abis").orNull
val hostJniLibraries =
    providers
        .environmentVariable("VESPER_ANDROID_HOST_JNI_LIBS")
        .orElse("src/main/jniLibs")
val extractsInstrumentationNativeLibraries =
    gradle.startParameter.taskNames.any { taskName ->
        taskName.contains("AndroidTest", ignoreCase = true) ||
            taskName.endsWith("connectedCheck", ignoreCase = true) ||
            taskName.endsWith("deviceCheck", ignoreCase = true)
    }

require(vesperCli.isFile) {
    "Vesper CLI launcher not found: ${vesperCli.absolutePath}"
}

val buildRustAndroidHostDebug = tasks.register<Exec>("buildRustAndroidHostDebug") {
    group = "rust"
    description = "Builds debug Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(vesperCli.absolutePath, "android", "jni", "debug")
    if (!rustAndroidAbis.isNullOrBlank()) {
        environment("RUST_ANDROID_ABIS", rustAndroidAbis)
    }
}

val buildRustAndroidHostRelease = tasks.register<Exec>("buildRustAndroidHostRelease") {
    group = "rust"
    description = "Builds release Android JNI libraries for the Rust player host library."
    workingDir = repoRoot
    commandLine(vesperCli.absolutePath, "android", "jni", "release")
    if (!rustAndroidAbis.isNullOrBlank()) {
        environment("RUST_ANDROID_ABIS", rustAndroidAbis)
    }
}

val provisionAndroidTestNativeLibraries = tasks.register<Exec>("provisionAndroidTestNativeLibraries") {
    group = "rust"
    description = "Builds test-only decoder, SourceNormalizer, and FFmpeg JNI libraries."
    dependsOn(buildRustAndroidHostDebug)
    workingDir = repoRoot
    commandLine(
        vesperCli.absolutePath,
        "android",
        "provision-test-jni",
        androidTestJniLibsDir.asFile.absolutePath,
        "--profile",
        "debug",
        "--ffmpeg-profile",
        "default",
    )
    if (!rustAndroidAbis.isNullOrBlank()) {
        environment("RUST_ANDROID_ABIS", rustAndroidAbis)
    }
    outputs.dir(androidTestJniLibsDir)
    outputs.upToDateWhen { false }
}

extensions.configure<LibraryExtension>("android") {
    namespace = "io.github.umbrella22.vesper.player.android"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
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

    packaging {
        jniLibs.useLegacyPackaging = extractsInstrumentationNativeLibraries
    }

    sourceSets.getByName("main").jniLibs.directories.apply {
        clear()
        add(hostJniLibraries.get())
    }

    sourceSets.getByName("androidTest").apply {
        assets.directories.add(repoRoot.resolve("fixtures/media").absolutePath)
        jniLibs.directories.add(androidTestJniLibsDir.asFile.absolutePath)
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
    val media3Version = "1.11.0"

    implementation("androidx.core:core-ktx:1.18.0")
    implementation("androidx.media3:media3-exoplayer:$media3Version")
    implementation("androidx.media3:media3-exoplayer-hls:$media3Version")
    implementation("androidx.media3:media3-exoplayer-dash:$media3Version")
    implementation("androidx.media3:media3-session:$media3Version")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20260719")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.11.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test:rules:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:core:1.7.0")
}

val checkPublicApiSurface = tasks.register("checkPublicApiSurface") {
    group = "verification"
    description = "Fails when bridge, JNI, or Native* implementation types leak into the Kotlin public API."
    val kotlinSources = fileTree("src/main/java") {
        include("**/*.kt")
    }
    inputs.files(kotlinSources)

    doLast {
        val declarationPattern =
            Regex("^(?:public\\s+)?(?:(?:data|sealed|enum|value)\\s+)*(class|interface|object|typealias)\\s+([A-Za-z_][A-Za-z0-9_]*)")
        val forbiddenNamePattern = Regex("(?:^Native|^VesperNative|Bridge|Jni)")
        val allowedPublicInternalNames =
            setOf(
                "VesperNativeFramePipelineConfiguration",
                "VesperNativeFramePipelineMode",
            )
        val leaks = kotlinSources.files.flatMap { file ->
            file.readLines().mapIndexedNotNull { index, line ->
                val trimmed = line.trim()
                if (
                    trimmed.startsWith("internal ") ||
                    trimmed.startsWith("private ") ||
                    trimmed.startsWith("@")
                ) {
                    return@mapIndexedNotNull null
                }

                val match = declarationPattern.find(trimmed) ?: return@mapIndexedNotNull null
                val declarationName = match.groupValues[2]
                if (declarationName in allowedPublicInternalNames) {
                    return@mapIndexedNotNull null
                }
                if (!forbiddenNamePattern.containsMatchIn(declarationName)) {
                    return@mapIndexedNotNull null
                }

                "${file.relativeTo(projectDir)}:${index + 1}: $trimmed"
            }
        }

        if (leaks.isNotEmpty()) {
            throw GradleException(
                "Internal Android bridge/JNI/native declarations leaked into the public API:\n" +
                    leaks.joinToString(separator = "\n"),
            )
        }
    }
}

tasks.named("check").configure {
    dependsOn(checkPublicApiSurface)
}

tasks.matching {
    it.name == "preDebugBuild" ||
        it.name == "preDebugAndroidTestBuild" ||
        it.name == "mergeDebugJniLibFolders" ||
        it.name == "mergeDebugAndroidTestJniLibFolders" ||
        (it.name.startsWith("generateDebug") && it.name.contains("Lint") && it.name.endsWith("Model"))
}.configureEach {
    dependsOn(buildRustAndroidHostDebug)
}

tasks.matching {
    it.name == "preDebugAndroidTestBuild" ||
        it.name == "mergeDebugAndroidTestJniLibFolders" ||
        it.name == "mergeDebugAndroidTestNativeLibs"
}.configureEach {
    dependsOn(provisionAndroidTestNativeLibraries)
}

tasks.matching {
    it.name == "preReleaseBuild" ||
        it.name == "preProfileBuild" ||
        it.name == "mergeReleaseJniLibFolders" ||
        it.name == "mergeProfileJniLibFolders" ||
        (it.name.startsWith("generateRelease") && it.name.contains("Lint") && it.name.endsWith("Model")) ||
        (it.name.startsWith("generateProfile") && it.name.contains("Lint") && it.name.endsWith("Model"))
}.configureEach {
    dependsOn(buildRustAndroidHostRelease)
}

buildRustAndroidHostRelease.configure {
    mustRunAfter(tasks.matching { task ->
        task.name == "mergeDebugJniLibFolders" ||
            task.name == "mergeDebugAndroidTestJniLibFolders"
    })
}

tasks.matching { task -> task.name == "verifyVesperNativeBinaryNames" }.configureEach {
    mustRunAfter(buildRustAndroidHostDebug, buildRustAndroidHostRelease)
}

tasks.matching {
    it.name == "assembleRelease" ||
        it.name == "assembleProfile" ||
        it.name == "bundleReleaseAar" ||
        it.name == "publishReleasePublicationToMavenLocal"
}.configureEach {
    dependsOn(buildRustAndroidHostRelease)
}
