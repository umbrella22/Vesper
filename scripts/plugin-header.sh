#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
abi="$root/crates/plugin/player-plugin-abi"
header="$root/include/vesper_plugin.h"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Give cbindgen the standalone ABI source in an isolated directory. Its single
# file mode resolves sibling modules relative to that file's directory.
cp "$abi/src/lib.rs" "$work/lib.rs"
cbindgen --config "$abi/cbindgen.toml" --output "$work/raw.h" "$work/lib.rs"
# cbindgen does not emit exception specifications on C ABI callback pointers.
# Its aggregate constants also use C compound literals and C++20 designators.
python3 - "$work" <<'PY'
import pathlib
import re
import sys

work = pathlib.Path(sys.argv[1])
text = (work / 'raw.h').read_text()
text = re.sub(r'(\(\*[A-Za-z_]\w*\)\([^;]*?\))\s*;', r'\1 VESPER_NOEXCEPT;', text)
text = text.replace('extern const struct VesperPluginRoot *vesper_native_plugin_entry(void);',
                    'VESPER_PLUGIN_API const struct VesperPluginRoot *vesper_native_plugin_entry(void) VESPER_NOEXCEPT;')
text = re.sub(r'^#define (VESPER_\w+_ID) \(VesperInterfaceId\)\{ \.bytes = \{ ([^}]+)\} \}',
              r'static const VesperInterfaceId \1 = {{ \2}};', text, flags=re.MULTILINE)
text = re.sub(r'(^#define VESPER_\w+ \d+)\n\n(?=#define VESPER_\w+ \d+)', r'\1\n', text, flags=re.MULTILINE)
(work / 'vesper_plugin.h').write_text(text)
PY

cat > "$work/header-check.c" <<'C'
#define VESPER_PLUGIN_BUILD
#include "vesper_plugin.h"
#ifdef __cplusplus
#include <type_traits>
static_assert(noexcept(vesper_native_plugin_entry()));
static_assert(noexcept(((VesperVideoFrame *)nullptr)->release_fn(nullptr, 0, 0)));
static_assert(noexcept(((VesperAudioProcessorVtable *)nullptr)->process_block(nullptr, nullptr, nullptr)));
static_assert(std::is_standard_layout<VesperVideoFrame>::value);
#endif
const VesperPluginRoot *vesper_native_plugin_entry(void) VESPER_NOEXCEPT { return 0; }
int check_ids(void) {
    return VESPER_VIDEO_DECODER_ID.bytes[0] + VESPER_VIDEO_FILTER_ID.bytes[0] +
        VESPER_AUDIO_PROCESSOR_ID.bytes[0] + VESPER_SOURCE_NORMALIZER_ID.bytes[0] +
        VESPER_SOURCE_NORMALIZER_RESOURCE_ID.bytes[0] + VESPER_POST_DOWNLOAD_PROCESSOR_ID.bytes[0];
}
C
"${CC:-clang}" -x c -std=c99 -pedantic-errors -Wall -Wextra -Werror -fsyntax-only "$work/header-check.c"
"${CXX:-clang++}" -x c++ -std=c++17 -pedantic-errors -Wall -Wextra -Werror -fsyntax-only "$work/header-check.c"

case "${1:-verify}" in
    generate) cp "$work/vesper_plugin.h" "$header" ;;
    verify)
        if ! cmp -s "$header" "$work/vesper_plugin.h"; then
            echo 'Plugin ABI header has drifted. Run vesper plugin header-generate.' >&2
            diff -u "$header" "$work/vesper_plugin.h" || true
            exit 5
        fi
        ;;
    *) exit 2 ;;
esac
printf 'Verified vesper_plugin.h with C99 and C++17.\n'
