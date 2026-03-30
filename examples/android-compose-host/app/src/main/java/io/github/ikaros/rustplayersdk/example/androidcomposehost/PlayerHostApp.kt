package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import io.github.ikaros.vesper.player.android.PlaybackStateUi
import io.github.ikaros.vesper.player.android.VesperPlayerController
import io.github.ikaros.vesper.player.android.VesperPlayerSource
import io.github.ikaros.vesper.player.android.VesperPlayerSurface
import io.github.ikaros.vesper.player.android.TimelineKind
import io.github.ikaros.vesper.player.android.rememberVesperPlayerController
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

@Composable
fun PlayerHostApp(
    controller: VesperPlayerController = rememberVesperPlayerController(
        initialSource = androidHlsDemoSource(),
    ),
) {
    val context = LocalContext.current
    val uiState by controller.uiState.collectAsStateWithLifecycle()
    var pendingSeekRatio by remember(
        uiState.timeline.positionMs,
        uiState.timeline.durationMs,
        uiState.timeline.seekableRange?.startMs,
        uiState.timeline.seekableRange?.endMs,
        uiState.timeline.liveEdgeMs,
    ) {
        mutableFloatStateOf(uiState.timeline.displayedRatio ?: 0f)
    }
    var remoteStreamUrl by remember { mutableStateOf(ANDROID_HLS_DEMO_URL) }
    val pickVideoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        controller.selectSource(
            VesperPlayerSource.local(
                uri = uri.toString(),
                label = displayNameForUri(context, uri),
            )
        )
    }

    MaterialTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color(0xFFF4F1EA))
                    .windowInsetsPadding(WindowInsets.safeDrawing)
                    .verticalScroll(rememberScrollState())
                    .padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(uiState.title, style = MaterialTheme.typography.headlineMedium)
                Text(uiState.subtitle, style = MaterialTheme.typography.bodyMedium)
                Text(
                    "Source: ${uiState.sourceLabel}",
                    style = MaterialTheme.typography.bodySmall,
                    color = Color(0xFF5A4B8A),
                )

                VesperPlayerSurface(
                    controller = controller,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(240.dp),
                )

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    AssistChip(
                        onClick = {},
                        label = { Text(uiState.playbackState.name) },
                    )
                    AssistChip(
                        onClick = {},
                        label = { Text(controller.backend.name) },
                    )
                    AssistChip(
                        onClick = {},
                        label = { Text("rate ${uiState.playbackRate}x") },
                    )
                    if (uiState.isBuffering) {
                        AssistChip(onClick = {}, label = { Text("buffering") })
                    }
                    if (uiState.isInterrupted) {
                        AssistChip(onClick = {}, label = { Text("interrupted") })
                    }
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { pickVideoLauncher.launch(arrayOf("video/*")) }) {
                        Text("Pick Video")
                    }
                    Button(
                        onClick = {
                            controller.selectSource(androidHlsDemoSource())
                        }
                    ) {
                        Text("Use HLS Demo")
                    }
                    Button(
                        onClick = {
                            controller.selectSource(androidDashDemoSource())
                        }
                    ) {
                        Text("Use DASH Demo")
                    }
                }

                TextField(
                    value = remoteStreamUrl,
                    onValueChange = { remoteStreamUrl = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Remote Stream URL") },
                    singleLine = true,
                )

                Button(
                    onClick = {
                        val url = remoteStreamUrl.trim()
                        if (url.isNotEmpty()) {
                            controller.selectSource(
                                VesperPlayerSource.remote(
                                    uri = url,
                                    label = "Custom Remote Stream",
                                )
                            )
                        }
                    }
                ) {
                    Text("Open Remote URL")
                }

                if (uiState.timeline.isSeekable &&
                    (uiState.timeline.kind == TimelineKind.Vod || uiState.timeline.kind == TimelineKind.LiveDvr)
                ) {
                    Slider(
                        value = pendingSeekRatio,
                        onValueChange = { pendingSeekRatio = it },
                        onValueChangeFinished = { controller.seekToRatio(pendingSeekRatio) },
                    )
                    if (uiState.timeline.kind == TimelineKind.LiveDvr) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            val liveLatencyMs = uiState.timeline.liveEdgeMs?.let {
                                (it - uiState.timeline.positionMs).coerceAtLeast(0L)
                            }
                            AssistChip(onClick = {}, label = {
                                Text(
                                    if (liveLatencyMs != null && liveLatencyMs > 1_500L) {
                                        "LIVE -${formatMillis(liveLatencyMs)}"
                                    } else {
                                        "LIVE"
                                    }
                                )
                            })
                            Button(onClick = { controller.seekToLiveEdge() }) {
                                Text("Go Live")
                            }
                        }
                        Text(
                            "DVR ${formatMillis(uiState.timeline.positionMs)} / ${
                                formatMillis(uiState.timeline.liveEdgeMs ?: uiState.timeline.durationMs ?: 0L)
                            }",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    } else {
                        Text(
                            "${formatMillis(uiState.timeline.positionMs)} / ${
                                formatMillis(uiState.timeline.durationMs ?: 0L)
                            }",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                } else if (uiState.timeline.kind == TimelineKind.Live) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        AssistChip(onClick = {}, label = { Text("LIVE") })
                        uiState.timeline.liveEdgeMs?.let {
                            Text(
                                "Edge ${formatMillis(it)}",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                    }
                } else {
                    Text("Live timeline UI will be shown here when live/DVR backends land.")
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = { controller.seekBy(-5_000L) }) { Text("<< 5s") }
                    Button(onClick = { controller.togglePause() }) {
                        Text(if (uiState.playbackState == PlaybackStateUi.Playing) "Pause" else "Play")
                    }
                    Button(onClick = { controller.stop() }) { Text("Stop") }
                    Button(onClick = { controller.seekBy(5_000L) }) { Text("5s >>") }
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    VesperPlayerController.supportedPlaybackRates.forEach { rate ->
                        Button(onClick = { controller.setPlaybackRate(rate) }) {
                            Text("${rate}x")
                        }
                    }
                }
            }
        }
    }
}

private fun formatMillis(value: Long): String {
    val totalSeconds = value / 1000L
    val minutes = totalSeconds / 60L
    val seconds = totalSeconds % 60L
    return "%02d:%02d".format(minutes, seconds)
}

private fun displayNameForUri(context: Context, uri: Uri): String {
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
            val columnIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (columnIndex >= 0 && cursor.moveToFirst()) {
                cursor.getString(columnIndex)?.takeIf { it.isNotBlank() }?.let { return it }
            }
        }

    return uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
        ?: uri.toString()
}

private const val ANDROID_HLS_DEMO_URL =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"

private const val ANDROID_DASH_DEMO_URL =
    "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd"

private fun androidHlsDemoSource(): VesperPlayerSource =
    VesperPlayerSource.hls(
        uri = ANDROID_HLS_DEMO_URL,
        label = "HLS Demo (BipBop)",
    )

private fun androidDashDemoSource(): VesperPlayerSource =
    VesperPlayerSource.dash(
        uri = ANDROID_DASH_DEMO_URL,
        label = "DASH Demo (Envivio)",
    )
