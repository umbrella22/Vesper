package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BrightnessAuto
import androidx.compose.material.icons.rounded.DarkMode
import androidx.compose.material.icons.rounded.LightMode
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.max

@Composable
internal fun ExamplePlayerHeader(
    sourceLabel: String,
    subtitle: String,
    palette: ExampleHostPalette,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = "Vesper",
            style = MaterialTheme.typography.headlineLarge.copy(
                color = palette.title,
                fontWeight = FontWeight.Black,
                letterSpacing = (-1.2).sp,
            ),
        )
        Text(
            text = sourceLabel,
            style = MaterialTheme.typography.titleSmall.copy(
                color = palette.title,
                fontWeight = FontWeight.SemiBold,
            ),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodyMedium.copy(color = palette.body),
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
internal fun ExampleSourceSection(
    palette: ExampleHostPalette,
    themeMode: ExampleThemeMode,
    remoteStreamUrl: String,
    onThemeModeChange: (ExampleThemeMode) -> Unit,
    onRemoteStreamUrlChange: (String) -> Unit,
    onPickVideo: () -> Unit,
    onUseHlsDemo: () -> Unit,
    onUseDashDemo: () -> Unit,
    onOpenRemote: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(palette.sectionBackground, RoundedCornerShape(24.dp))
            .border(1.dp, palette.sectionStroke, RoundedCornerShape(24.dp))
            .padding(18.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(
            text = stringResource(R.string.example_sources_title),
            style = MaterialTheme.typography.titleMedium.copy(
                color = palette.title,
                fontWeight = FontWeight.Bold,
            ),
        )
        Text(
            text = stringResource(R.string.example_sources_subtitle),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )

        Row(
            modifier = Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            OutlinedButton(onClick = onPickVideo) {
                Text(stringResource(R.string.example_sources_pick_video))
            }
            OutlinedButton(onClick = onUseHlsDemo) {
                Text(stringResource(R.string.example_sources_hls_demo))
            }
            OutlinedButton(onClick = onUseDashDemo) {
                Text(stringResource(R.string.example_sources_dash_demo))
            }
        }

        OutlinedTextField(
            value = remoteStreamUrl,
            onValueChange = onRemoteStreamUrlChange,
            modifier = Modifier.fillMaxWidth(),
            label = { Text(stringResource(R.string.example_sources_remote_stream_url)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            singleLine = true,
        )

        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                text = stringResource(R.string.example_sources_theme),
                style = MaterialTheme.typography.labelLarge.copy(
                    color = palette.title,
                    fontWeight = FontWeight.SemiBold,
                ),
            )
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                ThemeModeChip(
                    icon = Icons.Rounded.BrightnessAuto,
                    label = stringResource(ExampleThemeMode.System.titleRes),
                    selected = themeMode == ExampleThemeMode.System,
                    onClick = { onThemeModeChange(ExampleThemeMode.System) },
                )
                ThemeModeChip(
                    icon = Icons.Rounded.LightMode,
                    label = stringResource(ExampleThemeMode.Light.titleRes),
                    selected = themeMode == ExampleThemeMode.Light,
                    onClick = { onThemeModeChange(ExampleThemeMode.Light) },
                )
                ThemeModeChip(
                    icon = Icons.Rounded.DarkMode,
                    label = stringResource(ExampleThemeMode.Dark.titleRes),
                    selected = themeMode == ExampleThemeMode.Dark,
                    onClick = { onThemeModeChange(ExampleThemeMode.Dark) },
                )
            }
        }

        Button(
            onClick = onOpenRemote,
            colors = ButtonDefaults.buttonColors(
                containerColor = palette.primaryAction,
                contentColor = Color.White,
            ),
        ) {
            Text(stringResource(R.string.example_sources_open_remote_url))
        }
    }
}

@Composable
internal fun ExampleResilienceSection(
    palette: ExampleHostPalette,
    selectedProfile: ExampleResilienceProfile,
    isApplyingProfile: Boolean,
    onApplyProfile: (ExampleResilienceProfile) -> Unit,
) {
    val policy = selectedProfile.policy
    ExampleSectionShell(
        palette = palette,
        title = stringResource(R.string.example_resilience_title),
        subtitle = stringResource(R.string.example_resilience_subtitle),
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            AdaptiveChipWrap(
                modifier = Modifier.fillMaxWidth(),
                horizontalSpacing = 10.dp,
                verticalSpacing = 10.dp,
            ) {
                ExampleResilienceProfile.values().forEach { profile ->
                    SelectionChip(
                        label = stringResource(profile.titleRes),
                        selected = profile == selectedProfile,
                        onClick = { onApplyProfile(profile) },
                    )
                }
            }

            Text(
                text = stringResource(selectedProfile.subtitleRes),
                style = MaterialTheme.typography.bodyMedium.copy(
                    color = palette.body,
                    lineHeight = 22.sp,
                ),
            )

            if (isApplyingProfile) {
                ExampleStatusPill(
                    label = stringResource(R.string.example_resilience_applying),
                    palette = palette,
                )
            }

            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                ExampleFactRow(
                    label = stringResource(R.string.example_resilience_fact_buffering),
                    value = resilienceBufferingValue(policy.buffering),
                    palette = palette,
                )
                ExampleFactRow(
                    label = stringResource(R.string.example_resilience_fact_retry),
                    value = resilienceRetryValue(policy.retry),
                    palette = palette,
                )
                ExampleFactRow(
                    label = stringResource(R.string.example_resilience_fact_cache),
                    value = resilienceCacheValue(policy.cache),
                    palette = palette,
                )
            }
        }
    }
}

