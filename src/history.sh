#!/bin/bash
set -euo pipefail

# validation
if [ $# -lt 2 ]; then
    echo "Error: Missing arguments."
    echo "Usage: $0 [repo] [lookback_days]"
    exit 1
fi

OWNER="moleraat"
REPO=$1
LOOKBACK_DAYS=$2

[[ "$LOOKBACK_DAYS" =~ ^[0-9]+$ ]] || { echo "Error: lookback_days must be a positive integer."; exit 1; }

# get commit history across all branches, write to file
# (works on Ubuntu, not on Mac)
since=$(date -u -d "$LOOKBACK_DAYS days ago" +%Y-%m-%d)

branches=$(gh api repos/$OWNER/$REPO/branches --jq '.[].name')
for branch in $branches; do
    gh api "repos/$OWNER/$REPO/commits?sha=$branch&since=$since" \
      --jq ".[] | {hash: .sha, branch: \"$branch\", date: .commit.author.date, subject: .commit.message, link: .html_url}"
done
