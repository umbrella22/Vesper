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
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += configuredAndroidAbis
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
    }

    packaging {
        jniLibs {
            // 例子里需要把 remux 插件暴露成稳定文件路径，便于交给动态插件加载器。
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
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation(project(":vesper-player-kit-compose-ui"))
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

    doFirst {
        commandLine(
            scriptFile.asFile.absolutePath,
            playerFfmpegPluginJniLibsDirFile.absolutePath,
            playerFfmpegPluginBuildProfile.get(),
            *configuredAndroidAbis.toTypedArray(),
        )
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildPlayerRemuxFfmpegAndroidPlugin)
}
