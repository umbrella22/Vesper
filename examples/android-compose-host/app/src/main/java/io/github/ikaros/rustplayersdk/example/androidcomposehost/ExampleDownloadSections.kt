package io.github.ikaros.vesper.example.androidcomposehost

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.ikaros.vesper.player.android.VesperDownloadState
import io.github.ikaros.vesper.player.android.VesperDownloadTaskSnapshot

internal data class ExamplePendingDownloadTask(
    val requestId: String,
    val assetId: String,
    val label: String,
    val sourceUri: String,
)

@Composable
internal fun ExampleDownloadHeader(
    palette: ExampleHostPalette,
    isDownloadExportPluginInstalled: Boolean,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = "Vesper Download",
            style = MaterialTheme.typography.headlineLarge.copy(
                color = palette.title,
                fontWeight = FontWeight.Black,
                letterSpacing = (-1.2).sp,
            ),
        )
        Text(
            text = stringResource(R.string.example_download_header_subtitle),
            style = MaterialTheme.typography.bodyMedium.copy(color = palette.body),
        )
        Text(
            text =
                stringResource(
                    if (isDownloadExportPluginInstalled) {
                        R.string.example_download_export_plugin_ready
                    } else {
                        R.string.example_download_export_plugin_missing
                    },
                ),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )
    }
}

@Composable
internal fun ExampleDownloadCreateSection(
    palette: ExampleHostPalette,
    remoteUrl: String,
    onRemoteUrlChange: (String) -> Unit,
    onUseHlsDemo: () -> Unit,
    onUseDashDemo: () -> Unit,
    onCreateRemote: () -> Unit,
) {
    ExampleSectionShell(
        palette = palette,
        title = stringResource(R.string.example_download_create_title),
        subtitle = stringResource(R.string.example_download_create_subtitle),
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OutlinedButton(onClick = onUseHlsDemo) {
                    Text(stringResource(R.string.example_download_hls_demo))
                }
                OutlinedButton(onClick = onUseDashDemo) {
                    Text(stringResource(R.string.example_download_dash_demo))
                }
            }

            OutlinedTextField(
                value = remoteUrl,
                onValueChange = onRemoteUrlChange,
                modifier = Modifier.fillMaxWidth(),
                label = { Text(stringResource(R.string.example_download_remote_url)) },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                singleLine = true,
            )

            Button(
                onClick = onCreateRemote,
                colors = ButtonDefaults.buttonColors(
                    containerColor = palette.primaryAction,
                    contentColor = Color.White,
                ),
            ) {
                Text(stringResource(R.string.example_download_create_remote_task))
            }
        }
    }
}

@Composable
internal fun ExampleDownloadTasksSection(
    palette: ExampleHostPalette,
    tasks: List<VesperDownloadTaskSnapshot>,
    pendingTasks: List<ExamplePendingDownloadTask>,
    isDownloadExportPluginInstalled: Boolean,
    savingTaskIds: Set<Long>,
    exportProgressByTaskId: Map<Long, Float>,
    onPrimaryAction: (VesperDownloadTaskSnapshot) -> Unit,
    onSaveToGallery: (VesperDownloadTaskSnapshot) -> Unit,
    onRemoveTask: (VesperDownloadTaskSnapshot) -> Unit,
) {
    val visibleTasks = tasks.filter { task -> task.state != VesperDownloadState.Removed }
    ExampleSectionShell(
        palette = palette,
        title = stringResource(R.string.example_download_tasks_title),
        subtitle = stringResource(R.string.example_download_tasks_subtitle),
    ) {
        if (visibleTasks.isEmpty() && pendingTasks.isEmpty()) {
            Text(
                text = stringResource(R.string.example_download_empty),
                style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
            )
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                pendingTasks.reversed().forEach { pendingTask ->
                    ExamplePendingDownloadTaskRow(
                        palette = palette,
                        task = pendingTask,
                    )
                }
                visibleTasks.reversed().forEach { task ->
                    ExampleDownloadTaskRow(
                        palette = palette,
                        task = task,
                        isDownloadExportPluginInstalled = isDownloadExportPluginInstalled,
                        isSaving = savingTaskIds.contains(task.taskId),
                        exportProgress = exportProgressByTaskId[task.taskId],
                        onPrimaryAction = { onPrimaryAction(task) },
                        onSaveToGallery = { onSaveToGallery(task) },
                        onRemoveTask = { onRemoveTask(task) },
                    )
                }
            }
        }
    }
}

