# vesper_player_macos

The macOS implementation package for `vesper_player`.

> Experimental: this package does not yet ship a real playback backend. Its API
> behavior and capability matrix are not fully aligned with the mobile
> implementations, so it should not be considered production-ready.

## Current State

```dart
abstract final class VesperPlayerMacosPackage {
  static const bool isImplemented = false;
}
```

- Package structure and registration hooks are already in place
- No real playback backend is wired yet
- Unsupported operations are reported through `VesperPlayerCapabilities`
- No dedicated CI path exists yet

## Planned Direction

The macOS backend is expected to stay native-first and use AVFoundation. The
next useful milestone is validating the basic control loop, including local
files, basic streaming, and the core state pipeline, before filling in the
remaining capabilities gradually.

## Related Resources

- Main package: `vesper_player`
- Platform contract: `vesper_player_platform_interface`
