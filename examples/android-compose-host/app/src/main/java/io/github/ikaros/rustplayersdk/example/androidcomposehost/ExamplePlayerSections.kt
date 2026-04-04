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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

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
