# Android JNI bridge 会按精确二进制类名解析此包内的类和成员。
# release shrink/obfuscation 需要保留整个 bridge surface。
-keep class io.github.ikaros.vesper.player.android.** {
    *;
}
