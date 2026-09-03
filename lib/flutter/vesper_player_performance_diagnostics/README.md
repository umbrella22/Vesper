# vesper_player_performance_diagnostics

Optional native dependency package for Vesper Player's official bounded
performance diagnostics plugin. It adds no MethodChannel or Dart runtime
behavior; the diagnostics session API remains in `vesper_player`.

On Android, this package resolves the matching
`vesper-player-kit-performance-diagnostics` Maven artifact. On iOS, it resolves
the matching `VesperPlayerPerformanceDiagnostics` product from the remote
`VesperPlayerKit` Swift package.

The package is not part of the default `vesper_player` dependency graph. Add it
only to build variants that should carry the diagnostics binary. The plugin
aggregates bounded frame and playback metrics locally. It does not upload data
or read media URLs, request headers, cookies, account data, overlay text, or raw
error messages.
