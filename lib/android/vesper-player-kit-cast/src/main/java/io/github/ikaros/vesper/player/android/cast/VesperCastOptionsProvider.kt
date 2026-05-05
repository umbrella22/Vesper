package io.github.ikaros.vesper.player.android.cast

import android.content.Context
import android.content.pm.PackageManager
import com.google.android.gms.cast.CastMediaControlIntent
import com.google.android.gms.cast.framework.CastOptions
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionProvider

class VesperCastOptionsProvider : OptionsProvider {
    override fun getCastOptions(context: Context): CastOptions {
        val receiverId =
            context.applicationInfoMetadataString(METADATA_RECEIVER_APPLICATION_ID)
                ?: CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID
        return CastOptions.Builder()
            .setReceiverApplicationId(receiverId)
            .build()
    }

    override fun getAdditionalSessionProviders(context: Context): List<SessionProvider>? = null

    private fun Context.applicationInfoMetadataString(key: String): String? =
        runCatching {
            packageManager
                .getApplicationInfo(packageName, PackageManager.GET_META_DATA)
                .metaData
                ?.getString(key)
                ?.takeIf(String::isNotBlank)
        }.getOrNull()

    public companion object {
        public const val METADATA_RECEIVER_APPLICATION_ID: String =
            "io.github.ikaros.vesper.player.android.cast.RECEIVER_APPLICATION_ID"
    }
}
