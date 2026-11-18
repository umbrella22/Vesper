# @vesper/player-kit-arkui-ui

The optional `VesperPlayerStage` presents playback controls, captions, settings,
locking, fullscreen intent and PiP over `@vesper/player-kit-arkui`. Dependencies
flow from UI to binding to core. All three packages use `0.9.1-harmony.0`.

```ts
VesperPlayerStage({
  binding: this.binding,
  title: 'Movie',
  controlLayout: this.controlLayout,
  isFullscreen: this.fullscreen,
  pictureInPicture: this.pip,
  onFullscreenChange: (value: boolean): void => { this.setFullscreen(value); },
  onRetry: (): void => { this.reloadSource(); }
})
```

Retain one binding across `standard`/`compact`/`expanded` control layout and fullscreen
changes. `isFullscreen` is host state; `onFullscreenChange` requests a native
window change. Retry is also host policy. The component does not register a new
source or recreate a controller when its presentation changes.
`title` updates with the host's current queue item. Hosts that configure PiP
on demand can supply `onPictureInPicture` instead of an already-created
`pictureInPicture` owner. The callback takes precedence and lets the host create
or start PiP without copying its resource owner into reactive presentation state.
Compact uses a single transport row with an unspaced `00:07/03:13` clock; only
insufficient space for a 56 vp scrubber moves the time onto its own line.
Expanded adds time and a 28 vp native scrubber above the bottom play/shortcut
row. Compact scrubbers occupy 44 vp within the button row. Icon buttons retain
their independent targets of at least 44 vp in both layouts. Actual window
insets, button dimensions and native text measurements determine the layout.
Short windows and large text can force compact geometry; the secondary header
badge disappears when it would crowd the transport. Standard retains the classic
central controls only when the entire cluster fits between header and footer.
The expanded row reserves each button and gap before admitting shortcuts,
prioritizing quality over speed when both cannot fit. Hidden shortcuts remain
available in More. Font size is never reduced to fit the footer.
PiP, lock, volume and track selection remain reachable from settings.

Controls use native `SymbolGlyph($r('sys.symbol.…'))` resources. Playing controls
hide after three seconds without interaction. Pausing, completion, settings,
locking, timeline dragging, keyboard focus and an active accessibility service
cancel hiding. Tap the video to reveal controls;
buttons and sliders own their touch regions separately from background gestures.
Double tap the video to play or pause. Hold a playing VOD for 450 ms to use 2×
temporarily; release or cancellation restores the captured rate. Source changes
discard restoration for the old playback epoch. Geometry, layout, fullscreen,
locking, settings, disappearance and presentation suppression cancel gestures.
Set `interactionEnabled: false` while the host presents another page or native
PiP. The example derives this input from its current page and actual PiP state.

`skin` accepts an optional `VesperPlayerStageSkin`. Omitting it, or replacing it
with `undefined`, restores that Stage's own default palette and metrics. Replace
the skin during playback to change colors, button geometry and caption size;
skin changes issue no player commands and retain the binding and Surface.

```ts
const colors = new VesperStageColors();
colors.buttonBackground = '#087f72';
colors.accent = '#35d8ba';
const skin = new VesperPlayerStageSkin(colors, new VesperStageMetrics(44, 8, 16, 20));
// Pass skin alongside binding and the existing Stage inputs.
```

Palette fields cover foreground/secondary text, Surface background, scrim,
buttons, accent, timeline track/thumb, settings, errors, captions and gesture HUD.
Metrics use viewport units and require finite dimensions, a button height of at
least 44, nonnegative spacing/radius and positive caption size. The host must
choose dimensions that fit its window. Labels and callbacks stay independent of
the skin. Icon buttons expose accessible labels and retain at least 44 vp touch
targets. Default controls use translucent white roles, a warm accent and a pink
to gold timeline. `VesperPlayerStageIcons` overrides individual system resources;
unreplaced roles retain their defaults. `iconContent: wrapBuilder(builder)` plus
`customIconRoles` supplies bounded decorative content for selected roles. Input,
labels, focus, hit targets and callbacks continue to belong to the SDK button.
Icon dimensions are in vp and stay inside their visual container when text is
scaled. Custom `SymbolGlyph` builders should use `configuration.size + 'vp'`;
text labels continue to follow the application's font configuration.
`VesperStageIconButton`, `VesperStagePillButton` and
`VesperStagePrimaryPlayButton` are public reusable controls.
Pill shortcuts use the standard skin radius and ten-percent `buttonBackground`,
independently of the gesture HUD surface. They share reduced-motion and 0.96
press feedback with the icon buttons. Time uses two-digit minutes; speed uses a
single decimal with `x`. System-selected quality uses a neutral label, and
automatic quality includes the observed output height. A confirmed fixed
bitrate shows the localized Locked label with its effective value in kbps;
it does not turn the observed output height into a locked resolution. Fixed
track selection shows Locked with the height once its output dimensions match.
Failed requests retain the previous effective selection and the settings error.
The native More resource is the unframed `sys.symbol.more`.

