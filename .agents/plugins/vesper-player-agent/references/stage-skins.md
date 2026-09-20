# Vesper Stage Skin Contract

## Version and placement

The Stage skin API is introduced in 0.6.4. Check the consumer's resolved package
and the current source before proposing it. Packages at 0.6.3 or earlier cannot
replace built-in transport icons through this API; upgrade to 0.6.4 or later
or implement the SDK change in its own checkout.

The maintained migration guide is `lib/doc/stage-skins.md` when a Vesper checkout
is available. This card carries the integration rules for installed plugin
copies that have no repository documentation.

| Host | UI module | Default icon resource | Custom renderer |
| --- | --- | --- | --- |
| Flutter | `vesper_player_ui` | `IconData` | `iconBuilder(context, role, style) -> Widget?` |
| Compose | `vesper-player-kit-compose-ui` | `ImageVector` | `iconContent(role) -> (@Composable (style) -> Unit)?` |
| SwiftUI | `VesperPlayerKitUI` | SF Symbols name | `iconBuilder(role, style) -> AnyView?` on the main actor |

Flutter uses the Dart skin even on Android and iOS. Native image types, resource
identifiers, widget trees, and loaders remain in their UI module. Do not add
skin state to Rust models, FFI, JNI, player controllers, or Flutter channels.

## Selection and rendering

Every Stage accepts `skin: VesperPlayerStageSkin?`. A host chooses its custom
skin or null/nil from application state. Null/nil establishes SDK defaults for
that Stage even inside a parent theme. Disabling a skin restores default
appearance; hiding controls uses the existing presentation/visibility API.

`VesperPlayerStageSkin` contains `icons`, `colors`, `metrics`, and the optional
platform renderer. Constructors supply defaults, so overrides may be partial.
The renderer's null/nil result falls back to the configured icon collection.

`VesperStageIconRole` describes the available action or feedback: `play`,
`pause`, `fullscreen`, `exitFullscreen`, `navigateBack`, `more`, `brightness`,
`volume`, and `speed`. Kotlin uses PascalCase enum cases. While playing, the
action role is pause; in fullscreen, it is exitFullscreen. Do not derive a role
from the glyph name or reuse a play asset for both playback states.

Custom renderers receive a resolved size and color. Return decorative visual
content and use host asset bundles or supported native loaders for images/SVG.
The SDK supplies action callbacks, labels, semantics, and hit testing. Custom
content must not create duplicate accessibility nodes, intercept button
actions, or intercept HUD-area Stage gestures. Keep renderer work bounded and
synchronous; remote skin downloads or executable skin packages are not part of
this API. Assets requiring RTL mirroring implement it in the host renderer.

`VesperStageButtonVariant` contains `standard`, `toolbar`, `navigation`,
`compact`, `compactFullscreen`, `expanded`, `expandedFullscreen`, and `primary`.
An explicit `style` overrides the selected variant. `VesperStageButtonStyle`
provides `size`, `iconSize`, `backgroundOpacity`, and `borderRadius`.
Visual size does not reduce the minimum icon-button bounds: 48 logical pixels
on Flutter, 48 dp on Android, and 44 points on iOS. Check row fit after changing
sizes or spacing; there is no automatic control wrapping.

`VesperStageMetrics` adds horizontal `buttonSpacing`, `hudIconSize`,
`hudBorderRadius`, `timelineTrackHeight`, `timelineThumbSize`, and
`timelineLargeThumbSize`. `VesperStageColors` supplies foreground/secondary
foreground, background/scrim/button background, accent, timeline start/end/
inactive/thumb, and HUD background/foreground. Control opacity multiplies the
configured color alpha. Default dimensions follow each platform's existing
visuals; they are not a promise of identical glyph geometry across platforms.

## Host controls and migration to 0.6.4

Stage slots inherit their Stage's skin. Standalone controls use these scopes:

- Flutter: `VesperPlayerStageTheme(skin: skin, child: control)`;
  `VesperPlayerStageTheme.of(context)` reads it.
- Compose: `VesperPlayerStageTheme(skin) { control() }`;
  `LocalVesperPlayerStageSkin.current` reads it.
