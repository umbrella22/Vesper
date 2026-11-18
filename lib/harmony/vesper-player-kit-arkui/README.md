# @vesper/player-kit-arkui

The HarmonyOS binding connects one `VesperPlayerController` to a stable native
XComponent Surface and observable snapshot. It depends on `@vesper/player-kit`
and contains no Stage controls. Both packages use version `0.9.1-harmony.0`.

```ts
import { VesperPlayerController } from '@vesper/player-kit';
import { VesperPlayerBinding, VesperPlayerSurface } from '@vesper/player-kit-arkui';

// Retain this binding in the component's observed state.
const binding = new VesperPlayerBinding(new VesperPlayerController(context), {
  onDisappear: 'keep', releaseController: true
});
// Place VesperPlayerSurface({ binding }) in the component's build tree.
```

`VesperPlayerSurface` attaches after XComponent load and detaches the matching
Surface on destruction. Layout and controls can change without replacing the
binding or controller. Attachment renews the native subscription through ArkUI's
observed instance so callbacks update the displayed snapshot. `onDisappear`
accepts `keep`, `pause` or `dispose`; page
disappearance and Surface destruction are separate events. A host that selects
`keep` must define the later session termination point.

`binding.close(true)` disposes the controller and joins cleanup.
`binding.close(false)` detaches the Surface and removes its observer. Failed
cleanup can be retried. `binding.report(promise)` captures an asynchronous UI
command failure in `binding.error` and redacted `binding.feedback`. The latter
exposes a message key, category, action and retryability without copying raw
messages, source locators or headers into ordinary UI. Raw errors are for host
diagnostics. New activation and newer successful commands
clear earlier UI errors; older command completions cannot overwrite that recovery.

`VesperPictureInPicture.create(context, binding)` associates PiP with the same
XComponent. `setAutoStart()`, `start()`, `stop()` and `close()` follow native
confirmation. Explicitly retain the PiP object and avoid destroying its binding
during an active transition. Controller disposal closes registered PiP/system
resources before releasing AVPlayer and its loader.

PiP creation uses actual media `contentWidth`/`contentHeight` when known and
updates them when the media size or playback epoch changes. Position-only
publications do not resize the window. Unknown media dimensions fall back to
the original Surface size captured during creation; when that size cannot be
read, creation uses the platform default. A later unknown-size reset can then
report `unavailable`. `snapshot()` exposes content dimensions, epoch, size origin,
size state and the raw native size error code. A size update failure is observable
and does not fail playback; native PiP window limits still apply. The same
XComponent and AVPlayer remain owned throughout these updates.

Each binding owns a unique `surfaceIdentity` used by XComponent and its controls;
two Stage instances can coexist without sharing a component ID.

`VesperWindowPolicy(new VesperNativeWindowAdapter(nativeWindow))` coordinates
keep-screen-on and temporary brightness leases for a window. Attach it with
`binding.attachWindowPolicy(policy)`. Visible playing video requests screen-on;
pause, hidden presentation, background, PiP and close release that request. One
window pump serializes writes and retains the host baseline across multiple
bindings, failed writes and physical completion after a deadline. Failures are
available in `binding.windowFeedback`. Hosts changing their own screen-on policy
during a lease use `setHostKeepScreenOn()`; a same-value external native write
cannot be distinguished from the SDK's write. Brightness restores the captured
baseline when the last visible lease releases it. The host closes its policy
owner after detaching/closing the binding. Window service checks do not establish
physical screen timeout or power acceptance.

The host declares background permissions/modes when needed. Simulator PiP service
checks do not establish physical device window, background or interruption
acceptance. The [platform guide](../README.md) records those boundaries. Optional
controls are in [@vesper/player-kit-arkui-ui](../vesper-player-kit-arkui-ui/README.md).