Eight button variants have independent `VesperStageButtonStyle` values, separating
visual size from a minimum 44 vp target. Metrics also cover insets, timeline and
HUD geometry. `contentOverlay` and `expandedLeadingActions` accept host builders;
declare `expandedLeadingWidth` so overflow calculation reserves their space.
The leading slot appears at its full declared width or is omitted when the
core controls leave insufficient room; it never exposes a clipped touch target.
Decorative content does not capture video gestures. Supplied skin objects belong
to the host; separate default skins never share mutable palettes.

`settingsStyle: 'native'` uses system colors for the whole settings surface.
Choose `'branded'` to apply the complete settings palette. Phone sheets size to
their content up to 85% of available height; wide Stages use a 380 vp center panel.
Pass the actual window `availableHeight`. Headers/close remain outside the list
scroll; selected rows scroll into view. Back navigates caption style → subtitles
→ menu → playback. Closing returns focus to the originating control.

Omit `presentation` for local state, or supply `VesperStagePresentation` and
`onPresentationChange` as a controlled pair. Presentation changes issue no
activation. `reducedMotion` disables control fades/press scaling; normal control
fades take 180 ms and custom button press scale is 0.96.

The footer provides current-speed and video-quality shortcuts. Subtitles opens
its own pane; the settings menu also links to audio, quality and speed panes.
Each native pane has a return action and the system sheet close control. Track,
subtitle Off, automatic bitrate and explicit bitrate choices show their selected
state. Requested selection/outcome and current output remain separate facts.
Adaptive playback checks Auto until a fixed track or bitrate takes effect. A
pending or failed request retains the preceding effective choice. Video tracks
sort by descending resolution and bitrate; repeated resolutions include the
native reported bitrate in their details. Raw bitrate rows appear when native
track selection is unavailable or a bitrate choice is effective.
Supported ISO 639 language aliases retain the same localized display name when
Media Kit updates a tag such as `en` to `eng`; catalog values remain unchanged.
Paused VOD audio choices display an apply-on-resume notice and retain the current
checkmark until native selection is confirmed after resuming playback.
The core exposes an API 26 native language-filter request with native readback;
this Stage does not infer automatic reselection from it. The core's one-time
language preferences and explicit native/adapter selections remain separate host
operations with source-specific results.
The menu also exposes mute, volume, optional window brightness and PiP.
Unsupported operations report through `binding.error`. Controls use accessible
icon buttons and adjustable sliders. Updating caption text is decorative and is
not repeatedly announced at every time update. Native screen-reader behavior,
large text and device window behavior
remain acceptance gates.
Rejected control commands remain visible alongside playback controls. The playback
error overlay supplies a reason and retry or change-source action, including in
compact Stages. Automatic quality is disabled when the
player exposes no native bitrate inventory.
Unsupported catalog actions are disabled with their selection reason. Buffering
has a separate loading indicator; a playing state can still be buffering.

Horizontal dragging previews a bounded VOD seek and commits on release. A
cancelled drag discards its pending seek. Vertical dragging applies volume on
the right or the optional host brightness adapter on the left during movement.
Each continuous control owns at most one physical command and one latest value;
intermediate samples do not accumulate in a command queue. An icon, percentage
and progress HUD accompanies adjustment and dismisses 520 ms after release.
Locked controls and an open settings panel suppress playback gestures. Live
horizontal seek is disabled because this route has no verified DVR window.

`environment.brightness` implements `current(): number` and
`set(value: number): Promise<void>` with values in `[0, 1]`. The standalone
[example](../../../examples/harmony-arkui-host/README.md) maps this adapter to
native window brightness. The package itself requires no brightness permission.

The [platform guide](../README.md) defines the tested media and system boundaries.

`VesperSubtitleStyle` validates basic font scale, foreground/background, edge
color/radius, bottom inset and line limit. Supply `subtitleStyle` with
`onSubtitleStyleChange` for host ownership; otherwise settings use Stage-local
style. Caption geometry uses the fitted video, safe area and measured header/
transport height. Long cues ellipsize within the measured line budget; a window
with no caption space suppresses the cue while controls occupy it. Captions return
when space is available and hide behind settings or suppressed presentation.

The host supplies `safeArea` in viewport units for immersive layouts. Stage
applies those insets to controls/settings and captions while the native Surface
fills its layout. The example maps window system/cutout/navigation avoid areas
and updates them when the window changes.
