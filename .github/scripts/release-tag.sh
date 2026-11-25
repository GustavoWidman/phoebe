#!/usr/bin/env bash
set -euo pipefail

: "${RELEASE_SHA:?set RELEASE_SHA to the validated commit}"
if [[ "$(git rev-parse HEAD)" != "$RELEASE_SHA" ]]; then
  echo "Checkout does not match validated RELEASE_SHA" >&2
  exit 1
fi
VERSION=$(python3 .github/scripts/version.py current)
TAG="v$VERSION"

git fetch origin --tags
TAG_EXISTS=false
if git show-ref --verify --quiet "refs/tags/$TAG"; then
  TAG_EXISTS=true
  TAG_SHA=$(git rev-parse "$TAG^{commit}")
  if ! git merge-base --is-ancestor "$TAG_SHA" "$RELEASE_SHA"; then
    echo "Existing $TAG is not an ancestor of the validated commit" >&2
    exit 1
  fi
fi

# A lookup by tag never returns a draft, so list every release, and let a failed
# request fail the run rather than read as no release.
RELEASES=$(gh api --paginate 'repos/{owner}/{repo}/releases?per_page=100')
if jq -es --arg tag "$TAG" 'any(.[][]; .tag_name == $tag)' <<<"$RELEASES" >/dev/null; then
  if [[ "$TAG_EXISTS" != true ]]; then
    echo "Release $TAG exists without its tag; refusing to recreate it" >&2
    exit 1
  fi
  echo "Release $TAG already exists; nothing to do."
  exit 0
fi
if [[ "$TAG_EXISTS" == true && "$TAG_SHA" != "$RELEASE_SHA" ]]; then
  echo "Missing release for $TAG: rerun the release at $TAG_SHA, not $RELEASE_SHA" >&2
  exit 1
fi

PREV_TAG=$(git describe --tags --match 'v[0-9]*' --abbrev=0 "$RELEASE_SHA^" 2>/dev/null || true)
RANGE="$RELEASE_SHA"
if [[ -n "$PREV_TAG" ]]; then RANGE="$PREV_TAG..$RELEASE_SHA"; fi
NOTES_FILE=$(mktemp)
trap 'rm -f "$NOTES_FILE"' EXIT
{
  printf 'Commits since %s:\n\n' "${PREV_TAG:-the start}"
  git log --reverse --format='- %s (%h)' "$RANGE" --
} >"$NOTES_FILE"

if [[ "$TAG_EXISTS" != true ]]; then
  git config user.name "github-actions[bot]"
  git config user.email "github-actions[bot]@users.noreply.github.com"
  git tag -a "$TAG" "$RELEASE_SHA" -m "phoebe $TAG"
  git push origin "refs/tags/$TAG"
fi
gh release create "$TAG" --verify-tag --title "phoebe $TAG" --notes-file "$NOTES_FILE"
echo "Released $TAG"
