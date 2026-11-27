#!/usr/bin/env bash
#
# lv2-check.sh - validate LV2 bundles the way a host sees them.
#
# Usage:
#   scripts/ci/lv2-check.sh [--passthrough] <bundle.lv2>...
#
# For each bundle:
#   1. lv2_validate: every .ttl checked against the installed LV2 spec
#      vocabularies (domains, ranges, required properties).
#   2. lilv discovery: lv2ls finds exactly one plugin in the bundle and
#      lv2info describes it, which is what a host does at scan time.
#   3. lv2_smoke: instantiate the plugin, connect every port and run two
#      blocks (see scripts/ci/lv2_smoke.c). --passthrough also requires
#      each audio output to equal the matching input.
#
# Needs lilv (with its headers) and the LV2 spec installed: the lilv-utils,
# liblilv-dev, lv2-dev and sordi packages on Debian / Ubuntu, `brew install
# lilv` on macOS.

set -euo pipefail

# lib.sh moves to the repo root, so pin the bundle paths first.
args=()
for arg in "$@"; do
    if [[ -d "$arg" ]]; then
        args+=("$(cd "$arg" && pwd)")
    else
        args+=("$arg")
    fi
done
set -- ${args[@]+"${args[@]}"}

source "$(dirname "$0")/../release/lib.sh"

die() {
    echo "lv2-check: $*" >&2
    exit 1
}

passthrough=()
if [[ "${1:-}" == "--passthrough" ]]; then
    passthrough=(--passthrough)
    shift
fi
if [[ $# -eq 0 ]]; then
    echo "usage: scripts/ci/lv2-check.sh [--passthrough] <bundle.lv2>..." >&2
    exit 2
fi

for tool in lv2_validate lv2ls lv2info cc; do
    command -v "$tool" >/dev/null || die "$tool not found; see this script's header for the packages"
done

# Build the smoke host once per run.
smoke=target/lv2-smoke/lv2_smoke
mkdir -p "$(dirname "$smoke")"
if command -v pkg-config >/dev/null && pkg-config --exists lilv-0; then
    read -r -a lilv_flags <<< "$(pkg-config --cflags --libs lilv-0)"
elif command -v brew >/dev/null; then
    prefix="$(brew --prefix)"
    lilv_flags=(-I"$prefix/include/lilv-0" -I"$prefix/include" -L"$prefix/lib" -llilv-0)
else
    die "can't find lilv's compiler flags (no pkg-config entry for lilv-0)"
fi
cc -std=c99 -D_DEFAULT_SOURCE -Wall -Wextra -Werror -O1 \
    -o "$smoke" scripts/ci/lv2_smoke.c "${lilv_flags[@]}" -lm

for bundle in "$@"; do
    [[ -d "$bundle" ]] || die "no bundle at $bundle"
    echo "→ $bundle"

    echo "  lv2_validate"
    lv2_validate "$bundle"/*.ttl

    echo "  lilv discovery"
    # A directory of its own, so plugins installed on the machine can't
    # stand in for the one under test.
    scan="$(mktemp -d)"
    ln -s "$(cd "$bundle" && pwd)" "$scan/"
    uris="$(LV2_PATH="$scan" lv2ls)"
    [[ "$(grep -c . <<< "$uris")" -eq 1 ]] || die "lv2ls found ${uris:-no plugins} in $bundle, expected one"
    LV2_PATH="$scan" lv2info "$uris" >/dev/null
    rm -rf "$scan"

    echo "  lv2_smoke ${passthrough[*]:-}"
    "$smoke" "$bundle" ${passthrough[@]+"${passthrough[@]}"}
done
