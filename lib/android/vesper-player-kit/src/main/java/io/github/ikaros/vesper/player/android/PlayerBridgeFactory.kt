package io.github.ikaros.vesper.player.android

import android.content.Context

object PlayerBridgeFactory {
    private val defaultBackend = PlayerBridgeBackend.VesperNativeStub

    fun createDefault(
        context: Context,
        initialSource: VesperPlayerSource? = null,
        resiliencePolicy: VesperPlaybackResiliencePolicy = VesperPlaybackResiliencePolicy(),
    ): PlayerBridge =
        when (defaultBackend) {
            PlayerBridgeBackend.FakeDemo ->
                FakePlayerBridge(
                    initialSource = initialSource,
                    appContext = context.applicationContext,
                )
            PlayerBridgeBackend.VesperNativeStub -> VesperNativePlayerBridge(
                bindings = VesperNativeJniBindings(context.applicationContext),
                initialSource = initialSource,
                resiliencePolicy = resiliencePolicy,
                appContext = context.applicationContext,
            )
        }
}