@Composable
internal fun ThemeModeChip(
    icon: ImageVector,
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    TextButton(
        onClick = onClick,
        colors = ButtonDefaults.textButtonColors(
            contentColor = if (selected) Color.White else MaterialTheme.colorScheme.onSurface,
        ),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
        modifier = Modifier
            .heightIn(min = 38.dp)
            .background(
                if (selected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.surface.copy(alpha = 0.72f)
                },
                RoundedCornerShape(999.dp),
            ),
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
        )
        Spacer(modifier = Modifier.width(6.dp))
        Text(label, maxLines = 1)
    }
}

@Composable
private fun SelectionChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    TextButton(
        onClick = onClick,
        colors = ButtonDefaults.textButtonColors(
            contentColor = if (selected) Color.White else MaterialTheme.colorScheme.onSurface,
        ),
        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp),
        modifier = Modifier
            .heightIn(min = 38.dp)
            .background(
                if (selected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.surface.copy(alpha = 0.72f)
                },
                RoundedCornerShape(999.dp),
            ),
    ) {
        Text(label, maxLines = 1)
    }
}

@Composable
private fun AdaptiveChipWrap(
    modifier: Modifier = Modifier,
    horizontalSpacing: androidx.compose.ui.unit.Dp,
    verticalSpacing: androidx.compose.ui.unit.Dp,
    content: @Composable () -> Unit,
) {
    Layout(
        modifier = modifier,
        content = content,
    ) { measurables, constraints ->
        val horizontalSpacingPx = horizontalSpacing.roundToPx()
        val verticalSpacingPx = verticalSpacing.roundToPx()
        val maxWidth = constraints.maxWidth.takeIf { it < Int.MAX_VALUE } ?: Int.MAX_VALUE
        val placeables = measurables.map { measurable ->
            measurable.measure(constraints.copy(minWidth = 0, minHeight = 0))
        }

        data class PositionedPlaceable(
            val placeable: androidx.compose.ui.layout.Placeable,
            val x: Int,
            val y: Int,
        )

        val positionedPlaceables = mutableListOf<PositionedPlaceable>()
        var currentX = 0
        var currentY = 0
        var currentRowHeight = 0
        var contentWidth = 0

        placeables.forEach { placeable ->
            val shouldWrap =
                currentX > 0 &&
                    currentX + placeable.width > maxWidth
            if (shouldWrap) {
                currentX = 0
                currentY += currentRowHeight + verticalSpacingPx
                currentRowHeight = 0
            }

            positionedPlaceables +=
                PositionedPlaceable(
                    placeable = placeable,
                    x = currentX,
                    y = currentY,
                )
            currentX += placeable.width + horizontalSpacingPx
            currentRowHeight = max(currentRowHeight, placeable.height)
            contentWidth = max(contentWidth, currentX - horizontalSpacingPx)
        }

        val widthCandidate =
            if (constraints.maxWidth < Int.MAX_VALUE) {
                constraints.maxWidth
            } else {
                contentWidth
            }
        val heightCandidate =
            if (positionedPlaceables.isEmpty()) {
                0
            } else {
                currentY + currentRowHeight
            }
        val layoutWidth = widthCandidate.coerceIn(constraints.minWidth, constraints.maxWidth)
        val layoutHeight = heightCandidate.coerceIn(constraints.minHeight, constraints.maxHeight)

        layout(layoutWidth, layoutHeight) {
            positionedPlaceables.forEach { positioned ->
                positioned.placeable.placeRelative(positioned.x, positioned.y)
            }
        }
    }
}

@Composable
private fun ExampleSectionShell(
    palette: ExampleHostPalette,
    title: String,
    subtitle: String,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(palette.sectionBackground, RoundedCornerShape(24.dp))
            .border(1.dp, palette.sectionStroke, RoundedCornerShape(24.dp))
            .padding(18.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium.copy(
                color = palette.title,
                fontWeight = FontWeight.Bold,
            ),
        )
        Spacer(modifier = Modifier.size(8.dp))
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodySmall.copy(
                color = palette.body,
                lineHeight = 20.sp,
            ),
        )
        Spacer(modifier = Modifier.size(14.dp))
        Box(
            modifier = Modifier
                .width(42.dp)
                .height(4.dp)
                .background(Color(0xFF172033), RoundedCornerShape(999.dp)),
        )
        Spacer(modifier = Modifier.size(16.dp))
        content()
    }
}

@Composable
private fun ExampleFactRow(
    label: String,
    value: String,
    palette: ExampleHostPalette,
) {
    Row(modifier = Modifier.padding(vertical = 6.dp)) {
        Text(
            text = label,
            modifier = Modifier.width(92.dp),
            style = MaterialTheme.typography.bodyMedium.copy(color = palette.body),
        )
        Spacer(modifier = Modifier.width(10.dp))
        Text(
            text = value,
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodyMedium.copy(
                color = palette.title,
                fontWeight = FontWeight.SemiBold,
            ),
        )
    }
}

@Composable
private fun ExampleStatusPill(
    label: String,
    palette: ExampleHostPalette,
) {
    Text(
        text = label,
        modifier = Modifier
            .background(
                palette.primaryAction.copy(alpha = 0.12f),
                RoundedCornerShape(999.dp),
            )
            .padding(horizontal = 12.dp, vertical = 7.dp),
        style = MaterialTheme.typography.labelMedium.copy(
            color = palette.primaryAction,
            fontWeight = FontWeight.SemiBold,
        ),
    )
}
