#!/usr/bin/env bash
set -euo pipefail

tag=$1
commit=$2
gh api --method POST "repos/$GH_REPO/releases/generate-notes" \
  -f tag_name="$tag" -f target_commitish="$commit" --jq .body |
  sed '/^\*\*Full Changelog\*\*:/d'
printf '\n[发布与下载](%s/%s/releases/tag/%s)\n' "$GITHUB_SERVER_URL" "$GH_REPO" "$tag"
