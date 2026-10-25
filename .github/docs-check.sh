#!/usr/bin/env bash
# Which docs cover code that changed since <base> but weren't touched.
#
# A doc names the code it covers in its front matter (`extra.covers`, git
# globs: `*` stays in a directory, `**` crosses them). Run by docs.yml on pull
# requests, where a "docs: none" line in the description excuses it, and by
# the docs-sync skill to know which pages to read.
#
#   .github/docs-check.sh [base]      base defaults to HEAD^1 (CI's merge commit)
set -euo pipefail
from=$(git merge-base "${1:-HEAD^1}" HEAD) # then diff to the working tree, so local edits count
stale=0 broken=0

for doc in $(find docs -name '*.md' | sort); do # sections are folders
  globs=$(awk 'NR == 1 { if ($0 != "---") exit; next }
               /^---$/ { exit }
               /^ *covers:/ { c = 1; next }
               c && /^ *- / { print $2; next }
               { c = 0 }' "$doc")
  [ -n "$globs" ] || continue
  specs=()
  while read -r g; do # not `for g in $globs`: the shell would expand them
    if [ -z "$(git ls-files -- ":(glob)$g")" ]; then
      echo "$doc: covers $g, which matches no file"
      broken=1
    fi
    specs+=(":(glob)$g")
  done <<<"$globs"
  changed=$(git diff --name-only "$from" -- "${specs[@]}")
  if [ -n "$changed" ] && git diff --quiet "$from" -- "$doc"; then
    echo "$doc: not touched, but what it covers changed:"
    echo "$changed" | sed 's/^/  /'
    stale=1
  fi
done

if [ "$stale" = 1 ] && grep -qiE '^docs: none' <<<"${BODY:-}"; then
  echo 'excused: the description says "docs: none"'
  stale=0
fi
exit $((stale | broken))
