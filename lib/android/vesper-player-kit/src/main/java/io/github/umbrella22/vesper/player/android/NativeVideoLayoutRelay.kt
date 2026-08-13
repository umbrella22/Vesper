package io.github.umbrella22.vesper.player.android

internal class NativeVideoLayoutRelay {
    @Volatile
    var current: NativeVideoLayoutInfo? = null
        private set

    private var listener: ((NativeVideoLayoutInfo?) -> Unit)? = null

    fun update(layoutInfo: NativeVideoLayoutInfo?) {
        current = layoutInfo
        listener?.invoke(layoutInfo)
    }

    fun setListener(listener: ((NativeVideoLayoutInfo?) -> Unit)?) {
        this.listener = listener
        listener?.invoke(current)
    }
}
