package io.github.ikaros.vesper.player.android

import android.content.Context

object PlayerBridgeFactory {
    private val defaultBackend = PlayerBridgeBackend.VesperNativeStub

    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
        surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
    ): PlayerBridge =
        when (defaultBackend) {
            PlayerBridgeBackend.FakeDemo ->
                FakePlayerBridge(
                    initialSource = initialSource,
                    resiliencePolicy = resiliencePolicy,
                    trackPreferencePolicy = trackPreferencePolicy,
                    preloadBudgetPolicy = preloadBudgetPolicy,
                    appContext = context.applicationContext,
                )
            PlayerBridgeBackend.VesperNativeStub -> VesperNativePlayerBridge(
                bindings =
                    VesperNativeJniBindings(
                        context = context.applicationContext,
                        preloadBudgetPolicy = preloadBudgetPolicy,
                        decoderBackend = decoderBackend,
                    ),
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                trackPreferencePolicy = trackPreferencePolicy,
                preloadBudgetPolicy = preloadBudgetPolicy,
                decoderBackend = decoderBackend,
                appContext = context.applicationContext,
                surfaceKind = surfaceKind,
            )
        }
}
