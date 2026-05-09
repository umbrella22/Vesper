# Rust JNI resolves Android bridge classes and members by exact binary names.
# Release shrinking and obfuscation must not rename or remove these classes or members.
-keep class io.github.ikaros.vesper.player.android.** {
    *;
}
