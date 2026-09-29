package io.github.umbrella22.vesper.player.android

import java.io.ByteArrayOutputStream
import java.io.File
import java.net.URI
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext

/** Reads only an explicitly supplied manifest, with the caller's remaining budget. */
internal suspend fun readVesperLocalManifest(uri: URI, maximumBytes: Long): ByteArray =
    withContext(Dispatchers.IO) {
        require(uri.scheme.equals("file", ignoreCase = true) && uri.userInfo == null)
        require(uri.authority.isNullOrEmpty() || uri.authority.equals("localhost", ignoreCase = true))
        require(uri.query == null && uri.fragment == null)
        require(maximumBytes in 1..1024 * 1024)
        val file = File(URI("file", null, uri.path, null))
        require(file.isFile) { "Manifest must be a regular local file" }
        require(file.length() <= maximumBytes) { "Manifest exceeds startup budget" }
        currentCoroutineContext().ensureActive()
        val output = ByteArrayOutputStream()
        file.inputStream().use { input ->
            val buffer = ByteArray(16 * 1024)
            while (true) {
                currentCoroutineContext().ensureActive()
                val count = input.read(buffer, 0, minOf(buffer.size.toLong(), maximumBytes - output.size() + 1).toInt())
                if (count < 0) break
                require(output.size().toLong() + count <= maximumBytes) { "Manifest exceeds startup budget" }
                output.write(buffer, 0, count)
            }
        }
        currentCoroutineContext().ensureActive()
        require(output.size() > 0) { "Manifest is empty" }
        output.toByteArray()
    }
