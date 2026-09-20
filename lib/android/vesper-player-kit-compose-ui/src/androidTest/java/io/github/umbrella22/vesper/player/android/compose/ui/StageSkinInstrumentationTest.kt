package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.umbrella22.vesper.player.android.PlaybackStateUi
import io.github.umbrella22.vesper.player.android.VesperPlayerControllerFactory
import io.github.umbrella22.vesper.player.android.VesperTrackCatalog
import io.github.umbrella22.vesper.player.android.VesperTrackSelectionSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class StageSkinInstrumentationTest {
    @get:Rule val rule = createComposeRule()

    @Test
    fun customIconCannotReplaceActionAndSmallVisualRetainsHitArea() {
        var actions = 0
        var childActions = 0
        var receivedStyle: VesperStageIconStyle? = null
        val skin = VesperPlayerStageSkin(
            colors = VesperStageColors(foreground = Color.Green),
            metrics = VesperStageMetrics(toolbar = VesperStageButtonStyle(size = 20.dp, iconSize = 12.dp)),
            iconContent = { role ->
                if (role == VesperStageIconRole.Play) { style ->
                    receivedStyle = style
                    Box(Modifier.size(style.size).clickable { childActions++ }) { Text("Icon") }
                } else null
            },
        )
        rule.setContent {
            VesperPlayerStageTheme(skin) {
                VesperStageIconButton(label = "Play", variant = VesperStageButtonVariant.Toolbar, onClick = { actions++ }) {
                    VesperStageIcon(VesperStageIconRole.Play)
                }
            }
        }
        rule.onNodeWithContentDescription("Play")
            .assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performTouchInput { click() }
        rule.onNodeWithContentDescription("Play")
            .performSemanticsAction(SemanticsActions.OnClick) { action -> assertTrue(action()) }
        rule.onNodeWithText("Icon").assertDoesNotExist()
        rule.runOnIdle {
            assertEquals(2, actions)
            assertEquals(0, childActions)
            assertEquals(VesperStageIconStyle(12.dp, Color.Green), receivedStyle)
        }
    }

    @Test
    fun bothLayoutsReactToSkinAndPlaybackStateWithoutRecreatingController() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val playLabel = context.getString(R.string.vesper_player_stage_play)
        val pauseLabel = context.getString(R.string.vesper_player_stage_pause)
        val fullscreenLabel = context.getString(R.string.vesper_player_stage_fullscreen)
        val exitFullscreenLabel = context.getString(R.string.vesper_player_stage_exit_fullscreen)
        val layout = mutableStateOf(VesperStageControlLayout.Compact)
        val playing = mutableStateOf(true)
        val fullscreen = mutableStateOf(false)
        val custom = mutableStateOf(true)
        val pip = mutableStateOf(false)
        val seen = mutableSetOf<VesperStageIconRole>()
        var creations = 0
        var disposals = 0
        var actions = 0
        var precedingLabel = ""
        val skin = VesperPlayerStageSkin(metrics = VesperStageMetrics(buttonSpacing = 20.dp), iconContent = { role ->
            seen.add(role)
            null // Exercise fallback through every built-in action role.
        })
        rule.setContent {
            val controller = remember { creations++; VesperPlayerControllerFactory.createPreview() }
            DisposableEffect(controller) { onDispose { disposals++; controller.dispose() } }
            precedingLabel = if (layout.value == VesperStageControlLayout.Compact) {
                compactTimelineSummary(controller.uiState.value.timeline, null)
            } else {
                qualityButtonLabel(VesperTrackCatalog.Empty, VesperTrackSelectionSnapshot())
            }
            VesperPlayerStage(
                controller = controller,
                uiState = controller.uiState.value.copy(playbackState = if (playing.value) PlaybackStateUi.Playing else PlaybackStateUi.Paused),
                controlsVisible = true, pendingSeekRatio = null,
                controlLayout = layout.value, isFullscreen = fullscreen.value,
                modifier = Modifier.size(360.dp, 300.dp), pictureInPicturePresentation = pip.value,
                onControlsVisibilityChange = {}, onPendingSeekRatioChange = {},
                onOpenSheet = { actions++ }, onToggleFullscreen = { actions++ },
                onTogglePlayback = { actions++ }, onNavigateBack = { actions++ },
                skin = if (custom.value) skin else null,
            )
        }
        for (variant in VesperStageControlLayout.entries) {
            rule.runOnIdle { layout.value = variant; playing.value = true; fullscreen.value = false }
            rule.onNodeWithContentDescription(pauseLabel)
                .performSemanticsAction(SemanticsActions.OnClick) { action -> assertTrue(action()) }
            val fullscreenBounds = rule.onNodeWithContentDescription(fullscreenLabel).fetchSemanticsNode().boundsInRoot
            val precedingBounds = rule.onNodeWithText(precedingLabel).fetchSemanticsNode().boundsInRoot
            assertEquals(with(rule.density) { 20.dp.toPx() }, fullscreenBounds.left - precedingBounds.right, 1f)
            rule.runOnIdle {
                assertTrue(seen.containsAll(listOf(VesperStageIconRole.Pause, VesperStageIconRole.Fullscreen,
                    VesperStageIconRole.NavigateBack, VesperStageIconRole.More)))
                playing.value = false; fullscreen.value = true
            }
            rule.onNodeWithContentDescription(playLabel).assertExists()
            rule.onNodeWithContentDescription(exitFullscreenLabel).assertExists()
        }
        rule.runOnIdle { custom.value = false }
        rule.onNodeWithContentDescription(playLabel).assertExists()
        rule.runOnIdle {
            assertTrue(seen.containsAll(listOf(VesperStageIconRole.Play, VesperStageIconRole.ExitFullscreen)))
            assertEquals(1, creations); assertEquals(0, disposals); assertEquals(2, actions)
            pip.value = true
        }
        rule.onNodeWithContentDescription(playLabel).assertDoesNotExist()
    }

    @Test
    fun customHudIconCannotInterceptStageTap() {
        rule.mainClock.autoAdvance = false
        var childActions = 0
        var hudCenter: Offset? = null
        val visibilityRequests = mutableListOf<Boolean>()
        val skin = VesperPlayerStageSkin(iconContent = { role ->
            if (role == VesperStageIconRole.Brightness) { style ->
                Box(Modifier.size(style.size)
                    .onGloballyPositioned { hudCenter = it.boundsInRoot().center }
                    .clickable { childActions++ })
            } else null
        })
        rule.setContent {
            val controller = remember { VesperPlayerControllerFactory.createPreview() }
            DisposableEffect(controller) { onDispose { controller.dispose() } }
            VesperPlayerStage(
                controller = controller,
                uiState = controller.uiState.value.copy(playbackState = PlaybackStateUi.Playing),
                controlsVisible = false, pendingSeekRatio = null,
                controlLayout = VesperStageControlLayout.Compact, isFullscreen = false,
                modifier = Modifier.size(360.dp, 300.dp).testTag("stage"),
                onControlsVisibilityChange = { visibilityRequests.add(it) },
                onPendingSeekRatioChange = {}, onOpenSheet = {}, onToggleFullscreen = {},
                currentBrightnessRatio = { 0.5f }, onSetBrightnessRatio = { it }, skin = skin,
            )
        }
        rule.mainClock.advanceTimeByFrame()
        rule.onNodeWithTag("stage").performTouchInput {
            swipe(Offset(width * 0.25f, height * 0.65f), Offset(width * 0.25f, height * 0.3f))
        }
        rule.mainClock.advanceTimeBy(48)
        val stageOrigin = rule.onNodeWithTag("stage").fetchSemanticsNode().boundsInRoot.topLeft
        val tapPosition = requireNotNull(hudCenter) - stageOrigin
        rule.runOnIdle { visibilityRequests.clear() }
        rule.onNodeWithTag("stage").performTouchInput { click(tapPosition) }
        rule.mainClock.advanceTimeBy(350)
        rule.runOnIdle {
            assertEquals(0, childActions)
            assertEquals(listOf(true), visibilityRequests)
        }
    }

    @Test
    fun hudAndStandalonePrimaryButtonConsumeTheSameSkin() {
        val seen = mutableMapOf<VesperStageIconRole, Color>()
        rule.setContent {
            VesperPlayerStageTheme(VesperPlayerStageSkin(
                colors = VesperStageColors(foreground = Color.Green, hudForeground = Color.Cyan),
                iconContent = { role -> { style -> seen[role] = style.color; Box(Modifier.size(style.size)) } },
            )) {
                Column {
                    VesperStagePrimaryPlayButton(isPlaying = true, onClick = {})
                    for (kind in StageGestureKind.entries) {
                        StageGestureFeedbackPanel(StageGestureFeedback(kind, 0.5f, "50%"))
                    }
                }
            }
        }
        rule.runOnIdle {
            assertEquals(Color.Green, seen[VesperStageIconRole.Pause])
            for (role in listOf(VesperStageIconRole.Brightness, VesperStageIconRole.Volume, VesperStageIconRole.Speed)) {
                assertEquals(Color.Cyan, seen[role])
            }
        }
    }
}
