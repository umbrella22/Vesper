package io.github.umbrella22.vesper.player.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class VesperPlaybackSequenceWarmupTest {
    private fun intentJson(revision: Long = 1L): JSONObject =
        JSONObject(
            """
            {
              "sessionGeneration": 7,
              "itemId": "item-a",
              "sourceReference": "sequence-source-1",
              "sourceRevision": $revision,
              "warmupTaskId": ${100 + revision},
              "warmupGoal": "progressiveRange",
              "priority": "next",
              "cacheIdentity": {
                "canonicalKey": "vesper-sequence-cache:v1:17:example.provider:9:content-a:4:1080:5:media:6:public:$revision"
              },
              "profile": {
                "expectedMemoryBytes": 99999999,
                "warmupWindowMs": 1000
              }
            }
            """.trimIndent(),
        )

    @Test
    fun parserPreservesGoalAndRevisionInKey() {
        val first = VesperSequenceWarmupIntent.fromJson(intentJson())
        val second = VesperSequenceWarmupIntent.fromJson(intentJson(revision = 2L))

        assertNotNull(first)
        assertNotNull(second)
        assertNotEquals(first!!.key, second!!.key)
        assertEquals("progressiveRange", first.goal)
    }

    @Test
    fun parserRejectsUrlLikeOrUnknownIdentity() {
        val urlLike = intentJson().put("cacheIdentity", JSONObject().put("canonicalKey", "https://token"))
        val unknownPriority = intentJson().put("priority", "background")
        val unknownGoal = intentJson().put("warmupGoal", "decoderReady")

        assertNull(VesperSequenceWarmupIntent.fromJson(urlLike))
        assertNull(VesperSequenceWarmupIntent.fromJson(unknownPriority))
        assertNull(VesperSequenceWarmupIntent.fromJson(unknownGoal))
    }

}
