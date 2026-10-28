#!/usr/bin/env python3
"""Set the app version in the Cargo manifests, meson.build, and Cargo.lock.

Run from anywhere inside the repository. Every file is checked before any
write, and the files are restored if the lockfile update fails.
"""
import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path

MANIFESTS = ("Cargo.toml", "crates/core/Cargo.toml", "crates/search-provider/Cargo.toml")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")
CARGO_VERSION = re.compile(r'^(version\s*=\s*)"[^"\n]*"', re.MULTILINE)
MESON_PROJECT = re.compile(r"^project\((.*?)^\)", re.MULTILINE | re.DOTALL)
MESON_VERSION = re.compile(r"^(\s*version:\s*)'[^'\n]*'", re.MULTILINE)


def find_root():
    result = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    if result.returncode != 0:
        raise ValueError("run this script from inside the sessions-chronicle repository")
    root = Path(result.stdout.strip())
    if "workspace" not in tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8")):
        raise ValueError(f"{root} is not the Cargo workspace root")
    return root


def package_span(content, name):
    """Return the (start, end) offsets of the [package] table body."""
    headers = list(re.finditer(r"^\s*\[.*$", content, re.MULTILINE))
    packages = [i for i, header in enumerate(headers) if header.group().strip() == "[package]"]
    if len(packages) != 1:
        raise ValueError(f"{name}: expected exactly one [package] table")
    index = packages[0]
    end = headers[index + 1].start() if index + 1 < len(headers) else len(content)
    return headers[index].end(), end


def replace_once(content, span, pattern, replacement, name):
    start, end = span
    body, count = pattern.subn(replacement, content[start:end])
    if count != 1:
        raise ValueError(f"{name}: expected one version field, found {count}")
    return content[:start] + body + content[end:]


def bump_cargo(content, name, version):
    updated = replace_once(content, package_span(content, name), CARGO_VERSION, rf'\g<1>"{version}"', name)
    if tomllib.loads(updated)["package"]["version"] != version:
        raise ValueError(f"{name}: version update did not round-trip")
    return updated


def bump_meson(content, version):
    project = MESON_PROJECT.search(content)
    if project is None:
        raise ValueError("meson.build: project(...) call not found")
    return replace_once(content, project.span(1), MESON_VERSION, rf"\g<1>'{version}'", "meson.build")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="release version (e.g. 0.12.0)")
    args = parser.parse_args()
    if not VERSION.fullmatch(args.version):
        parser.error("version must have the format MAJOR.MINOR.PATCH without leading zeroes")

    try:
        root = find_root()
        updates = []
        for name in (*MANIFESTS, "meson.build"):
            path = root / name
            original = path.read_text(encoding="utf-8")
            if name == "meson.build":
                updated = bump_meson(original, args.version)
            else:
                updated = bump_cargo(original, name, args.version)
            updates.append((path, name, original, updated))
        lockfile = root / "Cargo.lock"
        lockfile_original = lockfile.read_text(encoding="utf-8")
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1

    restore = []
    try:
        for path, name, original, updated in updates:
            if updated == original:
                print(f"Already up to date: {name}")
            else:
                # Register before writing: a failed write may already have truncated the file.
                restore.append((path, original))
                path.write_text(updated, encoding="utf-8")
                print(f"Updated {name}")

        restore.append((lockfile, lockfile_original))
        subprocess.run(["cargo", "update", "--workspace", "--offline"], cwd=root, check=True)
    except (OSError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        print(f"Error: version update failed: {error}", file=sys.stderr)
        restoration_failed = False
        for path, original in reversed(restore):
            try:
                path.write_text(original, encoding="utf-8")
            except OSError as restore_error:
                restoration_failed = True
                print(f"Error: could not restore {path}: {restore_error}", file=sys.stderr)
        if not restoration_failed:
            print("Restored the original files", file=sys.stderr)
        # 130 is the shell convention for a run stopped by SIGINT.
        return 130 if isinstance(error, KeyboardInterrupt) else 1
    print("Updated Cargo.lock workspace entries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
