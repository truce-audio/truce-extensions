#!/usr/bin/env bash
#
# align-truce.sh — set the sibling ../truce checkout's workspace version to
# this workspace's, when their major.minor differ.
#
# Usage:
#   scripts/align-truce.sh
#
# The truce dependencies here carry `~X.Y` for this workspace's
# major.minor, but truce only bumps its own version when it releases.
# Between a truce release branch opening and that release being cut, the
# two can't build against each other. This makes the same edit truce's
# own `bump.sh --edit-only` starts with - every "OLD" version string in
# ../truce/Cargo.toml becomes "NEW" - without its full-workspace
# lockfile refresh, which nothing here needs.
#
# CI runs it on a throwaway checkout. Locally it edits your truce
# working tree; undo it with `git -C ../truce checkout -- Cargo.toml`.
# A no-op when the major.minor already match.

source "$(dirname "$0")/release/lib.sh"

OURS="$(ws_version Cargo.toml)"
THEIRS="$(ws_version "$TRUCE_DIR/Cargo.toml")"

if [[ "$(major_minor "$THEIRS")" == "$(major_minor "$OURS")" ]]; then
    echo "$TRUCE_DIR is at $THEIRS, already on the $(major_minor "$OURS") line; nothing to do."
    exit 0
fi

echo "→ $TRUCE_DIR/Cargo.toml: $THEIRS → $OURS"
sed_inplace "s/\"$THEIRS\"/\"$OURS\"/g" "$TRUCE_DIR/Cargo.toml"
