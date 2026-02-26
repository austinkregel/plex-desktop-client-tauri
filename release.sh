#!/usr/bin/env bash

# Release script for GTK native Simplex.
# Usage: ./release.sh <version> [release-notes]

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [ -z "${1:-}" ]; then
  echo -e "${RED}Error: Version number is required${NC}"
  echo "Usage: $0 <version> [release-notes]"
  exit 1
fi

VERSION="$1"
RELEASE_NOTES="${2:-Release version $VERSION}"
TAG_NAME="v${VERSION}"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+)?(\+[a-zA-Z0-9]+)?$ ]]; then
  echo -e "${YELLOW}Warning: version does not look semver (X.Y.Z)${NC}"
fi

if [ -n "$(git status --porcelain)" ]; then
  echo -e "${YELLOW}Warning: working tree is not clean${NC}"
  git status --short
  read -p "Continue anyway? (y/N) " -n 1 -r
  echo
  if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    exit 1
  fi
fi

if git rev-parse "$TAG_NAME" >/dev/null 2>&1; then
  echo -e "${RED}Error: tag $TAG_NAME already exists${NC}"
  exit 1
fi

echo -e "${GREEN}Updating crate versions to ${VERSION}${NC}"
if [[ "$OSTYPE" == "darwin"* ]]; then
  sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" crates/simplex-app/Cargo.toml
  sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" crates/simplex-core/Cargo.toml
else
  sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/simplex-app/Cargo.toml
  sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/simplex-core/Cargo.toml
fi

echo -e "${GREEN}Refreshing lock file${NC}"
cargo check -p simplex-app >/dev/null

echo -e "${GREEN}Changes:${NC}"
git diff --stat

read -p "Commit and tag release? (Y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Nn]$ ]]; then
  echo -e "${YELLOW}Aborted. Version files were updated locally only.${NC}"
  exit 1
fi

git add crates/simplex-app/Cargo.toml crates/simplex-core/Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "Bump version to $VERSION"
git tag -a "$TAG_NAME" -m "$RELEASE_NOTES"

echo -e "${GREEN}Release tag created:${NC} $TAG_NAME"
echo "Push when ready:"
echo "  git push"
echo "  git push origin $TAG_NAME"

