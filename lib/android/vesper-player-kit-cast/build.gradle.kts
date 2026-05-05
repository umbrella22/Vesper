import com.android.Version
import org.jetbrains.kotlin.gradle.dsl.KotlinAndroidProjectExtension
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
}

if (!Version.ANDROID_GRADLE_PLUGIN_VERSION.startsWith("9.")) {
    apply(plugin = "org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.ikaros.vesper.player.android.cast"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
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
    api("androidx.media3:media3-cast:$media3Version")
    api("androidx.mediarouter:mediarouter:1.8.1")
    api("com.google.android.gms:play-services-cast-framework:22.3.1")
}
