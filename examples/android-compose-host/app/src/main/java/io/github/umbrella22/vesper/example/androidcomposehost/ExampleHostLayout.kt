package io.github.umbrella22.vesper.example.androidcomposehost

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Cast
import androidx.compose.material.icons.rounded.PictureInPictureAlt
import androidx.compose.material.icons.rounded.Subtitles
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** Width breakpoint (dp) at which the shell switches from a bottom bar to a rail. */
internal const val EXAMPLE_RAIL_MIN_WIDTH_DP = 600

/** Height breakpoint (dp) below which a landscape window plays immersively. */
internal const val EXAMPLE_COMPACT_HEIGHT_MAX_DP = 480

internal fun exampleUsesNavigationRail(screenWidthDp: Int): Boolean =
    screenWidthDp >= EXAMPLE_RAIL_MIN_WIDTH_DP

internal fun exampleIsCompactHeight(screenHeightDp: Int): Boolean =
    screenHeightDp < EXAMPLE_COMPACT_HEIGHT_MAX_DP

/** Material 3 margin between the FAB and the bottom bar (matches Scaffold's FAB spacing). */
internal val ExampleFabSpacing: Dp = 16.dp

/** Breathing room between the last list item and whatever is drawn below it. */
internal val ExampleListEndSpacing: Dp = 16.dp

/**
 * Extra bottom space a scrolling destination needs so its last item clears the
 * floating action button. [fabHeight] is the measured FAB height; zero means the
 * FAB has not been measured yet or is not shown.
 */
internal fun exampleFabClearance(fabVisible: Boolean, fabHeight: Dp): Dp =
    if (fabVisible && fabHeight > 0.dp) fabHeight + ExampleFabSpacing else 0.dp

/**
 * Keeps the top and side edges of the shell padding and drops the bottom edge,
 * so a destination can draw under the FAB / bottom bar and consume the bottom
 * inset itself as scrollable content padding.
 */
@Composable
internal fun PaddingValues.exampleWithoutBottom(): PaddingValues {
    val layoutDirection = LocalLayoutDirection.current
    return PaddingValues(
        start = calculateStartPadding(layoutDirection),
        top = calculateTopPadding(),
        end = calculateEndPadding(layoutDirection),
        bottom = 0.dp,
    )
}

/**
 * Shared scrolling container for every destination of the host shell.
 * [contentPadding] is the shell padding: Scaffold inner padding with the FAB
 * clearance already added to its bottom edge. Only the bottom edge is applied
 * here so the list scrolls up to, but never under, the FAB and bottom bar.
 */
@Composable
internal fun ExampleHostList(
    state: LazyListState,
    contentPadding: PaddingValues,
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        state = state,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = 16.dp,
            top = 8.dp,
            end = 16.dp,
            bottom = contentPadding.calculateBottomPadding() + ExampleListEndSpacing,
        ),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        content = content,
    )
}

/** Picture-in-picture, cast, and tracks shortcuts shown under the Play stage. */
@Composable
internal fun ExamplePlayQuickActions(
    onPictureInPicture: () -> Unit,
    onCast: () -> Unit,
    onTracks: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        ExampleQuickActionButton(
            icon = Icons.Rounded.PictureInPictureAlt,
            label = stringResource(R.string.example_action_pip),
            onClick = onPictureInPicture,
            modifier = Modifier.weight(1f),
        )
        ExampleQuickActionButton(
            icon = Icons.Rounded.Cast,
            label = stringResource(R.string.example_action_cast),
            onClick = onCast,
            modifier = Modifier.weight(1f),
        )
        ExampleQuickActionButton(
            icon = Icons.Rounded.Subtitles,
            label = stringResource(R.string.example_action_tracks),
            onClick = onTracks,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun ExampleQuickActionButton(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier,
) {
    // Icon above label, matching the prototype's quick-action tiles, so longer
    // English labels such as "Picture-in-picture" fit without truncation.
    FilledTonalButton(
        onClick = onClick,
        modifier = modifier.heightIn(min = 80.dp),
        shape = MaterialTheme.shapes.medium,
        contentPadding = PaddingValues(horizontal = 4.dp, vertical = 8.dp),
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(icon, contentDescription = null)
            Text(
                text = label,
                modifier = Modifier.padding(top = 6.dp),
                style = MaterialTheme.typography.labelLarge,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
