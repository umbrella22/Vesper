import org.gradle.api.publish.PublishingExtension
import org.gradle.api.publish.maven.MavenPublication
import org.gradle.api.tasks.bundling.Jar
import org.gradle.api.tasks.Exec
import org.gradle.plugins.signing.SigningExtension
import com.android.build.api.dsl.LibraryExtension

plugins {
    id("com.android.library") version "9.1.0" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.10" apply false
}

val vesperMavenGroupId =
    providers.gradleProperty("vesper.maven.groupId").orElse("io.github.umbrella22.vesper")

allprojects {
    group = vesperMavenGroupId.get()
    version = "0.4.1"
}

data class AndroidPublicationMetadata(
    val pomName: String,
    val description: String,
    val licenses: List<Pair<String, String>>,
)

data class AndroidPluginRegistryMetadata(
    val manifestPath: String,
    val libraryName: String,
    val pluginId: String,
    val variants: Set<String>,
)

val apacheLicense =
    "Apache License, Version 2.0" to "https://www.apache.org/licenses/LICENSE-2.0.txt"
val lgplLicense =
    "GNU Lesser General Public License, Version 2.1 or later" to
        "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html"

fun String.isTruthy(): Boolean =
    this == "1" || equals("true", ignoreCase = true) || equals("yes", ignoreCase = true)

val publishOptionalPluginArtifacts =
    providers.gradleProperty("vesper.publish.optionalPlugins")
        .map { it.isTruthy() }
        .orElse(false)
        .get()

val vesperAndroidCorePublications =
    mapOf(
        "vesper-player-kit" to
            AndroidPublicationMetadata(
                pomName = "Vesper Player Android Kit",
                description = "Android host kit for Vesper Player applications.",
                licenses = listOf(apacheLicense),
            ),
        "vesper-player-kit-compose" to
            AndroidPublicationMetadata(
                pomName = "Vesper Player Android Compose Adapter",
                description = "Jetpack Compose lifecycle and surface adapter for Vesper Player.",
                licenses = listOf(apacheLicense),
            ),
        "vesper-player-kit-compose-ui" to
            AndroidPublicationMetadata(
                pomName = "Vesper Player Android Compose UI",
                description = "Optional Jetpack Compose controls and player UI for Vesper Player.",
                licenses = listOf(apacheLicense),
            ),
    )

val vesperAndroidOptionalPluginPublications =
    mapOf(
        "vesper-player-kit-ffmpeg-runtime" to
            AndroidPublicationMetadata(
                pomName = "Vesper Player Android FFmpeg Runtime",
                description =
                    "Optional Android FFmpeg runtime libraries for Vesper Player plugins. " +
                        "Redistributed FFmpeg components keep their upstream license terms.",
                licenses = listOf(lgplLicense),
            ),
        "vesper-player-kit-source-normalizer-ffmpeg" to
            AndroidPublicationMetadata(
                pomName = "Vesper Player Android SourceNormalizer FFmpeg Plugin",
                description =
                    "Optional Android SourceNormalizer plugin for Vesper Player. " +
                        "The artifact depends on the FFmpeg runtime artifact for libav/libsw dependencies.",
                licenses = listOf(apacheLicense),
            ),
    )

val vesperAndroidPublications =
    vesperAndroidCorePublications +
        if (publishOptionalPluginArtifacts) {
            vesperAndroidOptionalPluginPublications
        } else {
            emptyMap()
        }

val vesperAndroidPluginRegistries =
    mapOf(
        "vesper-player-kit-decoder-mediacodec" to
            AndroidPluginRegistryMetadata(
                manifestPath = "plugins/decoder-mediacodec/vesper-plugin.toml",
                libraryName = "vesper_decoder_mediacodec",
                pluginId = "io.github.umbrella22.vesper.decoder-mediacodec",
                variants = setOf("release"),
            ),
        "vesper-player-kit-source-normalizer-ffmpeg" to
            AndroidPluginRegistryMetadata(
                manifestPath = "plugins/source-normalizer-ffmpeg/vesper-plugin.toml",
                libraryName = "vesper_source_normalizer_ffmpeg",
                pluginId = "io.github.umbrella22.vesper.source-normalizer-ffmpeg",
                variants = setOf("profile", "release"),
            ),
        "vesper-player-kit-frame-processor-diagnostic" to
            AndroidPluginRegistryMetadata(
                manifestPath = "plugins/frame-processor-diagnostic/vesper-plugin.toml",
                libraryName = "vesper_frame_processor_diagnostic",
                pluginId = "dev.vesper.frame-processor-diagnostic",
                variants = setOf("profile", "release"),
            ),
    )

