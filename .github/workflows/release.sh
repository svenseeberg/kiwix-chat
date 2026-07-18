#!/bin/bash
set -euo pipefail

# Args: $1 = GitHub token, $2 = git ref (refs/tags/vX.Y.Z)
TOKEN="$1"
REF="$2"
# GITHUB_REPOSITORY (owner/repo) is provided automatically by GitHub Actions.
REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY not set}"
BINARY="kiwix-chat"
ASSET_NAME="kiwix-chat-x86_64-unknown-linux-gnu"

TAG=$(echo "$REF" | sed "s#refs/tags/##")
VERSION=$(echo "$REF" | sed "s#refs/tags/v##")
echo "Creating release for version $VERSION (tag $TAG) in $REPO"

echo "Building release binary..."
cargo build --release
ls -lah ./target/release/

echo "Creating GitHub release..."
RESPONSE=$(curl -fsSL -X POST \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "https://api.github.com/repos/$REPO/releases" \
  -d "{\"tag_name\":\"$TAG\",\"target_commitish\":\"main\",\"name\":\"$TAG\",\"body\":\"Version $VERSION\",\"draft\":false,\"prerelease\":false,\"generate_release_notes\":true}")
echo "$RESPONSE"

RELEASE_ID=$(echo "$RESPONSE" | jq .id)
if [ "$RELEASE_ID" = "null" ] || [ -z "$RELEASE_ID" ]; then
  echo "Failed to create release" >&2
  exit 1
fi

echo "Attaching binary to release $RELEASE_ID as $ASSET_NAME"
curl -fsSL -X POST \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @"./target/release/$BINARY" \
  "https://uploads.github.com/repos/$REPO/releases/$RELEASE_ID/assets?name=$ASSET_NAME"

echo "Finished: $?"
