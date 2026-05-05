# Third-Party Notices

This file tracks third-party notice information for Vesper source releases and
for future binary distributions that bundle third-party components.

## Project License

The Vesper source repository is licensed under Apache-2.0.

- Copyright 2026 umbrella22
- Repository license: [LICENSE](LICENSE)
- Repository notice file: [NOTICE](NOTICE)

## Current Repository Status

At the source-repository level, Vesper does not currently ship generated
FFmpeg binaries or any other vendored third-party binary bundle inside the
repository root.

Release gate:

- if any Android, iOS, desktop, Flutter, or other shipped artifact bundles
  FFmpeg or any other third-party binary, update this file before cutting that
  release
- helper commands such as `scripts/vesper android ffmpeg`,
  `scripts/vesper apple ffmpeg`, or `scripts/vesper desktop ensure-ffmpeg`
  create local build inputs only; running them does not satisfy redistribution
  notice, source, or relinking obligations by itself
- artifacts that bundle `player-remux-ffmpeg` must be reviewed as FFmpeg
  redistribution artifacts because the plugin depends on FFmpeg libraries

## Planned Third-Party Runtime Tracking

When a release artifact bundles a third-party runtime, add an entry here with:

1. component name
2. exact version
3. upstream project URL
4. exact license identifier and license text location
5. whether the component is dynamically or statically linked
6. any required attribution, source-offer, or relinking obligations
7. the exact build configuration or feature flags used

## FFmpeg / LGPL Compliance Policy

This section is release guidance, not legal advice. It is based on the FFmpeg
project's public license guidance and the same boundary model used by media
SDKs such as libVLC: Vesper's SDK license is separate from the license of the
media runtime libraries redistributed with a host application.

Important boundary:

- FFmpeg is not covered by Vesper's Apache-2.0 license and is not relicensed by
  this repository
- any bundled FFmpeg binaries must keep their own license text, notices,
  copyright attribution, source availability, and relinking rights
- the exact obligations depend on the shipped FFmpeg configure flags, enabled
  external libraries, and whether the artifact uses dynamic or static linkage

Default Vesper scripts are intended to stay on the LGPL-oriented side:

- `scripts/vesper android ffmpeg` builds shared FFmpeg libraries for Android
  and does not pass `--enable-gpl` or `--enable-nonfree`; the default OpenSSL
  backend adds `--enable-version3`, so release artifacts produced from that
  default Android build must be treated as LGPLv3-or-later unless a release
  owner verifies a different configuration
- `scripts/vesper apple ffmpeg` builds Apple FFmpeg static archives and
  dynamic libraries without `--enable-gpl`, `--enable-nonfree`, or
  `--enable-version3` by default, so it should be treated as an
  LGPLv2.1-or-later FFmpeg build unless a release owner changes those flags or
  dependencies
- `scripts/vesper desktop ensure-ffmpeg` creates a repository-local static
  desktop fallback for development; desktop releases should prefer system or
  dynamic FFmpeg when possible, and any statically linked redistributed binary
  must include an LGPL-compliant way to relink against a modified FFmpeg build
- `scripts/vesper android remux-plugin` and
  `scripts/vesper ios remux-plugin` produce optional plugin artifacts; bundling
  those plugins in an app is an explicit decision by the host and triggers the
  same FFmpeg redistribution review

Before shipping any artifact that includes FFmpeg libraries:

1. Confirm that the final FFmpeg configure flags do not include
   `--enable-gpl` or `--enable-nonfree` unless the release intentionally moves
   to GPL terms or accepts that a nonfree FFmpeg binary may be
   non-redistributable.
2. Record the exact FFmpeg version, upstream source archive URL, source archive
   checksum, local patches, and full configure line.
3. Distribute the corresponding FFmpeg source for the exact binary being
   shipped, including any local changes and build instructions.
4. Include the applicable LGPL license text and FFmpeg copyright notices in
   the app, package, website download page, or release notes where users obtain
   the binary.
5. Preserve user relinking rights. Dynamic libraries are preferred. If FFmpeg
   is statically linked into a redistributed artifact, publish relinkable
   object files or another documented mechanism that allows relinking against a
   modified LGPL FFmpeg build.
6. Do not remove or obscure FFmpeg library names, notices, or attribution.
7. Review external libraries compiled into FFmpeg. Android defaults currently
   involve OpenSSL and, when DASH is enabled, libxml2; their notices and source
   obligations must be tracked alongside FFmpeg.
8. Keep the host app's EULA, about screen, and download page consistent with
   FFmpeg's separate license and avoid terms that prohibit reverse engineering
   where LGPL relinking/debugging rights apply.

## FFmpeg Release Entry Template

Use a block like this for every Vesper release artifact that ships FFmpeg:

```text
Component: FFmpeg
Version: <fill in>
Upstream source: <URL>
Source checksum: <sha256>
Local changes: <none|patch file / diff URL>
License mode: <LGPLv2.1-or-later|LGPLv3-or-later|GPL|nonfree>
Linkage: <dynamic|static|mixed>
Build command: <scripts/vesper ...>
Configure flags: <full configure line>
Artifact scope: <Android remux plugin / iOS remux plugin / desktop app / other>
Bundled FFmpeg libraries: <libavcodec, libavformat, ...>
Bundled external libraries: <OpenSSL, libxml2, ...>
FFmpeg source location: <same release download URL / source bundle URL>
Relinking materials: <not required for dynamic-only|object files / relink docs>
User-facing notice location: <about screen / release notes / download page>
Notes: <additional attribution, patent, or platform-specific details>
```

## Maintenance Note

This file is intentionally conservative. Do not treat it as a substitute for
checking the exact license terms of the third-party binaries that are actually
distributed in a release.
