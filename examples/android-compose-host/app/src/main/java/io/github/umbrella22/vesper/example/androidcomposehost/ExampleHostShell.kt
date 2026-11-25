package io.github.umbrella22.vesper.example.androidcomposehost

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.PlayCircle
import androidx.compose.material.icons.rounded.Science
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material.icons.rounded.VideoLibrary
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

internal val ExampleHostDestination.icon: ImageVector
    get() =
        when (this) {
            ExampleHostDestination.Play -> Icons.Rounded.PlayCircle
            ExampleHostDestination.Library -> Icons.Rounded.VideoLibrary
            ExampleHostDestination.Downloads -> Icons.Rounded.Download
            ExampleHostDestination.Lab -> Icons.Rounded.Science
        }

/**
 * Adaptive host shell: a bottom [NavigationBar] on compact widths and a
 * [NavigationRail] on medium / expanded widths. The accent color is used only by
 * the selected indicator pill and the "Open media" FAB.
 */
@Composable
internal fun ExampleHostShell(
    navState: ExampleHostNavState,
    useRail: Boolean,
    showOpenMedia: Boolean,
    openMediaExpanded: Boolean,
    snackbarHostState: SnackbarHostState,
    onSelectDestination: (ExampleHostDestination) -> Unit,
    onBack: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenMedia: () -> Unit,
    content: @Composable (PaddingValues) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Row(modifier = Modifier.fillMaxSize()) {
        if (useRail) {
            NavigationRail(
                containerColor = colors.surfaceContainerLow,
                header = {
                    if (showOpenMedia) {
                        FloatingActionButton(
                            onClick = onOpenMedia,
                            containerColor = colors.primaryContainer,
                            contentColor = colors.onPrimaryContainer,
                        ) {
                            Icon(Icons.Rounded.Add, stringResource(R.string.example_action_open_media))
                        }
                    }
                },
            ) {
                Spacer(Modifier.height(12.dp))
                ExampleHostDestination.entries.forEach { destination ->
                    NavigationRailItem(
                        selected = navState.destination == destination,
                        onClick = { onSelectDestination(destination) },
                        icon = { Icon(destination.icon, contentDescription = null) },
                        label = { Text(stringResource(destination.labelRes)) },
                        colors = NavigationRailItemDefaults.colors(
                            indicatorColor = colors.primaryContainer,
                            selectedIconColor = colors.onPrimaryContainer,
                        ),
                    )
                }
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            // The Scaffold FAB floats above the bottom bar, so its measured height
            // (identical when expanded or collapsed to "+") plus the Material 3 FAB
            // margin is added to the bottom of the padding handed to destinations.
            val density = LocalDensity.current
            var fabHeightPx by remember { mutableIntStateOf(0) }
            val fabVisible = showOpenMedia && !useRail
            val fabHeight = with(density) { fabHeightPx.toDp() }
            Scaffold(
                containerColor = colors.background,
                topBar = { ExampleHostTopBar(navState, onBack, onOpenSettings) },
                snackbarHost = { SnackbarHost(snackbarHostState) },
                floatingActionButton = {
                    if (fabVisible) {
                        ExtendedFloatingActionButton(
                            onClick = onOpenMedia,
                            modifier = Modifier.onSizeChanged { fabHeightPx = it.height },
                            expanded = openMediaExpanded,
                            icon = { Icon(Icons.Rounded.Add, contentDescription = null) },
                            text = { Text(stringResource(R.string.example_action_open_media)) },
                            containerColor = colors.primaryContainer,
                            contentColor = colors.onPrimaryContainer,
                        )
                    }
                },
                bottomBar = {
                    if (!useRail) {
                        NavigationBar(containerColor = colors.surfaceContainerLow) {
                            ExampleHostDestination.entries.forEach { destination ->
                                NavigationBarItem(
                                    selected = navState.destination == destination,
                                    onClick = { onSelectDestination(destination) },
                                    icon = { Icon(destination.icon, contentDescription = null) },
                                    label = { Text(stringResource(destination.labelRes)) },
                                    colors = NavigationBarItemDefaults.colors(
                                        indicatorColor = colors.primaryContainer,
                                        selectedIconColor = colors.onPrimaryContainer,
                                    ),
                                )
                            }
                        }
                    }
                },
            ) { innerPadding ->
                val layoutDirection = LocalLayoutDirection.current
                val clearance = exampleFabClearance(fabVisible, fabHeight)
                content(
                    PaddingValues(
                        start = innerPadding.calculateStartPadding(layoutDirection),
                        top = innerPadding.calculateTopPadding(),
                        end = innerPadding.calculateEndPadding(layoutDirection),
                        bottom = innerPadding.calculateBottomPadding() + clearance,
                    ),
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ExampleHostTopBar(
    navState: ExampleHostNavState,
    onBack: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val collection =
        navState.libraryCollection.takeIf { navState.destination == ExampleHostDestination.Library }
    TopAppBar(
        title = {
            Text(
                text = collection?.let { stringResource(it.titleRes) }
                    ?: stringResource(R.string.example_app_title),
            )
        },
        navigationIcon = {
            if (collection != null) {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.example_action_back))
                }
            }
        },
        actions = {
            IconButton(onClick = onOpenSettings) {
                Icon(Icons.Rounded.Settings, stringResource(R.string.example_action_settings))
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
    )
}
