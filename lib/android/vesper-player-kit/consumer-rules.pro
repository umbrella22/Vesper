# Rust JNI 会按精确二进制类名解析 Android bridge 类和成员。
# release shrink/obfuscation 不能重命名或移除这些类与成员。
-keep class io.github.ikaros.vesper.player.android.** {
    *;
}
