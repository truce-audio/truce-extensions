#!/usr/bin/env bash
#
# bump.sh — bump the workspace version and commit on the current branch.
#
# Usage:
#   bump.sh patch                # X.Y.Z → X.Y.(Z+1)
#   bump.sh minor                # X.Y.Z → X.(Y+1).0
#   bump.sh major                # X.Y.Z → (X+1).0.0
#   bump.sh 7.0.0                # explicit version (any SemVer)
#
#   bump.sh --edit-only <bump>   # rewrite files only, no commit
#
# Rewrites, in Cargo.toml:
#   - [workspace.package].version, which every crate inherits;
#   - the `version` on this repo's own crates in [workspace.dependencies]
#     (load-bearing at publish: cargo strips `path` and embeds it);
#   - the `version` on every truce dependency, to `~X.Y` of the new
#     version - see `truce_req` in lib.sh.
# Then refreshes Cargo.lock and commits.
#
# truce-extensions tracks truce's major.minor and releases *after* truce,
# so the sibling ../truce checkout must already be at the new major.minor.
# Bump and release truce first.

source "$(dirname "$0")/lib.sh"

EDIT_ONLY=0
BUMP=""
for arg in "$@"; do
    case "$arg" in
        --edit-only) EDIT_ONLY=1 ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        -*)
            echo "Error: unknown flag $arg" >&2
            exit 1
            ;;
        *)
            if [[ -n "$BUMP" ]]; then
                echo "Error: unexpected extra argument $arg" >&2
                exit 1
            fi
            BUMP="$arg"
            ;;
    esac
done

if [[ -z "$BUMP" ]]; then
    echo "Usage: bump.sh [--edit-only] patch | minor | major | <X.Y.Z>" >&2
    exit 1
fi

# Read current version + compute new -----------------------------------------

CURRENT="$(ws_version Cargo.toml)"

case "$BUMP" in
    patch|minor|major)
        # Strip a pre-release suffix (e.g. -rc.1) before the SemVer math.
        IFS=. read -r MAJOR MINOR PATCH <<< "${CURRENT%%-*}"
        case "$BUMP" in
            patch) NEW="$MAJOR.$MINOR.$((PATCH + 1))" ;;
            minor) NEW="$MAJOR.$((MINOR + 1)).0" ;;
            major) NEW="$((MAJOR + 1)).0.0" ;;
        esac
        ;;
    *)
        NEW="$BUMP"
        ;;
esac

REQ="$(truce_req "$NEW")"

# The sibling truce has to satisfy the new requirement, or the lockfile
# refresh below fails with a resolver error that doesn't say why.
TRUCE_VERSION="$(ws_version "$TRUCE_DIR/Cargo.toml")"
if [[ "$(major_minor "$TRUCE_VERSION")" != "$(major_minor "$NEW")" ]]; then
    echo "Error: $TRUCE_DIR is at $TRUCE_VERSION, but truce-extensions $NEW" >&2
    echo "needs truce $(major_minor "$NEW").x. truce releases first: bump it" >&2
    echo "(truce/scripts/release/bump.sh), then re-run this." >&2
    exit 1
fi

echo
echo "Bumping $CURRENT → $NEW (truce requirement $REQ, sibling truce $TRUCE_VERSION)"
echo

# Edit Cargo.toml -------------------------------------------------------------

echo "→ editing Cargo.toml"
sed_inplace \
    -e "/^\[workspace\.package\]/,/^\[/ s/^version = \"[^\"]*\"/version = \"$NEW\"/" \
    -e "/path = \"crates\//    s/version = \"[^\"]*\"/version = \"$NEW\"/" \
    -e "/path = \"\.\.\/truce\// s/version = \"[^\"]*\"/version = \"$REQ\"/" \
    Cargo.toml

# Refresh Cargo.lock ----------------------------------------------------------

# `update --workspace` re-reads the path packages' versions without
# touching any registry dependency's resolution.
echo "→ refreshing Cargo.lock ($CARGO update --workspace)"
"$CARGO" update --workspace

# Commit ----------------------------------------------------------------------

if (( EDIT_ONLY )); then
    echo
    echo "Edited Cargo.toml + Cargo.lock for v$NEW. No commit made."
    exit 0
fi

echo "→ committing on $(git rev-parse --abbrev-ref HEAD)"
git add Cargo.toml Cargo.lock
git commit -m "Release v$NEW"

echo
echo "Bump committed. When truce $(major_minor "$NEW") is on crates.io, run release.sh."
