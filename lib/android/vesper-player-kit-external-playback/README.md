# Android External Playback Relay

## Host-prepared DASH input

The FFmpeg-backed DLNA fallback accepts bounded, unencrypted, single-period
fMP4 DASH tracks described by a complete `SegmentTemplate` on the selected
`Representation` or its `AdaptationSet`. The template may use fixed segment
duration or `SegmentTimeline`. Android parses the selected tracks, delegates
segment planning and URI template expansion to the shared Rust bridge, fetches
initialization and media segments through the existing source, origin, address
and request-header policy, and feeds the existing FIFOs into MPEG-TS remux.

Timeline entries preserve `S@t`, `S@d` and `S@r`. An omitted `t` continues from
the previous entry; an omitted `r` means zero repetitions. `r=-1` requires a
following explicit `S@t` or a finite `mediaPresentationDuration`. URI expansion
supports `$Number$`, `$Time$`, `$RepresentationID$`, `$Bandwidth$`, numeric
padding and `$$`. `$Time$` uses the original timeline ticks, including when
`presentationTimeOffset` is present. The offset participates in shared segment
planning; it is not subtracted from resource names or used to rewrite media
timestamps.

Invalid numeric attributes fail with `unsupported_dash_layout` rather than
falling back to default values. Each selected track requires an initialization
segment and is limited to 8,192 expanded media segments. Dynamic input is a
bounded snapshot; the relay does not poll manifest updates. Unbounded negative
repeats, encrypted content, multiple periods and dynamic `SegmentBase` remain
unsupported. Existing remote-reference and private-address opt-in requirements
continue to apply to every expanded resource.

## Diagnostic headers

When relay debug diagnostics are enabled, the native FFmpeg relay adds these
HTTP response fields:

| Field | Representation |
| --- | --- |
| `X-Vesper-FFmpeg-Configure-Metadata` | Percent-encoded bytes of the original UTF-8 configure metadata. |
| `X-Vesper-FFmpeg-Configure-Metadata-Encoding` | The literal `percent-encoded-utf8`. |

The metadata field uses uppercase `%HH` escapes. ASCII letters, digits, `-`,
`.`, `_`, and `~` remain literal; every other UTF-8 byte is escaped. Decode
percent escapes to bytes once, then decode those bytes as UTF-8. This is not
form encoding: `+` does not represent a space. For example, `a=100%\r\nb=é\n`
is transmitted as `a%3D100%25%0D%0Ab%3D%C3%A9%0A`.

Both fields are omitted when debug diagnostics are disabled. The encoding
applies only to this HTTP field; JNI runtime metadata JSON and relay diagnostic
details retain the original configure metadata, including its line breaks.

Host-prepared DASH track writers wait for the native FIFO reader before fetching
or writing media. Reader rendezvous uses a cancellable 10-second budget, matching
the relay HTTP connection timeout. Each writer owns a write-only descriptor and
uses blocking writes to retain pipe backpressure. Completing a track closes its
writer so the native reader can observe EOF; cancelling the session also closes
active writers and interrupts pending reader rendezvous.

DLNA protocol-info discovery is optional: a non-cancellation query failure uses
the existing empty-capability preparation policy. Cancellation propagates to
the caller and is checked before preparation begins. Resource handoff remains
protected so a cancellation after preparation closes the prepared relay and
does not start the device load.
