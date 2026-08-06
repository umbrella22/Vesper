# Vesper Plugin Schemas

The plugin package flow uses separate records for author input, generated
package metadata, publisher trust, and mobile embedding:

```text
vesper-plugin.toml
    -> project.schema.json
    -> vesper plugin package
    -> manifest.json + SHA256SUMS + signature.json
    -> vesper plugin verify/install + trust-store.schema.json
```

`project.schema.json` describes the JSON-equivalent structure of the
author-owned `vesper-plugin.toml`. The package command computes artifact
SHA-256 values and writes `manifest.json`; artifact hashes are not accepted
from the source manifest.

`manifest.schema.json` describes canonical `manifest.json` inside a
`.vesper-plugin` archive. `descriptor.schema.json` describes the
artifact-independent identity and capability record used by embedded mobile
registries. `embedded-registry.schema.json` covers Android and Apple
build-time registry assets.

`signature.schema.json` describes the canonical signature envelope. The
Ed25519 signature input is:

```text
"vesper-plugin-signature\0" || exact SHA256SUMS bytes
```

`SHA256SUMS` contains sorted entries for `manifest.json` and every payload
file. It excludes `SHA256SUMS` and `signature.json`. Verification binds the
signature publisher and key ID to `trust-store.schema.json`. A trust store
may contain multiple active keys for one publisher during rotation; revoked
keys cannot verify new installations.

Archive paths are bounded relative file paths. The CLI rejects traversal,
backslashes, platform prefixes, control characters, case collisions, duplicate
paths, and file/directory ancestor conflicts. Artifacts use mode `0755`; all
other package entries use mode `0644`. ZIP metadata cannot override these
installation modes, and non-regular file types are rejected.

JSON Schema cannot express cryptographic relationships, semantic-version
range ordering, normalized case-folded path uniqueness, file/directory
ancestor conflicts, or UTF-8 byte lengths. Schema `maxLength` values provide
character-count prechecks; `vesper plugin package` and `vesper plugin verify`
enforce the byte limits and cross-record rules.
