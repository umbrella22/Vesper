#!/usr/bin/env python3
"""Check the native runtime inventory of one mobile distribution set."""

import argparse
from collections import defaultdict
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import zipfile


FFMPEG_ELF = re.compile(r"lib(avcodec|avformat|avutil|avfilter|avdevice|swresample|swscale)\.so(?:\.\d+)*")
FFMPEG_APPLE = re.compile(r"(?:VesperFFmpeg(AVCodec|AVFormat|AVUtil|AVFilter|AVDevice|SwResample|SwScale)\.framework|lib(avcodec|avformat|avutil|avfilter|avdevice|swresample|swscale)(?:\.\d+)*\.dylib)", re.IGNORECASE)
FFMPEG_SHARED = {"avcodec", "avformat", "avutil"}
SHADER_COMPILERS = re.compile(r"(?:lib)?(?:slangc|slang|slang-compiler|slang-glslang)(?:\.[\w.-]+)?", re.IGNORECASE)


RETIRED_PLUGINS = re.compile(
    r"frame[-_]?processor[-_]?diagnostic|performance[-_]?diagnostics", re.IGNORECASE
)


def members(artifact):
    if artifact.is_dir():
        for path in artifact.rglob("*"):
            if path.is_file() or path.is_symlink():
                yield path.relative_to(artifact).as_posix()
    elif artifact.suffix in {".aar", ".apk"}:
        with zipfile.ZipFile(artifact) as archive:
            for info in archive.infolist():
                if not info.is_dir():
                    yield info.filename
    elif artifact.suffix == ".har":
        with tarfile.open(artifact) as archive:
            for info in archive:
                if not info.isdir():
                    yield info.name
    else:
        raise ValueError(f"unsupported artifact type {artifact}")


def inventory(artifacts):
    providers = defaultdict(list)
    errors = []
    for artifact in artifacts:
        if not artifact.exists():
            raise ValueError(f"missing artifact {artifact}")
        apple = artifact.suffix in {".xcframework", ".app", ".framework"}
        if RETIRED_PLUGINS.search(artifact.name):
            errors.append(f"{artifact} is a retired plugin artifact")
        frameworks = set()
        for member in members(artifact):
            parts = PurePosixPath(member).parts
            if any(SHADER_COMPILERS.fullmatch(part) for part in parts):
                errors.append(f"{artifact}!{member} ships a shader compiler")
            if any(RETIRED_PLUGINS.search(part) for part in parts):
                errors.append(f"{artifact}!{member} ships a retired plugin")
            if not parts:
                continue
            if apple:
                parts = (artifact.name, *parts)
                for index, part in enumerate(parts):
                    match = FFMPEG_APPLE.fullmatch(part)
                    if match is None:
                        continue
                    library = (match[1] or match[2]).lower()
                    # An XCFramework deliberately contains a provider per platform slice.
                    slice_name = parts[1] if artifact.suffix == ".xcframework" else "app"
                    origin = "/".join(parts[: index + 1])
                    frameworks.add((slice_name, library, origin))
            else:
                match = FFMPEG_ELF.fullmatch(parts[-1])
                if match is not None:
                    abi = parts[-2] if len(parts) > 1 else "root"
                    providers[(abi, match[1])].append(f"{artifact}!{member}")
        for slice_name, library, origin in sorted(frameworks):
            providers[(slice_name, library)].append(f"{artifact}!{origin}")
    return providers, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ffmpeg", choices=("absent", "shared"), required=True)
    parser.add_argument("artifacts", nargs="+", type=Path)
    args = parser.parse_args()
    providers, errors = inventory(args.artifacts)
    by_slice = defaultdict(set)
    for (slice_name, library), origins in sorted(providers.items()):
        by_slice[slice_name].add(library)
        if len(origins) != 1:
            errors.append(f"{slice_name}/{library} has multiple providers: {', '.join(origins)}")
        print(f"{slice_name}/{library}: {', '.join(origins)}")
    if args.ffmpeg == "absent" and providers:
        errors.append("expected a distribution without FFmpeg runtimes")
    if args.ffmpeg == "shared" and (
        not by_slice or any(libraries != FFMPEG_SHARED for libraries in by_slice.values())
    ):
        errors.append("expected one shared avcodec, avformat and avutil runtime per slice")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Checked {len(args.artifacts)} artifacts ({args.ffmpeg} FFmpeg)")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as error:
        sys.exit(str(error))
