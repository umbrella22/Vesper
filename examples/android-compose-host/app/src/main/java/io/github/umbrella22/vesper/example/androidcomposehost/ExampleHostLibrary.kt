package io.github.umbrella22.vesper.example.androidcomposehost

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** A built-in demo stream that the library can play directly. */
internal data class ExampleLibraryEntry(
    val itemId: String,
    val titleRes: Int,
    val url: String,
)

internal fun exampleLibraryEntries(collection: ExampleLibraryCollection): List<ExampleLibraryEntry> =
    when (collection) {
        ExampleLibraryCollection.Adaptive ->
            listOf(
                ExampleLibraryEntry(
                    ANDROID_HLS_PLAYLIST_ITEM_ID,
                    R.string.example_source_hls_demo_label,
                    ANDROID_HLS_DEMO_URL,
                ),
                ExampleLibraryEntry(
                    ANDROID_DASH_PLAYLIST_ITEM_ID,
                    R.string.example_source_dash_demo_label,
                    ANDROID_DASH_DEMO_URL,
                ),
            )
        ExampleLibraryCollection.Live ->
            listOf(
                ExampleLibraryEntry(
                    ANDROID_LIVE_DVR_PLAYLIST_ITEM_ID,
                    R.string.example_source_live_dvr_acceptance_label,
                    ANDROID_LIVE_DVR_ACCEPTANCE_URL,
                ),
            )
        ExampleLibraryCollection.DolbyAcceptance -> emptyList()
    }

/** Library home: one card per collection; Dolby shows its preset count. */
internal fun LazyListScope.exampleLibraryHome(
    dolbyPresetCount: Int,
    onOpenCollection: (ExampleLibraryCollection) -> Unit,
) {
    items(ExampleLibraryCollection.entries, key = { it.name }) { collection ->
        val count =
            if (collection == ExampleLibraryCollection.DolbyAcceptance) {
                dolbyPresetCount
            } else {
                exampleLibraryEntries(collection).size
            }
        ExampleListCard(
            title = stringResource(collection.titleRes),
            body = stringResource(collection.descriptionRes),
            meta = pluralStringResource(R.plurals.example_library_item_count, count, count),
            onClick = { onOpenCollection(collection) },
            trailing = {
                Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null)
            },
        )
    }
}

/** Secondary page for the non-Dolby collections. */
internal fun LazyListScope.exampleLibraryCollectionEntries(
    collection: ExampleLibraryCollection,
    onPlay: (ExampleLibraryEntry) -> Unit,
) {
    item(key = "desc") {
        Text(
            text = stringResource(collection.descriptionRes),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    items(exampleLibraryEntries(collection), key = { it.itemId }) { entry ->
        ExampleListCard(
            title = stringResource(entry.titleRes),
            body = entry.url,
            meta = null,
            onClick = { onPlay(entry) },
            trailing = {
                FilledTonalButton(onClick = { onPlay(entry) }) {
                    Icon(Icons.Rounded.PlayArrow, contentDescription = null)
                    Text(stringResource(R.string.example_library_play))
                }
            },
        )
    }
}

@Composable
internal fun ExampleListCard(
    title: String,
    body: String,
    meta: String?,
    onClick: (() -> Unit)?,
    trailing: @Composable () -> Unit = {},
) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(12.dp)
    val content: @Composable () -> Unit = {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                Text(
                    text = body,
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                meta?.let {
                    Text(it, style = MaterialTheme.typography.labelSmall, color = colors.tertiary)
                }
            }
            trailing()
        }
    }
    val border = BorderStroke(1.dp, colors.outlineVariant)
    if (onClick != null) {
        Surface(onClick = onClick, shape = shape, color = colors.surfaceContainerLow, border = border) {
            content()
        }
    } else {
        Surface(shape = shape, color = colors.surfaceContainerLow, border = border) { content() }
    }
}
