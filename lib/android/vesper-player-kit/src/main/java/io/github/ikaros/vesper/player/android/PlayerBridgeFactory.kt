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
                    ),
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                trackPreferencePolicy = trackPreferencePolicy,
                preloadBudgetPolicy = preloadBudgetPolicy,
                appContext = context.applicationContext,
                surfaceKind = surfaceKind,
            )
        }
}
