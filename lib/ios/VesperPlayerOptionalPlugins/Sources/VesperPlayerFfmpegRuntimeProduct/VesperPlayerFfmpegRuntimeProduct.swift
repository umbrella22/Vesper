/// Swift Package Manager marker that keeps the shared FFmpeg component
/// frameworks in the selected app product.
public enum VesperPlayerFfmpegRuntimeProduct {
    public static let frameworkNames = [
        "VesperFFmpegAVCodec",
        "VesperFFmpegAVFormat",
        "VesperFFmpegAVUtil",
    ]
}