- SwiftUI: `control.vesperPlayerStageSkin(skin)`;
  `@Environment(\.vesperPlayerStageSkin)` reads it.

Flutter `VesperStageIconButton.icon` changes from `IconData` to `Widget`.
Replace `icon: Icons.favorite` with `icon: const Icon(Icons.favorite)`.
Replace `size`, `iconSize`, and `containerAlpha` with a named variant or an
explicit style. The mechanical equivalent is:

```dart
VesperStageIconButton(
  icon: const Icon(Icons.favorite),
  label: 'Favorite',
  style: const VesperStageButtonStyle(
    size: 38, iconSize: 23, backgroundOpacity: 0,
  ),
  onPressed: onFavorite,
)
```

Use `variant: VesperStageButtonVariant.toolbar` to follow the skin's toolbar
style. Use `VesperStageIcon(VesperStageIconRole.play)` when a host action should
use the configured SDK role renderer. `VesperStagePrimaryPlayButton` replaces
its size/iconSize arguments with `style` and accepts localized `strings`.
Tests tap the action button or semantic label, not decorative icon content.

Compose exposes `VesperStageIconButton(label, variant, onClick) { icon() }`
and `VesperStagePrimaryPlayButton`. A generic icon slot inherits
`LocalContentColor` and a bounded box; semantic `VesperStageIcon` also receives
the resolved icon style. Recompile Kotlin consumers for the extended Stage
signature; old compiled default-argument calls are not binary compatible.

SwiftUI exposes `VesperStageIconButton(label:variant:action:) { image }` and
`VesperStagePrimaryPlayButton`. Rebuild consumers with the matching UI framework
or Swift package after the Stage initializer change. Host asset catalogs stay
in the host; SF Symbols are configured by name. Primary buttons accept a
`label` override; skin configuration does not localize text.

## Playback and platform boundaries

Changing skins must preserve controller, surface, media source, and Stage
identity and must not call play, pause, seek, prepare, or dispose. Do not key
the controller/surface by skin or recreate them in a settings toggle. Layout
changes still follow existing gesture cancellation rules.

The API covers Stage chrome, timeline, HUD, and reusable buttons. System PiP,
Android media notifications, lock-screen controls, native AirPlay pickers,
and host-owned sheets have independent styling. Skin changes do not modify
platform playback capability or enable a different media backend.

## Evidence requirements

Cover partial overrides and null fallback; play/pause and fullscreen transitions
in both layouts; all three HUD roles; standalone controls and injected slots;
minimum touch bounds; accessible labels/actions; non-default spacing and sizes;
and runtime switching without controller/surface recreation or player commands.

For custom interactive content supplied accidentally, verify both physical taps
and accessibility clicks still reach the SDK button. A custom HUD icon must not
consume taps or drags intended for the Stage. A native compile is not input or
accessibility execution evidence. Use `validation-contract.md` for the package
and native UI test entrypoints and report missing device execution explicitly.

Resolve SDK accessibility labels from the application's localized resources in
Android tests. Hard-coded English labels fail on hosts configured for another
supported language. Keep physical input and semantic actions as separate checks:
Compose `performClick()` injects touch input on Android; use
`performSemanticsAction(SemanticsActions.OnClick)` to verify the labeled node's
accessibility action. Preview-controller UI tests can prove control behavior
and retained identity without establishing media decode or playback evidence.

SwiftUI buttons expose one accessibility element with the SDK label, button
trait, and default action while excluding decorative descendants. Standalone
icons and HUD icons also exclude their custom view descendants from the
accessibility tree. Keep scrims, borders, and HUD presentation out of hit
testing. Validate this with nested interactive icon content, taps inside the
minimum bounds but outside the visual frame, and a HUD-area tap while feedback
is actually visible. Record HUD visibility at touch-down: single-tap callbacks
can follow feedback dismissal while waiting for double-tap recognition. Use the
separate `VesperPlayerKitUIInteractionTests` scheme for touch and accessibility-tree
checks; the rendering tests in `VesperPlayerKitUITests` cannot establish them.
