package io.github.umbrella22.vesper.example.androidcomposehost

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color

/**
 * Brand tokens shared by the Vesper example hosts (iOS / Android / HarmonyOS).
 *
 * The values mirror the confirmed prototype tokens. Dynamic color is intentionally
 * not used so the brand accent stays stable across devices.
 */
internal data class ExampleHostTokens(
    val accent: Color,
    val onAccent: Color,
    val accentInk: Color,
    val background: Color,
    val surface: Color,
    val surface2: Color,
    val ink: Color,
    val ink2: Color,
    val line: Color,
    val outline: Color,
    val ok: Color,
    val warn: Color,
    val err: Color,
    val info: Color,
    val onErr: Color,
)

/** The SDK stage is always dark, independent of the host theme. */
internal val ExampleStageColor: Color = Color(0xFF0B0D16)
internal val ExampleStageForeground: Color = Color(0xFFF4F2EC)

internal val ExampleLightTokens: ExampleHostTokens =
    ExampleHostTokens(
        accent = Color(0xFFE8A93A),
        onAccent = Color(0xFF1B1406),
        accentInk = Color(0xFF8C5B00),
        background = Color(0xFFF5F3EE),
        surface = Color(0xFFFFFFFF),
        surface2 = Color(0xFFECE9E2),
        ink = Color(0xFF17181E),
        ink2 = Color(0xFF5C5F6B),
        line = Color(0x1A17181E),
        outline = Color(0xFF8A8D98),
        ok = Color(0xFF2E9D6A),
        warn = Color(0xFFD98A1C),
        err = Color(0xFFD2463C),
        info = Color(0xFF4C7FD9),
        onErr = Color(0xFFFFFFFF),
    )

internal val ExampleDarkTokens: ExampleHostTokens =
    ExampleLightTokens.copy(
        accentInk = Color(0xFFF0BC5E),
        background = Color(0xFF0E1017),
        surface = Color(0xFF171A23),
        surface2 = Color(0xFF20242F),
        ink = Color(0xFFECEAE4),
        ink2 = Color(0xFF9B9EAA),
        line = Color(0x1AECEAE4),
        outline = Color(0xFF6E7280),
    )

internal fun exampleHostTokens(useDarkTheme: Boolean): ExampleHostTokens =
    if (useDarkTheme) ExampleDarkTokens else ExampleLightTokens

/**
 * Builds the Material 3 color scheme from the brand tokens.
 *
 * Following the prototype, filled buttons are neutral (`primary` = ink on
 * background) and the bright accent is reserved for at most two prominent
 * elements per screen: the navigation indicator pill and the extended FAB
 * (`primaryContainer`). `tertiary` carries the accent ink for small highlights.
 */
internal fun exampleHostColorScheme(useDarkTheme: Boolean): ColorScheme {
    val t = exampleHostTokens(useDarkTheme)
    val outlineVariant = if (useDarkTheme) Color(0xFF2C303B) else Color(0xFFDCD9D2)
    val lowest = if (useDarkTheme) t.background else t.surface
    return if (useDarkTheme) {
        darkColorScheme(
            primary = t.ink, onPrimary = t.background,
            primaryContainer = t.accent, onPrimaryContainer = t.onAccent,
            secondary = t.ink2, onSecondary = t.background,
            secondaryContainer = t.surface2, onSecondaryContainer = t.ink,
            tertiary = t.accentInk, onTertiary = t.onAccent,
            background = t.background, onBackground = t.ink,
            surface = t.background, onSurface = t.ink,
            surfaceVariant = t.surface2, onSurfaceVariant = t.ink2,
            surfaceContainerLowest = lowest, surfaceContainerLow = t.surface,
            surfaceContainer = t.surface, surfaceContainerHigh = t.surface2,
            surfaceContainerHighest = t.surface2,
            outline = t.outline, outlineVariant = outlineVariant,
            error = t.err, onError = t.onErr,
            inverseSurface = t.ink, inverseOnSurface = t.background,
            inversePrimary = t.accent, scrim = Color.Black,
        )
    } else {
        lightColorScheme(
            primary = t.ink, onPrimary = t.background,
            primaryContainer = t.accent, onPrimaryContainer = t.onAccent,
            secondary = t.ink2, onSecondary = t.surface,
            secondaryContainer = t.surface2, onSecondaryContainer = t.ink,
            tertiary = t.accentInk, onTertiary = Color.White,
            background = t.background, onBackground = t.ink,
            surface = t.background, onSurface = t.ink,
            surfaceVariant = t.surface2, onSurfaceVariant = t.ink2,
            surfaceContainerLowest = lowest, surfaceContainerLow = t.surface,
            surfaceContainer = t.surface, surfaceContainerHigh = t.surface2,
            surfaceContainerHighest = t.surface2,
            outline = t.outline, outlineVariant = outlineVariant,
            error = t.err, onError = t.onErr,
            inverseSurface = t.ink, inverseOnSurface = t.background,
            inversePrimary = t.accent, scrim = Color.Black,
        )
    }
}
