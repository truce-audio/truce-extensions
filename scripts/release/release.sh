#!/usr/bin/env bash
#
# release.sh — tag HEAD, publish every truce-extensions crate to
# crates.io, push the tag, create the GitHub Release. Idempotent: each
# step skips if it's already done, so a re-run after a partial failure
# picks up where it left off.
#
# Usage:
#   scripts/release/release.sh             # release
#   scripts/release/release.sh --dry-run   # checks + publish order only
#
# Run from the commit bump.sh made. The version is read from
# Cargo.toml's [workspace.package].version and the tag is `vX.Y.Z`.
#
# truce releases first. Before tagging anything this script requires
# every truce crate the workspace depends on to be on crates.io at a
# version its `~X.Y` requirement accepts: `cargo publish` strips the
# `../truce` paths and resolves those requirements from the registry,
# so publishing ahead of truce would ship crates nobody can build.
#
# Publish order is computed by topo.py from `cargo metadata`.
#
# Rate limits: crates.io throttles first-time publishes harder than
# version updates. After each publish the script sleeps
# INDEX_PROP_DELAY so the next crate's resolver sees the new index
# entry, and a rate-limit failure (429 / "rate limit" / "too many
# requests") retries the same crate with exponential backoff, up to
# MAX_RETRY_ATTEMPTS.
#
# Pre-reqs:
#   - `cargo login <token>` and `gh auth login` already run.
#   - HEAD's Cargo.toml carries the version to ship (bump.sh).

source "$(dirname "$0")/lib.sh"

DRY_RUN=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Error: unknown argument $arg" >&2
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Tunables
# ---------------------------------------------------------------------------

INDEX_PROP_DELAY="${INDEX_PROP_DELAY:-30}"
RATE_LIMIT_INITIAL_DELAY="${RATE_LIMIT_INITIAL_DELAY:-60}"
MAX_RETRY_ATTEMPTS="${MAX_RETRY_ATTEMPTS:-6}"

USER_AGENT="truce-extensions-release-script/1.0 (https://github.com/truce-audio/truce-extensions)"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

crates_io() {
    # Args: <api path>. Fetch https://crates.io/api/v1/<path> to stdout;
    # non-zero when it doesn't exist.
    curl -sf -H "User-Agent: $USER_AGENT" "https://crates.io/api/v1/$1" 2>/dev/null
}

is_published_on_crates_io() {
    crates_io "crates/$1/$2" >/dev/null
}

crate_exists_on_crates_io() {
    crates_io "crates/$1" >/dev/null
}

newest_release_in_line() {
    # Args: <crate> <X.Y>. Print the newest non-yanked, non-pre-release
    # X.Y.* of the crate on crates.io - what a `~X.Y` requirement resolves
    # to - or nothing when there is none.
    crates_io "crates/$1" | python3 -c '
import json, sys
line = sys.argv[1]
nums = [
    v["num"]
    for v in json.load(sys.stdin)["versions"]
    if not v["yanked"] and "-" not in v["num"]
    and v["num"].rsplit(".", 1)[0] == line
]
nums.sort(key=lambda n: tuple(int(x) for x in n.split(".")))
print(nums[-1] if nums else "")
' "$2" 2>/dev/null || true
}

is_tag_on_origin() {
    git ls-remote --tags origin "refs/tags/$1" 2>/dev/null | grep -q "refs/tags/$1$"
}

is_github_release_present() {
    gh release view "$1" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# Verify versions
# ---------------------------------------------------------------------------

echo "→ reading + verifying versions in Cargo.toml"

WS_VERSION="$(ws_version Cargo.toml)"
REQ="$(truce_req "$WS_VERSION")"
LINE="$(major_minor "$WS_VERSION")"

# This repo's crates must pin the workspace version and truce's must
# carry the matching `~X.Y`: cargo embeds both at publish time, and
# drift in either breaks downstream resolution after the fact.
DRIFT="$(
    ws_deps_with_path "crates/" | awk -F'\t' -v want="$WS_VERSION" \
        '$2 != want { printf "  %s = \"%s\" (want \"%s\")\n", $1, $2, want }'
    ws_deps_with_path "../truce/" | awk -F'\t' -v want="$REQ" \
        '$2 != want { printf "  %s = \"%s\" (want \"%s\")\n", $1, $2, want }'
)"
if [[ -n "$DRIFT" ]]; then
    echo "Error: [workspace.dependencies] version drift vs $WS_VERSION:" >&2
    echo "$DRIFT" >&2
    echo "Run bump.sh rather than editing versions by hand." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Verify truce is released
# ---------------------------------------------------------------------------

echo "→ checking truce $LINE.x is on crates.io"

MISSING=""
while IFS=$'\t' read -r crate _; do
    found="$(newest_release_in_line "$crate" "$LINE")"
    if [[ -n "$found" ]]; then
        echo "  $crate $found"
    else
        MISSING="$MISSING $crate"
    fi
