#!/bin/sh
# Tag the current commit as a release, which triggers the release workflow.
#
#   ./update_release.sh            # tag v<version from Cargo.toml>
#   ./update_release.sh v0.3.1     # tag explicitly
#
# Options:
#   --skip-ci      don't wait on / check GitHub CI for this commit
#   --skip-tests   don't run the local test suite

set -eu

SKIP_CI=0
SKIP_TESTS=0
TAG=""

for arg in "$@"; do
    case "$arg" in
        --skip-ci)    SKIP_CI=1 ;;
        --skip-tests) SKIP_TESTS=1 ;;
        -*) echo "unknown option: $arg" >&2; exit 1 ;;
        *)  TAG="$arg" ;;
    esac
done

say()  { printf '\n==> %s\n' "$1"; }
err()  { printf 'error: %s\n' "$1" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[ -n "$VERSION" ] || err "could not read version from Cargo.toml"
VERSION_TAG="v$VERSION"

[ -n "$TAG" ] || TAG="$VERSION_TAG"

SHA="$(git rev-parse HEAD)"

say "Releasing $TAG at $(git rev-parse --short HEAD)"

# --- Preflight -------------------------------------------------------------

[ -z "$(git status --porcelain)" ] \
    || err "working tree is dirty. Commit or stash first."

# Pick up tags pushed from elsewhere, so the checks below see them too
git fetch --quiet --tags origin

tag_exists() { git rev-parse --verify --quiet "refs/tags/$1" >/dev/null; }

# The version in Cargo.toml must not already be released, whatever tag
# was asked for. Catches forgetting to bump the version.
if tag_exists "$VERSION_TAG"; then
    err "Cargo.toml is at $VERSION, but $VERSION_TAG is already released.
Bump the version in Cargo.toml, or delete the tag:
  git tag -d $VERSION_TAG && git push origin :refs/tags/$VERSION_TAG"
fi

if [ "$TAG" != "$VERSION_TAG" ]; then
    printf 'warning: tag %s does not match Cargo.toml version %s\n' "$TAG" "$VERSION" >&2

    if tag_exists "$TAG"; then
        err "tag $TAG already exists. Delete it, or pick another:
  git tag -d $TAG && git push origin :refs/tags/$TAG"
    fi
fi

# Abort if this commit was already released under some other tag. Only plush
# tags count: the extension workflow tags its own commits vsix-*
existing="$(git tag --points-at HEAD --list 'v[0-9]*' | head -1)"
[ -z "$existing" ] \
    || err "HEAD is already tagged as $existing. Nothing new to release."

branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$SHA" = "$(git rev-parse "origin/$branch")" ] \
    || err "HEAD differs from origin/$branch. Push your commits first."

# --- CI status -------------------------------------------------------------

if [ "$SKIP_CI" = 0 ]; then
    if ! command -v gh >/dev/null 2>&1; then
        err "gh CLI not found, needed to check CI status. Install it, or pass --skip-ci"
    fi

    say "Checking CI status for $SHA"

    # Only the test workflow gates a release; the release workflow's own
    # runs against this commit are irrelevant here
    runs="$(gh run list --commit "$SHA" --workflow test.yml \
              --json status,conclusion --jq '.[]' 2>/dev/null || true)"

    [ -n "$runs" ] || err "no CI run found for $SHA. Wait for it to start, or pass --skip-ci"

    if printf '%s' "$runs" | grep -qv '"status":"completed"'; then
        err "CI is still running for $SHA. Wait for it to finish."
    fi

    if printf '%s' "$runs" | grep -qv '"conclusion":"success"'; then
        err "CI did not pass for $SHA. See: gh run list --commit $SHA"
    fi

    echo "CI passed."
fi

# --- Local tests -----------------------------------------------------------

if [ "$SKIP_TESTS" = 0 ]; then
    say "Running tests (debug)"
    RUST_BACKTRACE=1 cargo test

    say "Running tests (release)"
    RUST_BACKTRACE=1 cargo test --release

    # The release build is what ships, so make sure it links statically too
    say "Checking the static-sdl release build"
    cargo build --release --locked --features static-sdl
fi

# --- Tag and push ----------------------------------------------------------

say "Tagging and pushing $TAG"
git tag -a "$TAG" -m "Release $TAG"
git push origin "$TAG"

say "Pushed $TAG. The release workflow is now building:"
echo "  https://github.com/maximecb/plush/actions"
