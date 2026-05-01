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
        benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
    ): PlayerBridge =
        when (defaultBackend) {
            PlayerBridgeBackend.FakeDemo ->
                FakePlayerBridge(
                    initialSource = initialSource,
                    resiliencePolicy = resiliencePolicy,
                    trackPreferencePolicy = trackPreferencePolicy,
                    preloadBudgetPolicy = preloadBudgetPolicy,
                    benchmarkConfiguration = benchmarkConfiguration,
                    appContext = context.applicationContext,
                )
            PlayerBridgeBackend.VesperNativeStub -> {
                val benchmarkRecorder = VesperBenchmarkRecorder(benchmarkConfiguration)
                VesperNativePlayerBridge(
                    bindings =
                        VesperNativeJniBindings(
                            context = context.applicationContext,
                            preloadBudgetPolicy = preloadBudgetPolicy,
                            decoderBackend = decoderBackend,
                            benchmarkRecorder = benchmarkRecorder,
                        ),
                    initialSource = initialSource,
                    currentResiliencePolicy = resiliencePolicy,
                    trackPreferencePolicy = trackPreferencePolicy,
                    preloadBudgetPolicy = preloadBudgetPolicy,
                    decoderBackend = decoderBackend,
                    benchmarkRecorder = benchmarkRecorder,
                    appContext = context.applicationContext,
                    surfaceKind = surfaceKind,
                )
            }
        }
}