val vesperAndroidNativeLibraryEnvironmentVariables =
    mapOf(
        "vesper-player-kit" to "VESPER_ANDROID_HOST_JNI_LIBS",
        "vesper-player-kit-decoder-mediacodec" to "VESPER_ANDROID_DECODER_JNI_LIBS",
        "vesper-player-kit-source-normalizer-ffmpeg" to
            "VESPER_ANDROID_SOURCE_NORMALIZER_JNI_LIBS",
        "vesper-player-kit-frame-processor-diagnostic" to
            "VESPER_ANDROID_FRAME_PROCESSOR_JNI_LIBS",
    )

val vesperRepoRoot = rootProject.file("../..").canonicalFile
val configuredVesperCli = providers.environmentVariable("VESPER_CLI")
val defaultVesperCli = vesperRepoRoot.resolve("target/release/vesper")
val vesperCli =
    configuredVesperCli
        .map { configuredPath ->
            val configuredFile = java.io.File(configuredPath)
            if (configuredFile.isAbsolute) {
                configuredFile
            } else {
                vesperRepoRoot.resolve(configuredPath)
            }
        }.orElse(defaultVesperCli)
val buildVesperPluginCli =
    tasks.register<Exec>("buildVesperPluginCli") {
        group = "vesper"
        description = "Builds the Rust CLI used to generate embedded plugin registry fragments."
        onlyIf { !configuredVesperCli.isPresent }
        workingDir = vesperRepoRoot
        commandLine("cargo", "build", "-p", "player-cli", "--bin", "vesper", "--release")
        outputs.file(defaultVesperCli)
        outputs.upToDateWhen { false }
    }

