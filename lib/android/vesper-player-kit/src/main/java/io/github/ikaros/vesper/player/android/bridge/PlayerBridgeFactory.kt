package io.github.ikaros.vesper.player.android

import android.content.Context

internal object PlayerBridgeFactory {
    private val defaultBackend = PlayerBridgeBackend.VesperNativeStub

    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
        trackPreferencePolicy: VesperTrackPreferencePolicy = VesperTrackPreferencePolicy(),
        preloadBudgetPolicy: VesperPreloadBudgetPolicy = VesperPreloadBudgetPolicy(),
        decoderBackend: VesperDecoderBackend = VesperDecoderBackend.SystemOnly,
        surfaceKind: NativeVideoSurfaceKind = NativeVideoSurfaceKind.SurfaceView,
        keepScreenOnDuringPlayback: Boolean = true,
        benchmarkConfiguration: VesperBenchmarkConfiguration = VesperBenchmarkConfiguration.Disabled,
        sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration =
            VesperSourceNormalizerConfiguration(),
        frameProcessorConfiguration: VesperFrameProcessorConfiguration =
            VesperFrameProcessorConfiguration(),
        nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration =
            VesperNativeFramePipelineConfiguration(),
        pipelineEventHookConfiguration: VesperPipelineEventHookConfiguration =
            VesperPipelineEventHookConfiguration(),
    ): PlayerBridge =
        when (defaultBackend) {
            PlayerBridgeBackend.FakeDemo -> {
                require(pipelineEventHookConfiguration.pluginReferences.isEmpty()) {
                    "Fake demo players do not support Android playback event-hook references"
                }
                FakePlayerBridge(
                    initialSource = initialSource,
                    resiliencePolicy = resiliencePolicy,
                    trackPreferencePolicy = trackPreferencePolicy,
                    preloadBudgetPolicy = preloadBudgetPolicy,
                    keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                    benchmarkConfiguration = benchmarkConfiguration,
                    appContext = context.applicationContext,
                )
            }
            PlayerBridgeBackend.VesperNativeStub -> {
                val appContext = context.applicationContext
                val resolvedPluginArtifacts =
                    VesperBundledPluginResolver.resolve(
                        context = appContext,
                        sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                        frameProcessorConfiguration = frameProcessorConfiguration,
                        nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
                    )
                val pipelineEventHookRegistryOwner =
                    pipelineEventHookConfiguration.pluginReferences
                        .takeIf { it.isNotEmpty() }
                        ?.let { references ->
                            DefaultVesperPluginRegistryFactory.create(appContext, references)
                        }
                try {
                    val benchmarkRecorder =
                        VesperBenchmarkRecorder(
                            configuration = benchmarkConfiguration,
                            context = appContext,
                        )
                    VesperNativePlayerBridge(
                        bindings =
                            VesperNativeJniBindings(
                                context = appContext,
                                preloadBudgetPolicy = preloadBudgetPolicy,
                                decoderBackend = decoderBackend,
                                benchmarkRecorder = benchmarkRecorder,
                                sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                                resolvedPluginArtifacts = resolvedPluginArtifacts,
                                pipelineEventHookRegistryHandle =
                                    pipelineEventHookRegistryOwner?.handle ?: 0L,
                                pipelineEventHookReferencesJson =
                                    encodeVesperPluginReferences(
                                        pipelineEventHookConfiguration.pluginReferences,
                                    ),
                            ),
                        initialSource = initialSource,
                        currentResiliencePolicy = resiliencePolicy,
                        trackPreferencePolicy = trackPreferencePolicy,
                        preloadBudgetPolicy = preloadBudgetPolicy,
                        decoderBackend = decoderBackend,
                        benchmarkRecorder = benchmarkRecorder,
                        keepScreenOnDuringPlayback = keepScreenOnDuringPlayback,
                        appContext = appContext,
                        surfaceKind = surfaceKind,
                        sourceNormalizerConfiguration = sourceNormalizerConfiguration,
                        frameProcessorConfiguration = frameProcessorConfiguration,
                        nativeFramePipelineConfiguration = nativeFramePipelineConfiguration,
                        pipelineEventHookRegistryOwner = pipelineEventHookRegistryOwner,
                    )
                } catch (error: Throwable) {
                    pipelineEventHookRegistryOwner?.close()
                    throw error
                }
            }
        }
}
