#!/usr/bin/env bash
# Prints `version=X.Y.Z` and `skip=true|false` for the release workflow.
# Rules (Conventional Commits since the latest vX.Y.Z tag):
#   breaking change -> major (minor while the major version is 0)
#   feat            -> minor
#   anything else   -> patch
set -euo pipefail

last=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)
if [ -z "$last" ]; then
  # First release: take the version already in Cargo.toml.
  v=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
  echo "version=$v"; echo "skip=false"; exit 0
fi

range="$last..HEAD"
if [ "$(git rev-list --count "$range")" = "0" ]; then
  echo "version=${last#v}"; echo "skip=true"; exit 0
fi

IFS=. read -r major minor patch <<<"${last#v}"
log=$(git log --format='%s%n%b' "$range")
if grep -qE '^[a-z]+(\([^)]*\))?!:|BREAKING[ -]CHANGE' <<<"$log"; then
  if [ "$major" -eq 0 ]; then minor=$((minor + 1)); patch=0; else major=$((major + 1)); minor=0; patch=0; fi
elif grep -qE '^feat(\([^)]*\))?:' <<<"$log"; then
  minor=$((minor + 1)); patch=0
else
  patch=$((patch + 1))
fi
echo "version=$major.$minor.$patch"; echo "skip=false"
