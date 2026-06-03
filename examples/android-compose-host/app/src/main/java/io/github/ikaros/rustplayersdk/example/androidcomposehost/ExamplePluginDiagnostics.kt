package io.github.ikaros.vesper.example.androidcomposehost

internal fun exampleSourceNormalizerDiagnostics(
    pluginDiagnostics: List<Map<String, Any?>>,
): List<Map<String, Any?>> =
    pluginDiagnostics.filter { diagnostic ->
        diagnostic["pluginKind"] == "source_normalizer" ||
            diagnostic["status"]?.toString()?.startsWith("sourceNormalizer") == true
    }

internal fun exampleFrameProcessorDiagnostics(
    pluginDiagnostics: List<Map<String, Any?>>,
): List<Map<String, Any?>> =
    pluginDiagnostics.filter { diagnostic ->
        diagnostic["pluginKind"] == "frame_processor" ||
            diagnostic["status"]?.toString()?.startsWith("frameProcessor") == true
    }

internal fun exampleNativeFramePipelineDiagnostics(
    pluginDiagnostics: List<Map<String, Any?>>,
): List<Map<String, Any?>> =
    pluginDiagnostics.filter { diagnostic ->
        diagnostic["pluginKind"] == "native_frame_pipeline"
    }