@Composable
private fun ExamplePendingDownloadTaskRow(
    palette: ExampleHostPalette,
    task: ExamplePendingDownloadTask,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(palette.fieldBackground, RoundedCornerShape(20.dp))
            .border(1.dp, palette.sectionStroke, RoundedCornerShape(20.dp))
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = task.label,
            style = MaterialTheme.typography.bodyLarge.copy(
                color = palette.title,
                fontWeight = FontWeight.SemiBold,
            ),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text =
                stringResource(
                    R.string.example_download_task_meta,
                    task.assetId,
                    stringResource(R.string.example_download_state_preparing),
                ),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )
        LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        Text(
            text = stringResource(R.string.example_download_pending_task_details),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )
        Text(
            text = stringResource(R.string.example_download_pending_source_uri, task.sourceUri),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun ExampleDownloadTaskRow(
    palette: ExampleHostPalette,
    task: VesperDownloadTaskSnapshot,
    isDownloadExportPluginInstalled: Boolean,
    isSaving: Boolean,
    exportProgress: Float?,
    onPrimaryAction: () -> Unit,
    onSaveToGallery: () -> Unit,
    onRemoveTask: () -> Unit,
) {
    val primaryActionLabel = primaryActionLabel(task.state)
    val canSaveToGallery =
        task.state == VesperDownloadState.Completed &&
            !task.assetIndex.completedPath.isNullOrBlank()
    val requiresExport =
        task.source.contentFormat == io.github.ikaros.vesper.player.android.VesperDownloadContentFormat.HlsSegments ||
            task.source.contentFormat == io.github.ikaros.vesper.player.android.VesperDownloadContentFormat.DashSegments
    val saveButtonVisuallyUnavailable =
        requiresExport && !isDownloadExportPluginInstalled && !isSaving
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(palette.fieldBackground, RoundedCornerShape(20.dp))
            .border(1.dp, palette.sectionStroke, RoundedCornerShape(20.dp))
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = task.source.source.label,
            style = MaterialTheme.typography.bodyLarge.copy(
                color = palette.title,
                fontWeight = FontWeight.SemiBold,
            ),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            text =
                stringResource(
                    R.string.example_download_task_meta,
                    task.assetId,
                    downloadStateLabel(task.state),
                ),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )
        Text(
            text = progressSummary(task),
            style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
        )
        if (isSaving && exportProgress != null) {
            LinearProgressIndicator(
                progress = { exportProgress.coerceIn(0f, 1f) },
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                text =
                    stringResource(
                        R.string.example_download_export_progress,
                        (exportProgress.coerceIn(0f, 1f) * 100f).toInt(),
                    ),
                style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
            )
        }
        task.assetIndex.completedPath?.takeIf { it.isNotBlank() }?.let { path ->
            Text(
                text = stringResource(R.string.example_download_completed_path, path),
                style = MaterialTheme.typography.bodySmall.copy(color = palette.body),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        task.error?.message?.takeIf { it.isNotBlank() }?.let { message ->
            Text(
                text = stringResource(R.string.example_download_error_message, message),
                style = MaterialTheme.typography.bodySmall.copy(color = Color(0xFFC13C36)),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            if (primaryActionLabel != null) {
                Button(
                    onClick = onPrimaryAction,
                    enabled = !isSaving,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = palette.primaryAction,
                        contentColor = Color.White,
                    ),
                ) {
                    Text(primaryActionLabel)
                }
            }
            if (canSaveToGallery) {
                OutlinedButton(
                    onClick = onSaveToGallery,
                    enabled = !isSaving,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
                    colors =
                        ButtonDefaults.outlinedButtonColors(
                            contentColor =
                                if (saveButtonVisuallyUnavailable) {
                                    palette.body.copy(alpha = 0.55f)
                                } else {
                                    palette.title
                                },
                        ),
                ) {
                    Text(
                        stringResource(
                            if (isSaving && exportProgress != null) {
                                R.string.example_download_exporting
                            } else {
                                R.string.example_download_save_to_gallery
                            },
                        ),
                    )
                }
            }
            TextButton(
                onClick = onRemoveTask,
                enabled = !isSaving,
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 10.dp),
            ) {
                Text(stringResource(R.string.example_download_remove_task))
            }
            Spacer(modifier = Modifier.width(0.dp))
        }
    }
}

@Composable
private fun downloadStateLabel(state: VesperDownloadState): String =
    when (state) {
        VesperDownloadState.Queued -> stringResource(R.string.example_download_state_queued)
        VesperDownloadState.Preparing -> stringResource(R.string.example_download_state_preparing)
        VesperDownloadState.Downloading -> stringResource(R.string.example_download_state_downloading)
        VesperDownloadState.Paused -> stringResource(R.string.example_download_state_paused)
        VesperDownloadState.Completed -> stringResource(R.string.example_download_state_completed)
        VesperDownloadState.Failed -> stringResource(R.string.example_download_state_failed)
        VesperDownloadState.Removed -> stringResource(R.string.example_download_state_removed)
    }

@Composable
private fun primaryActionLabel(state: VesperDownloadState): String? =
    when (state) {
        VesperDownloadState.Queued,
        VesperDownloadState.Failed,
        -> stringResource(R.string.example_download_action_start)
        VesperDownloadState.Preparing,
        VesperDownloadState.Downloading,
        -> stringResource(R.string.example_download_action_pause)
        VesperDownloadState.Paused -> stringResource(R.string.example_download_action_resume)
        VesperDownloadState.Completed,
        VesperDownloadState.Removed,
        -> null
    }

@Composable
private fun progressSummary(task: VesperDownloadTaskSnapshot): String {
    val ratio = task.progress.completionRatio
    val ratioText =
        ratio?.let { progress ->
            "${(progress * 100f).toInt()}%"
        } ?: stringResource(R.string.example_download_progress_unknown)
    val bytesText =
        stringResource(
            R.string.example_download_progress_bytes,
            formatBytes(task.progress.receivedBytes),
            formatBytes(task.progress.totalBytes),
        )
    return "$ratioText · $bytesText"
}
