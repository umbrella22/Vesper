package io.github.ikaros.vesper.player.android

import android.view.ViewGroup
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.foundation.shape.RoundedCornerShape

@Composable
fun VesperPlayerSurface(
    controller: VesperPlayerController,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .background(Color.Black, RoundedCornerShape(20.dp)),
    ) {
        DisposableEffect(controller) {
            controller.initialize()
            onDispose { controller.dispose() }
        }
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                object : android.widget.FrameLayout(context) {}.apply {
                    clipToOutline = true
                    controller.attachSurfaceHost(this)
                }
            },
            update = { host ->
                controller.attachSurfaceHost(host as ViewGroup)
            },
        )
        DisposableEffect(controller) {
            onDispose { controller.detachSurfaceHost() }
        }
        Text(
            text = when (controller.backend) {
                PlayerBridgeBackend.FakeDemo -> "Preview surface host"
                PlayerBridgeBackend.VesperNativeStub -> "Rust native surface host"
            },
            color = Color.White,
            modifier = Modifier
                .align(Alignment.Center)
                .background(Color(0x44000000), RoundedCornerShape(10.dp))
                .padding(horizontal = 12.dp, vertical = 8.dp),
        )
    }
}
