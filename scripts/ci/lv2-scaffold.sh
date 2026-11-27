#!/usr/bin/env bash
#
# lv2-scaffold.sh - scaffold fresh truce plugins, add LV2 the way a user
# does, and build their LV2 bundles.
#
# Usage:
#   scripts/ci/lv2-scaffold.sh <out-dir>
#
# For an effect and an instrument: `cargo truce new` in <out-dir>, then the
# edit the scaffold's own Cargo.toml comment tells users to make - an `lv2`
# feature on by default and `truce-lv2 = { version = "~X.Y", optional =
# true }` - and `cargo truce build --lv2`. The truce crates and truce-lv2
# resolve from the sibling ../truce checkout and this repo through
# `[patch.crates-io]`, so the dependency lines stay exactly what a user
# writes while CI builds unreleased code. Prints the built bundles' paths,
# one per line, for scripts/ci/lv2-check.sh.
#
# Needs cargo-truce and truce-format-lv2 on PATH. <out-dir> must be outside
# this repo's workspace; each project is made its own workspace root
# regardless.

set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: scripts/ci/lv2-scaffold.sh <out-dir>" >&2; exit 2; }
mkdir -p "$1"
out="$(cd "$1" && pwd)"

source "$(dirname "$0")/../release/lib.sh"

ext="$(pwd)"
truce="$(cd "$TRUCE_DIR" && pwd)"
req="~$(major_minor "$(ws_version Cargo.toml)")"

# Every publishable crate of the truce workspace, as `name = { path = ... }`
# patch lines. A patch the scaffold doesn't use is only a cargo note.
patches="$(cargo metadata --format-version 1 --no-deps --manifest-path "$truce/Cargo.toml" |
    python3 -I -c '
import json, os, sys
for p in json.load(sys.stdin)["packages"]:
    if p["publish"] is None:
        name, path = p["name"], os.path.dirname(p["manifest_path"])
        print(name + " = { path = " + json.dumps(path) + " }")
')"

# One target dir for both, so the second build reuses the first's deps.
export CARGO_TARGET_DIR="$out/target"
rm -rf "$CARGO_TARGET_DIR/bundles"

for kind in effect instrument; do
    name="truce-ci-lv2-$kind"
    flags=(--no-standalone --vendor "Truce CI" --vendor-id com.truce.ci)
    [[ "$kind" == instrument ]] && flags+=(--instrument)
    rm -rf "${out:?}/$name"
    (cd "$out" && cargo truce new "$name" "${flags[@]}") >&2

    python3 -I - "$out/$name/Cargo.toml" "$req" "$patches" "$ext/crates/truce-lv2" <<'PY'
import re, sys
path, req, patches, lv2 = sys.argv[1:]
text = open(path).read()
default = re.search(r'^default = \[(.*)\]$', text, re.M)
assert default, "no [features] default line"
text = text.replace(default.group(0), f'default = [{default.group(1)}, "lv2"]\nlv2 = ["dep:truce-lv2"]', 1)
deps = "\n[dependencies]\n"
assert deps in text, "no [dependencies] table"
text = text.replace(deps, f'{deps}truce-lv2 = {{ version = "{req}", optional = true }}\n', 1)
text += f'\n[workspace]\n\n[patch.crates-io]\n{patches}\ntruce-lv2 = {{ path = "{lv2}" }}\n'
open(path, "w").write(text)
PY

    (cd "$out/$name" && cargo truce build --lv2 --debug) >&2
done

shopt -s nullglob
bundles=("$CARGO_TARGET_DIR"/bundles/*.lv2)
[[ ${#bundles[@]} -eq 2 ]] || { echo "lv2-scaffold: expected 2 bundles, got ${#bundles[@]}" >&2; exit 1; }
printf '%s\n' "${bundles[@]}"