done < <(ws_deps_with_path "../truce/")

if [[ -n "$MISSING" ]]; then
    echo "Error: no $LINE.x release on crates.io for:$MISSING" >&2
    echo "Release truce $LINE first (truce/scripts/release/release.sh)." >&2
    exit 1
fi

TAG="v$WS_VERSION"

ORDER="$(python3 scripts/release/topo.py)"
if [[ -z "$ORDER" ]]; then
    echo "Error: no publishable crates in the workspace" >&2
    exit 1
fi

echo
echo "Releasing $TAG (HEAD: $(git rev-parse --short HEAD))"
echo "Publish order:"
printf '%s\n' "$ORDER" | sed 's/^/  /'

if (( DRY_RUN )); then
    echo
    echo "Dry run: checks passed. Nothing tagged or published."
    exit 0
fi

# ---------------------------------------------------------------------------
# Step 1 — local tag
# ---------------------------------------------------------------------------

echo
echo "→ local tag $TAG"
if git rev-parse --verify "$TAG" >/dev/null 2>&1; then
    existing_sha="$(git rev-parse "$TAG^{commit}")"
    head_sha="$(git rev-parse HEAD^{commit})"
    if [[ "$existing_sha" != "$head_sha" ]]; then
        echo "Error: tag $TAG already exists locally at $existing_sha," >&2
        echo "but HEAD is $head_sha. Delete the stale tag with" >&2
        echo "'git tag -d $TAG' or check out the right commit." >&2
        exit 1
    fi
    echo "  already exists at HEAD; using it"
else
    git tag -a "$TAG" -m "truce-extensions $WS_VERSION"
    echo "  created"
fi

# ---------------------------------------------------------------------------
# Step 2 — publish each crate in topo order
# ---------------------------------------------------------------------------

publish_one() {
    # Args: <crate>. `cargo publish -p <crate>`, retrying rate-limit
    # failures with exponential backoff. Any other failure returns at
    # once so the compile / network / auth error stays on screen.
    local crate="$1" log delay attempts=0
    log="$(mktemp -t truce-extensions-publish.XXXXXX)"
    delay="$RATE_LIMIT_INITIAL_DELAY"

    while (( attempts < MAX_RETRY_ATTEMPTS )); do
        attempts=$((attempts + 1))
        # pipefail propagates cargo's exit status through tee.
        if "$CARGO" publish -p "$crate" 2>&1 | tee "$log"; then
            rm -f "$log"
            return 0
        fi
        if ! grep -qiE '429|rate.?limit|too many requests' "$log"; then
            break
        fi
        if (( attempts >= MAX_RETRY_ATTEMPTS )); then
            echo "Error: $crate still rate-limited after $attempts attempts" >&2
            break
        fi
        echo "  rate-limited; sleeping ${delay}s before retry $((attempts + 1))/$MAX_RETRY_ATTEMPTS"
        sleep "$delay"
        delay=$((delay * 2))
    done

    rm -f "$log"
    return 1
}

echo
echo "→ publishing crates"

while IFS= read -r crate; do
    [[ -z "$crate" ]] && continue
    echo
    echo "→ $crate $WS_VERSION"

    if is_published_on_crates_io "$crate" "$WS_VERSION"; then
        echo "  already on crates.io at $WS_VERSION; skipping"
        continue
    fi
    if crate_exists_on_crates_io "$crate"; then
        echo "  (existing crate — publishing new version)"
    else
        echo "  (NEW crate — first publish; rate-limit retries enabled)"
    fi

    publish_one "$crate"

    echo "  sleeping ${INDEX_PROP_DELAY}s for index propagation"
    sleep "$INDEX_PROP_DELAY"
done <<< "$ORDER"

# ---------------------------------------------------------------------------
# Step 3 — push tag
# ---------------------------------------------------------------------------

echo
echo "→ pushing tag $TAG"
if is_tag_on_origin "$TAG"; then
    echo "  already on origin; skipping"
else
    git push origin "$TAG"
fi

# ---------------------------------------------------------------------------
# Step 4 — GitHub Release
# ---------------------------------------------------------------------------

echo
echo "→ creating GitHub Release"
if is_github_release_present "$TAG"; then
    echo "  already exists: $(gh release view "$TAG" --json url --jq .url 2>/dev/null || true)"
else
    gh release create "$TAG" --title "truce-extensions $WS_VERSION"
fi

echo
echo "Released $TAG."
echo "  https://crates.io/crates/truce-format-lv2/$WS_VERSION"
echo
echo "Smoke-test from a clean install:"
echo "  cargo install --force truce-format-lv2 --version $WS_VERSION"
echo "  truce-format-lv2 describe"
echo "  cargo truce doctor"
