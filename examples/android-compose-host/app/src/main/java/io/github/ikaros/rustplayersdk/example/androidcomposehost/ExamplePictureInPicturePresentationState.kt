package io.github.ikaros.vesper.example.androidcomposehost

internal data class ExamplePictureInPicturePresentationState(
    val presentation: Boolean = false,
    val active: Boolean = false,
    val pendingAutoEnter: Boolean = false,
)

internal fun ExamplePictureInPicturePresentationState.onPictureInPictureRequestStarted():
    ExamplePictureInPicturePresentationState =
    copy(presentation = true, pendingAutoEnter = false)

internal fun ExamplePictureInPicturePresentationState.onPictureInPictureUserLeaveHint(
    enabled: Boolean,
): ExamplePictureInPicturePresentationState =
    if (enabled) {
        copy(presentation = true, pendingAutoEnter = true)
    } else {
        this
    }

internal fun ExamplePictureInPicturePresentationState.onPictureInPictureModeChanged(
    isInPictureInPictureMode: Boolean,
): ExamplePictureInPicturePresentationState =
    if (isInPictureInPictureMode) {
        copy(presentation = true, active = true, pendingAutoEnter = false)
    } else {
        copy(presentation = false, active = false, pendingAutoEnter = false)
    }

internal fun ExamplePictureInPicturePresentationState.onPictureInPictureRequestRejected():
    ExamplePictureInPicturePresentationState =
    copy(presentation = false, active = false, pendingAutoEnter = false)

internal fun ExamplePictureInPicturePresentationState.onPictureInPictureAutoEnterTimeout():
    ExamplePictureInPicturePresentationState =
    if (pendingAutoEnter && !active) {
        copy(presentation = false, pendingAutoEnter = false)
    } else {
        this
    }
