import org.gradle.api.publish.PublishingExtension
import org.gradle.api.publish.maven.MavenPublication
import org.gradle.plugins.signing.SigningExtension

plugins {
    id("com.android.library") version "9.1.0" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.10" apply false
}

allprojects {
    group = "io.github.ikaros.vesper"
    version = "0.3.0"
}

data class AndroidPublicationMetadata(
    val pomName: String,
    val description: String,
    val licenses: List<Pair<String, String>>,
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

subprojects {
    val metadata = vesperAndroidPublications[name] ?: return@subprojects

    pluginManager.apply("maven-publish")
    pluginManager.apply("signing")

    val publishing = extensions.getByType(PublishingExtension::class.java)
    components.configureEach {
        if (name != "release") {
            return@configureEach
        }
        publishing.publications.register<MavenPublication>("release") {
            from(this@configureEach)
            artifactId = project.name
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
                        id.set("ikaros")
                        name.set("Ikaros")
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

    val signingKey = providers.gradleProperty("signingInMemoryKey").orNull
    if (!signingKey.isNullOrBlank()) {
        extensions.configure<SigningExtension>("signing") {
            useInMemoryPgpKeys(
                signingKey,
                providers.gradleProperty("signingInMemoryKeyPassword").orNull,
            )
            sign(publishing.publications)
        }
    }
}
