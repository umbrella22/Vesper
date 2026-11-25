package io.github.umbrella22.vesper.example.androidcomposehost

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/** Host-owned modal bottom sheet using the themed surface (not the dark stage sheet). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ExampleHostBottomSheet(
    title: String,
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 24.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            content()
        }
    }
}

/** Settings sheet reached from the top bar gear: appearance and about. */
@Composable
internal fun ExampleSettingsSheet(
    themeMode: ExampleThemeMode,
    onThemeModeChange: (ExampleThemeMode) -> Unit,
    onDismiss: () -> Unit,
) {
    ExampleHostBottomSheet(
        title = stringResource(R.string.example_action_settings),
        onDismiss = onDismiss,
    ) {
        Text(
            text = stringResource(R.string.example_settings_appearance),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        ExampleThemeModeSelector(themeMode = themeMode, onThemeModeChange = onThemeModeChange)
        Text(
            text = stringResource(R.string.example_settings_about),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private const val RTSP_LAB_ACTIVITY_CLASS =
    "io.github.umbrella22.vesper.example.androidcomposehost.RtspLabActivity"

/**
 * Returns an intent for the debug-only RTSP Lab activity, or `null` when the
 * current build does not ship it (release builds).
 */
internal fun Context.exampleRtspLabIntent(): Intent? {
    val debuggable = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
    if (!debuggable) return null
    val intent = Intent().setClassName(packageName, RTSP_LAB_ACTIVITY_CLASS)
    return intent.takeIf { runCatching { packageManager.resolveActivity(it, 0) }.getOrNull() != null }
}

/** Lab entry card for the RTSP Lab; disabled with an explanatory label in release. */
@Composable
internal fun ExampleRtspLabEntry(
    available: Boolean,
    onOpen: () -> Unit,
) {
    ExampleListCard(
        title = stringResource(R.string.example_lab_rtsp_title),
        body = stringResource(R.string.example_lab_rtsp_subtitle),
        meta = null,
        onClick = if (available) onOpen else null,
        trailing = {
            AssistChip(
                onClick = onOpen,
                enabled = available,
                label = { Text(stringResource(R.string.example_lab_rtsp_badge)) },
            )
        },
    )
}
