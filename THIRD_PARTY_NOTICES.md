# Third-Party Notices

This file tracks third-party notice information for Vesper source releases and
for future binary distributions that bundle third-party components.

## Project License

The Vesper source repository is licensed under Apache-2.0.

- Copyright 2026 umbrella22
- Repository license: [LICENSE](LICENSE)
- Repository notice file: [NOTICE](NOTICE)

## Current Repository Status

At the source-repository level, Vesper does not currently ship a vendored
third-party binary bundle inside the repository root.

That means this file is primarily a forward-looking release checklist for
future downloadable artifacts, especially where FFmpeg or other media runtime
libraries are redistributed together with Vesper.

## Planned Third-Party Runtime Tracking

When a release artifact bundles a third-party runtime, add an entry here with:

1. component name
2. exact version
3. upstream project URL
4. exact license identifier and license text location
5. whether the component is dynamically or statically linked
6. any required attribution, source-offer, or relinking obligations
7. the exact build configuration or feature flags used

## FFmpeg Guidance

Vesper's desktop/media backend work depends on FFmpeg-related libraries, but
the exact redistribution obligations depend on the specific FFmpeg build that
is shipped.

Important boundary:

- FFmpeg is not covered by Vesper's Apache-2.0 license
- any bundled FFmpeg binaries must keep their own license and notice materials
- the exact obligations depend on the real build configuration and enabled
  libraries

In practice, before shipping a Vesper release that includes FFmpeg binaries,
record at least:

- FFmpeg version
- upstream source URL
- configure flags used to produce the shipped binaries
- whether the shipped build is LGPL-oriented or GPL-oriented
- whether the binaries are dynamically or statically linked
- any additional third-party codec/library notices pulled in by that build

## Future FFmpeg Entry Template

Use a block like this when Vesper starts shipping FFmpeg in release artifacts:

```text
Component: FFmpeg
Version: <fill in>
Upstream: <fill in>
License: <fill in exact shipped license form>
Linkage: <dynamic|static>
Build configuration: <fill in configure flags / enabled libraries>
Artifact scope: <desktop release / Android / other>
Notes: <fill in required attribution or source-distribution details>
```

## Maintenance Note

This file is intentionally conservative. Do not treat it as a substitute for
checking the exact license terms of the third-party binaries that are actually
distributed in a release.
