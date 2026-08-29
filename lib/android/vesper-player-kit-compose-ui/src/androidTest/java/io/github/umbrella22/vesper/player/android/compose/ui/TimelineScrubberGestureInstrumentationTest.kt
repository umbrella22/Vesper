package io.github.umbrella22.vesper.player.android.compose.ui

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.down
import androidx.compose.ui.test.moveTo
import androidx.compose.ui.test.up
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TimelineScrubberGestureInstrumentationTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun dragContinuesWhenSiblingTimelineTextChangesScrubberWidth() {
        val previews = mutableListOf<Float>()
        val commits = mutableListOf<Float>()

        composeRule.setContent {
            var displayedRatio by remember { mutableFloatStateOf(0.1f) }
            var expandedSummary by remember { mutableStateOf(false) }

            Row(modifier = Modifier.width(360.dp)) {
                TimelineScrubber(
                    modifier = Modifier
                        .weight(1f)
                        .testTag(SCRUBBER_TAG),
                    displayedRatio = displayedRatio,
                    compact = true,
                    onSeekPreview = { ratio ->
                        previews += ratio
                        displayedRatio = ratio
                        expandedSummary = true
                    },
                    onSeekCommit = { ratio -> commits += ratio },
                    onSeekCancel = {},
                )
                Text(if (expandedSummary) "00:00/03:13" else "0")
            }
        }

        val scrubber = composeRule.onNodeWithTag(SCRUBBER_TAG)
        scrubber.performTouchInput {
            val y = centerY
            down(Offset(width * 0.1f, y))
            moveTo(Offset(width * 0.35f, y + 1f), delayMillis = 80L)
        }
        composeRule.waitForIdle()
        scrubber.performTouchInput {
            val y = centerY
            moveTo(Offset(width * 0.8f, y - 1f), delayMillis = 80L)
            up()
        }
        composeRule.waitForIdle()

        assertTrue("drag preview should reach the final pointer position: $previews", previews.last() > 0.7f)
        assertEquals("one completed drag should commit exactly once", 1, commits.size)
        assertTrue("drag commit should use the final pointer position: $commits", commits.single() > 0.7f)
    }

    private companion object {
        const val SCRUBBER_TAG = "timeline-scrubber"
    }
}
