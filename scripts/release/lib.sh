# lib.sh — helpers shared by bump.sh and release.sh. Source it; don't run it.
#
# Mirrors truce's scripts/release/ conventions (same version source,
# same cargo / cargo.exe detection) so a maintainer moving between the
# two repos finds the same shapes.
#
# Leaves the caller at the repo root, with $CARGO set.

set -euo pipefail

# The root is two levels above this file, wherever the caller runs from.
# Resolved from the script's own location rather than `git rev-parse`:
# outside a git checkout that expands to `cd ""`, a silent no-op that
# leaves every relative path below pointing at the wrong place.
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# On Windows (WSL) both cargo.exe and cargo can be on PATH; cargo.exe is
# the one that sees the Windows toolchain.
if command -v cargo.exe >/dev/null 2>&1; then
    CARGO=cargo.exe
elif command -v cargo >/dev/null 2>&1; then
    CARGO=cargo
else
    echo "Error: cargo not found on PATH (looked for 'cargo' and 'cargo.exe')" >&2
    exit 1
fi
export CARGO

# The sibling checkout the workspace's truce path dependencies point at.
TRUCE_DIR="../truce"

# Print `[workspace.package].version` from the given manifest.
ws_version() {
    local manifest="$1" v
    v="$(awk -F\" '
        /^\[workspace\.package\]/ { p = 1 }
        p && /^version = / { print $2; exit }
    ' "$manifest")"
    if [[ -z "$v" ]]; then
        echo "Error: could not read [workspace.package].version from $manifest" >&2
        exit 1
    fi
    printf '%s\n' "$v"
}

# `X.Y.Z[-pre]` -> `X.Y`.
major_minor() {
    local base="${1%%-*}"
    printf '%s\n' "${base%.*}"
}

# The requirement every truce dependency carries for a given extensions
# version. Tilde, not caret: the compatibility rule is "match
# major.minor", and `~X.Y` is exactly `>=X.Y.0, <X.(Y+1).0`, so cargo
# enforces the rule instead of a README table asserting it.
truce_req() {
    printf '~%s\n' "$(major_minor "$1")"
}

# Portable in-place sed (BSD on macOS takes `-i ''`, GNU takes `-i`).
sed_inplace() {
    if [[ "$(uname)" == "Darwin" ]]; then
        sed -i '' "$@"
    else
        sed -i "$@"
    fi
}

# Print `name<TAB>version` for each [workspace.dependencies] entry whose
# path matches the given prefix: `crates/` for this repo's own crates,
# `../truce/` for truce's.
ws_deps_with_path() {
    local prefix="$1"
    awk -v prefix="$prefix" '
        /^\[workspace\.dependencies\]/ { in_deps = 1; next }
        in_deps && /^\[/ { in_deps = 0 }
        in_deps && index($0, "path = \"" prefix) && match($0, /version *= *"[^"]*"/) {
            v = substr($0, RSTART, RLENGTH)
            sub(/^version *= *"/, "", v)
            sub(/"$/, "", v)
            name = $0
            sub(/ *=.*/, "", name)
            printf "%s\t%s\n", name, v
        }
    ' Cargo.toml
}
