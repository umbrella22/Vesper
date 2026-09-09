package io.github.umbrella22.vesper.player.android

import androidx.media3.common.C
import androidx.media3.datasource.ByteArrayDataSource
import androidx.media3.datasource.DataSource
import org.junit.Assert.assertSame
import org.junit.Test

class VesperHlsDataSourceFactoryTest {
    @Test
    fun manifestsBypassCacheWhileMediaUsesCachedFactory() {
        val manifestDataSource = ByteArrayDataSource(byteArrayOf(1))
        val mediaDataSource = ByteArrayDataSource(byteArrayOf(2))
        val factory =
            buildHlsPlaybackDataSourceFactory(
                manifestFactory = DataSource.Factory { manifestDataSource },
                mediaFactory = DataSource.Factory { mediaDataSource },
            )

        assertSame(manifestDataSource, factory.createDataSource(C.DATA_TYPE_MANIFEST))
        assertSame(mediaDataSource, factory.createDataSource(C.DATA_TYPE_MEDIA))
    }
}
