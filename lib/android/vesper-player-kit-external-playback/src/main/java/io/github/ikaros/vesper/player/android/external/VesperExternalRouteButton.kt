package io.github.ikaros.vesper.player.android.external

import android.content.Context
import android.content.res.Configuration
import android.os.Bundle
import android.util.AttributeSet
import android.view.ContextThemeWrapper
import androidx.mediarouter.app.MediaRouteButton
import androidx.mediarouter.app.MediaRouteChooserDialog
import androidx.mediarouter.app.MediaRouteChooserDialogFragment
import androidx.mediarouter.app.MediaRouteControllerDialog
import androidx.mediarouter.app.MediaRouteControllerDialogFragment
import androidx.mediarouter.app.MediaRouteDialogFactory
import androidx.mediarouter.app.MediaRouteDynamicChooserDialog
import androidx.mediarouter.app.MediaRouteDynamicControllerDialog
import com.google.android.gms.cast.framework.CastButtonFactory

class VesperExternalRouteButton @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : MediaRouteButton(routeContext(context, null), attrs) {
    init {
        CastButtonFactory.setUpMediaRouteButton(context, this)
        dialogFactory = VesperExternalRouteButtonDialogFactory(routeTheme(context, null).buttonTheme)
        setAlwaysVisible(true)
    }

    companion object {
        fun create(
            context: Context,
            brightness: VesperExternalRouteButtonBrightness? = null,
        ): MediaRouteButton {
            val theme = routeTheme(context, brightness)
            val themedContext = ContextThemeWrapper(context, theme.buttonTheme)
            return MediaRouteButton(themedContext).also { button ->
                CastButtonFactory.setUpMediaRouteButton(themedContext, button)
                button.dialogFactory = VesperExternalRouteButtonDialogFactory(theme.buttonTheme)
                button.setAlwaysVisible(true)
            }
        }
    }
}

enum class VesperExternalRouteButtonBrightness {
    Light,
    Dark,
}

private fun routeContext(
    context: Context,
    brightness: VesperExternalRouteButtonBrightness?,
): Context =
    ContextThemeWrapper(context, routeTheme(context, brightness).buttonTheme)

private fun routeTheme(
    context: Context,
    brightness: VesperExternalRouteButtonBrightness?,
): RouteTheme =
    when (brightness) {
        VesperExternalRouteButtonBrightness.Dark -> RouteTheme.Dark
        VesperExternalRouteButtonBrightness.Light -> RouteTheme.Light
        null -> if (context.resources.configuration.isNightMode) {
            RouteTheme.Dark
        } else {
            RouteTheme.Light
        }
    }

private val Configuration.isNightMode: Boolean
    get() = uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES

private const val ROUTE_DIALOG_THEME_ARGUMENT = "routeDialogTheme"

private data class RouteTheme(
    val buttonTheme: Int,
) {
    companion object {
        val Light = RouteTheme(R.style.VesperPlayerExternalRouteButtonTheme_Light)
        val Dark = RouteTheme(R.style.VesperPlayerExternalRouteButtonTheme_Dark)
    }
}

private class VesperExternalRouteButtonDialogFactory(
    private val routeDialogTheme: Int,
) : MediaRouteDialogFactory() {
    override fun onCreateChooserDialogFragment(): MediaRouteChooserDialogFragment =
        VesperExternalRouteChooserDialogFragment.newInstance(routeDialogTheme)

    override fun onCreateControllerDialogFragment(): MediaRouteControllerDialogFragment =
        VesperExternalRouteControllerDialogFragment.newInstance(routeDialogTheme)
}

class VesperExternalRouteChooserDialogFragment : MediaRouteChooserDialogFragment() {
    override fun onCreateChooserDialog(
        context: Context,
        savedInstanceState: Bundle?,
    ): MediaRouteChooserDialog =
        MediaRouteChooserDialog(routeContext(context), routeDialogTheme())

    override fun onCreateDynamicChooserDialog(context: Context): MediaRouteDynamicChooserDialog =
        MediaRouteDynamicChooserDialog(routeContext(context), routeDialogTheme())

    private fun routeContext(context: Context): Context =
        ContextThemeWrapper(context, routeDialogTheme())

    private fun routeDialogTheme(): Int =
        arguments?.getInt(ROUTE_DIALOG_THEME_ARGUMENT, 0)
            ?.takeIf { it != 0 }
            ?: R.style.VesperPlayerExternalRouteButtonTheme_Light

    companion object {
        fun newInstance(routeDialogTheme: Int): VesperExternalRouteChooserDialogFragment =
            VesperExternalRouteChooserDialogFragment().apply {
                arguments = Bundle().apply {
                    putInt(ROUTE_DIALOG_THEME_ARGUMENT, routeDialogTheme)
                }
            }
    }
}

class VesperExternalRouteControllerDialogFragment : MediaRouteControllerDialogFragment() {
    override fun onCreateControllerDialog(
        context: Context,
        savedInstanceState: Bundle?,
    ): MediaRouteControllerDialog =
        MediaRouteControllerDialog(routeContext(context), routeDialogTheme())

    override fun onCreateDynamicControllerDialog(context: Context): MediaRouteDynamicControllerDialog =
        MediaRouteDynamicControllerDialog(routeContext(context), routeDialogTheme())

    private fun routeContext(context: Context): Context =
        ContextThemeWrapper(context, routeDialogTheme())

    private fun routeDialogTheme(): Int =
        arguments?.getInt(ROUTE_DIALOG_THEME_ARGUMENT, 0)
            ?.takeIf { it != 0 }
            ?: R.style.VesperPlayerExternalRouteButtonTheme_Light

    companion object {
        fun newInstance(routeDialogTheme: Int): VesperExternalRouteControllerDialogFragment =
            VesperExternalRouteControllerDialogFragment().apply {
                arguments = Bundle().apply {
                    putInt(ROUTE_DIALOG_THEME_ARGUMENT, routeDialogTheme)
                }
            }
    }
}
