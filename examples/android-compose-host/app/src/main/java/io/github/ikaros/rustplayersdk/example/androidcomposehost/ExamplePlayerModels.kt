package io.github.ikaros.vesper.example.androidcomposehost

import android.content.Context
import androidx.annotation.StringRes
import androidx.compose.ui.graphics.Color
import io.github.ikaros.vesper.player.android.VesperPlaybackResiliencePolicy
import io.github.ikaros.vesper.player.android.VesperPlayerSource

internal enum class ExamplePlayerSheet {
    Menu,
    Quality,
    Audio,
    Subtitle,
    Speed,
}

internal enum class ExampleThemeMode(
    @get:StringRes val titleRes: Int,
) {
    System(R.string.example_theme_system),
    Light(R.string.example_theme_light),
    Dark(R.string.example_theme_dark),
}

internal enum class ExampleResilienceProfile(
    @get:StringRes val titleRes: Int,
    @get:StringRes val subtitleRes: Int,
) {
    Balanced(
        R.string.example_resilience_balanced,
        R.string.example_resilience_balanced_subtitle,
    ),
    Streaming(
        R.string.example_resilience_streaming,
        R.string.example_resilience_streaming_subtitle,
    ),
    Resilient(
        R.string.example_resilience_resilient,
        R.string.example_resilience_resilient_subtitle,
    ),
    LowLatency(
        R.string.example_resilience_low_latency,
        R.string.example_resilience_low_latency_subtitle,
    ),
    ;

    val policy: VesperPlaybackResiliencePolicy
        get() =
            when (this) {
                Balanced -> VesperPlaybackResiliencePolicy.balanced()
                Streaming -> VesperPlaybackResiliencePolicy.streaming()
                Resilient -> VesperPlaybackResiliencePolicy.resilient()
                LowLatency -> VesperPlaybackResiliencePolicy.lowLatency()
            }
}

internal data class ExampleHostPalette(
    val pageTop: Color,
    val pageBottom: Color,
    val sectionBackground: Color,
    val sectionStroke: Color,
    val title: Color,
    val body: Color,
    val fieldBackground: Color,
    val fieldText: Color,
    val primaryAction: Color,
)

internal fun exampleHostPalette(useDarkTheme: Boolean): ExampleHostPalette =
    if (useDarkTheme) {
        ExampleHostPalette(
            pageTop = Color(0xFF0C1018),
            pageBottom = Color(0xFF06080D),
            sectionBackground = Color.White.copy(alpha = 0.04f),
            sectionStroke = Color.White.copy(alpha = 0.06f),
            title = Color.White,
            body = Color(0xFF94A0B5),
            fieldBackground = Color.White.copy(alpha = 0.06f),
            fieldText = Color.White,
            primaryAction = Color(0xFF2A8BFF),
        )
    } else {
        ExampleHostPalette(
            pageTop = Color(0xFFF8F2EA),
            pageBottom = Color(0xFFF2F4F9),
            sectionBackground = Color.White.copy(alpha = 0.86f),
            sectionStroke = Color(0x140B1220),
            title = Color(0xFF101521),
            body = Color(0xFF5C667A),
            fieldBackground = Color(0xFFF6F7FA),
            fieldText = Color(0xFF101521),
            primaryAction = Color(0xFF256DFF),
        )
    }

internal const val ANDROID_HLS_DEMO_URL: String =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"

internal const val ANDROID_DASH_DEMO_URL: String =
    "https://dash.akamaized.net/envivio/EnvivioDash3/manifest.mpd"

internal fun androidHlsDemoSource(context: Context? = null): VesperPlayerSource =
    VesperPlayerSource.hls(
        uri = ANDROID_HLS_DEMO_URL,
        label = context?.getString(R.string.example_source_hls_demo_label) ?: "HLS Demo (BipBop)",
    )

internal fun androidDashDemoSource(context: Context? = null): VesperPlayerSource =
    VesperPlayerSource.dash(
        uri = ANDROID_DASH_DEMO_URL,
        label = context?.getString(R.string.example_source_dash_demo_label) ?: "DASH Demo (Envivio)",
    )
