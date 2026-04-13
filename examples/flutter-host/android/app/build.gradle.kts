import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("kotlin-android")
    id("dev.flutter.flutter-gradle-plugin")
}

val configuredAndroidAbis =
    providers.gradleProperty("vesper.player.android.app.abis").orNull
        ?.split(',', ' ')
        ?.map(String::trim)
        ?.filter(String::isNotEmpty)
        ?: listOf("arm64-v8a")

android {
    namespace = "io.github.ikaros.vesper.example.flutterhost"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.ikaros.vesper.example.flutterhost"
        minSdk = 26
        targetSdk = 36
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        ndk {
            abiFilters += configuredAndroidAbis
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
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