subprojects {
    val verifyVesperNativeBinaryNames = tasks.register("verifyVesperNativeBinaryNames") {
        group = "verification"
        description = "Fails if Android packaging inputs contain Vesper-owned native libraries using libplayer_* names."
        val nativeLibraryDirectory =
            vesperAndroidNativeLibraryEnvironmentVariables[name]
                ?.let { variable ->
                    providers.environmentVariable(variable).orElse("src/main/jniLibs").get()
                } ?: "src/main/jniLibs"
        val nativeLibraries = fileTree(nativeLibraryDirectory) {
            include("**/*.so")
        }
        inputs.files(nativeLibraries)

        doLast {
            val stalePlayerLibraries =
                nativeLibraries.files
                    .filter { file -> file.name.startsWith("libplayer_") }
                    .map { file -> file.relativeTo(projectDir).path }
                    .sorted()
            if (stalePlayerLibraries.isNotEmpty()) {
                throw GradleException(
                    "Vesper-owned Android native libraries must use libvesper_* names:\n" +
                        stalePlayerLibraries.joinToString(separator = "\n"),
                )
            }
        }
    }

    plugins.withId("com.android.library") {
        tasks.named("preBuild").configure {
            dependsOn(verifyVesperNativeBinaryNames)
        }
        tasks.named("check").configure {
            dependsOn(verifyVesperNativeBinaryNames)
        }

        val registryMetadata = vesperAndroidPluginRegistries[name]
        if (registryMetadata != null) {
            val android = extensions.getByType(LibraryExtension::class.java)
            val pluginManifest = rootProject.file("../../${registryMetadata.manifestPath}")
            registryMetadata.variants.forEach { variant ->
                val variantTitle = variant.replaceFirstChar(Char::uppercaseChar)
                val generatedAssets =
                    layout.buildDirectory.dir("generated/vesperPluginRegistryAssets/$variant")
                android.sourceSets.maybeCreate(variant).assets.directories.add(
                    generatedAssets.get().asFile.absolutePath,
                )
                val stripTaskName = "strip${variantTitle}DebugSymbols"
                val strippedPlugin =
                    layout.buildDirectory.file(
                        "intermediates/stripped_native_libs/$variant/$stripTaskName/out/lib/arm64-v8a/lib${registryMetadata.libraryName}.so",
                    )
                val registryFragment =
                    generatedAssets.map { directory ->
                        directory.file(
                            "vesper/plugins/arm64-v8a/${registryMetadata.pluginId}.json",
                        )
                    }
                val generateRegistry =
                    tasks.register<Exec>("generate${variantTitle}VesperPluginRegistry") {
                        dependsOn(stripTaskName)
                        dependsOn(buildVesperPluginCli)
                        inputs.file(vesperCli)
                        inputs.file(pluginManifest)
                        inputs.file(strippedPlugin)
                        inputs.property("platform", "android")
                        inputs.property("target", "aarch64-linux-android")
                        inputs.property("architecture", "arm64-v8a")
                        inputs.property("minimumOs", "26")
                        inputs.property("locatorName", registryMetadata.libraryName)
                        inputs.property("pluginId", registryMetadata.pluginId)
                        outputs.dir(generatedAssets)
                        outputs.upToDateWhen { false }

                        doFirst {
                            val generatedAssetsDirectory = generatedAssets.get().asFile
                            if (generatedAssetsDirectory.exists() &&
                                !generatedAssetsDirectory.deleteRecursively()
                            ) {
                                throw GradleException(
                                    "Failed to clear generated Vesper plugin registry assets: " +
                                        generatedAssetsDirectory,
                                )
                            }
                            registryFragment.get().asFile.parentFile.mkdirs()
                            commandLine(
                                vesperCli.get().absolutePath,
                                "plugin",
                                "registry-fragment",
                                pluginManifest.absolutePath,
                                "--platform",
                                "android",
                                "--target",
                                "aarch64-linux-android",
                                "--architecture",
                                "arm64-v8a",
                                "--minimum-os",
                                "26",
                                "--locator-name",
                                registryMetadata.libraryName,
                                "--artifact",
                                strippedPlugin.get().asFile.absolutePath,
                                "--output",
                                registryFragment.get().asFile.absolutePath,
                            )
                        }
                    }
                tasks.configureEach {
                    if (name == "merge${variantTitle}Assets") {
                        dependsOn(generateRegistry)
                    }
                }
            }
        }
    }

    val metadata = vesperAndroidPublications[name] ?: return@subprojects

    pluginManager.apply("maven-publish")
    pluginManager.apply("signing")

    val javadocJar =
        tasks.register<Jar>("javadocJar") {
            archiveClassifier.set("javadoc")
            from(rootProject.file("../../README.md"))
        }

    val publishing = extensions.getByType(PublishingExtension::class.java)
    publishing.repositories.maven {
        name = "centralStaging"
        val configuredDirectory =
            providers
                .gradleProperty("vesper.maven.repositoryDirectory")
                .orElse(rootProject.layout.buildDirectory.dir("central-staging").map { it.asFile.path })
                .get()
        url = uri(configuredDirectory)
    }
    components.configureEach {
        if (name != "release") {
            return@configureEach
        }
        publishing.publications.register<MavenPublication>("release") {
            from(this@configureEach)
            artifactId = project.name
            artifact(javadocJar)
            pom {
                name.set(metadata.pomName)
                description.set(metadata.description)
                url.set("https://github.com/umbrella22/Vesper")
                licenses {
                    metadata.licenses.forEach { (licenseName, licenseUrl) ->
                        license {
                            name.set(licenseName)
                            url.set(licenseUrl)
                        }
                    }
                }
                developers {
                    developer {
                        id.set("umbrella22")
                        name.set("umbrella22")
                        url.set("https://github.com/umbrella22")
                    }
                }
                scm {
                    connection.set("scm:git:https://github.com/umbrella22/Vesper.git")
                    developerConnection.set("scm:git:ssh://git@github.com/umbrella22/Vesper.git")
                    url.set("https://github.com/umbrella22/Vesper")
                }
            }
        }
    }

    val signingKey =
        providers
            .gradleProperty("signingInMemoryKey")
            .orElse(providers.environmentVariable("MAVEN_GPG_PRIVATE_KEY"))
            .orNull
    if (!signingKey.isNullOrBlank()) {
        extensions.configure<SigningExtension>("signing") {
            useInMemoryPgpKeys(
                signingKey,
                providers
                    .gradleProperty("signingInMemoryKeyPassword")
                    .orElse(providers.environmentVariable("MAVEN_GPG_PASSPHRASE"))
                    .orNull,
            )
            sign(publishing.publications)
        }
    }
}
