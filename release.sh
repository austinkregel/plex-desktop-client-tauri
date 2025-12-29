#!/bin/bash

# Release script for Plex Desktop
# Usage: ./release.sh <version> [release-notes]
# Example: ./release.sh 1.0.0 "Initial release"
# Example: ./release.sh 1.1.0

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if version is provided
if [ -z "$1" ]; then
    echo -e "${RED}Error: Version number is required${NC}"
    echo "Usage: $0 <version> [release-notes]"
    echo "Example: $0 1.0.0 \"Initial release\""
    exit 1
fi

VERSION="$1"
RELEASE_NOTES="${2:-Release version $VERSION}"
TAG_NAME="v${VERSION}"

# Validate version format (semantic versioning)
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+)?(\+[a-zA-Z0-9]+)?$ ]]; then
    echo -e "${YELLOW}Warning: Version format doesn't match semantic versioning (X.Y.Z)${NC}"
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

echo -e "${GREEN}🚀 Starting release process for version $VERSION${NC}"
echo ""

# Check if working directory is clean
if [ -n "$(git status --porcelain)" ]; then
    echo -e "${YELLOW}Warning: Working directory is not clean${NC}"
    git status --short
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

# Check if tag already exists
if git rev-parse "$TAG_NAME" >/dev/null 2>&1; then
    echo -e "${RED}Error: Tag $TAG_NAME already exists${NC}"
    exit 1
fi

# Check if we're on main/master branch
CURRENT_BRANCH=$(git branch --show-current)
if [[ "$CURRENT_BRANCH" != "main" && "$CURRENT_BRANCH" != "master" ]]; then
    echo -e "${YELLOW}Warning: Not on main/master branch (currently on $CURRENT_BRANCH)${NC}"
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

echo -e "${GREEN}📝 Step 1: Updating version in package.json${NC}"
npm version "$VERSION" --no-git-tag-version --allow-same-version || {
    # Fallback if npm version fails
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" package.json
    else
        sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" package.json
    fi
}

echo -e "${GREEN}📝 Step 2: Updating version in src-tauri/tauri.conf.json${NC}"
if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" src-tauri/tauri.conf.json
else
    sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" src-tauri/tauri.conf.json
fi

echo -e "${GREEN}📝 Step 3: Updating version in src-tauri/Cargo.toml${NC}"
if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml
else
    sed -i "s/^version = \".*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml
fi

# Also update Android versionName if it exists
if grep -q "versionName" src-tauri/tauri.conf.json; then
    echo -e "${GREEN}📝 Step 4: Updating Android versionName in src-tauri/tauri.conf.json${NC}"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "s/\"versionName\": \".*\"/\"versionName\": \"$VERSION\"/" src-tauri/tauri.conf.json
    else
        sed -i "s/\"versionName\": \".*\"/\"versionName\": \"$VERSION\"/" src-tauri/tauri.conf.json
    fi
    git add src-tauri/tauri.conf.json
fi

echo ""
echo -e "${GREEN}✅ Version files updated${NC}"
echo ""

# Show what changed
echo -e "${GREEN}📋 Changes to be committed:${NC}"
git diff --stat
echo ""

# Confirm before committing
read -p "Commit these changes? (Y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Nn]$ ]]; then
    echo -e "${YELLOW}Aborted. Changes are staged but not committed.${NC}"
    exit 1
fi

echo -e "${GREEN}💾 Step 5: Committing version changes${NC}"
git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "Bump version to $VERSION"

echo ""
echo -e "${GREEN}🏷️  Step 6: Creating git tag${NC}"
git tag -a "$TAG_NAME" -m "$RELEASE_NOTES"

echo ""
echo -e "${GREEN}📤 Step 7: Pushing commits and tag${NC}"
read -p "Push to remote? (Y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Nn]$ ]]; then
    echo -e "${YELLOW}Tag and commit created locally. Push manually with:${NC}"
    echo "  git push"
    echo "  git push origin $TAG_NAME"
    exit 0
fi

git push
git push origin "$TAG_NAME"

echo ""
echo -e "${GREEN}✅ Release process complete!${NC}"
echo ""
echo -e "${GREEN}Next steps:${NC}"
echo "1. Go to your GitHub repository"
echo "2. Navigate to Releases → Draft a new release"
echo "3. Select tag: $TAG_NAME"
echo "4. Add release title and description"
echo "5. Click 'Publish release' to trigger the build workflow"
echo ""
echo -e "${YELLOW}Or use GitHub CLI to create the release automatically:${NC}"
echo "  gh release create $TAG_NAME --title \"Plex Desktop v$VERSION\" --notes \"$RELEASE_NOTES\""
echo ""

# Check if GitHub CLI is available
if command -v gh &> /dev/null; then
    read -p "Create GitHub release now using GitHub CLI? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo -e "${GREEN}📦 Creating GitHub release...${NC}"
        gh release create "$TAG_NAME" \
            --title "Plex Desktop v$VERSION" \
            --notes "$RELEASE_NOTES" \
            --draft=false
        echo ""
        echo -e "${GREEN}✅ GitHub release created! Build workflow will start automatically.${NC}"
    fi
fi

