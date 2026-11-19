#!/usr/bin/env python3
"""Stamp a release version into the workspace and the Tauri config.

Usage: stamp-version.py X.Y.Z   (run from the repository root)

Portable on purpose: BSD sed (macOS runners) silently ignores GNU-only `0,/re/`
addresses, which left macOS binaries reporting the wrong version.
"""
import json
import re
import sys

version = sys.argv[1] if len(sys.argv) > 1 else ""
if not re.fullmatch(r"\d+\.\d+\.\d+", version):
    sys.exit(f"usage: stamp-version.py X.Y.Z (got {version!r})")

# Workspace version: the first `version = "..."` line in the root Cargo.toml.
text = open("Cargo.toml").read()
new, n = re.subn(r'(?m)^version = ".*"$', f'version = "{version}"', text, count=1)
if n != 1:
    sys.exit("could not find the workspace version in Cargo.toml")
open("Cargo.toml", "w").write(new)

# Tauri config.
path = "src-tauri/tauri.conf.json"
conf = json.load(open(path))
conf["version"] = version
with open(path, "w") as f:
    json.dump(conf, f, indent=2)
    f.write("\n")

print(f"stamped {version}")
