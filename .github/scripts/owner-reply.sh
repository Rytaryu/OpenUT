#!/usr/bin/env bash
# Hands the owner's reply on a needs-human Issue/PR to the role named in its
# "返事の行き先: role:<role> #<number>" line (docs/project/roles/README.md, section 8).
# No AI is involved: it only moves labels and posts the wake comment.
#
# Env: REPO (owner/name), ISSUE (the needs-human Issue/PR number),
#      COMMENT_URL (optional, link to the owner's comment), WAKE_PR (the number of the wake PR),
#      WAKE_TOKEN (optional, a token of the owner used only for the wake comment).
set -euo pipefail
: "${REPO:?}" "${ISSUE:?}" "${WAKE_PR:?}"
ALLOWED=" role:impl role:review role:design role:ux role:pm "
SIGN="— 受け渡しの仕組み（GitHub Actions。README の 8 章）"

uri() { jq -rn --arg s "$1" '$s|@uri'; }
is_not_found() {
  # gh api --include emits the HTTP status line before any error body.
  local status_line="${1%%$'\n'*}"
  local pattern='^HTTP/[0-9.]+[[:space:]]404([[:space:]]|$)'
  [[ "$status_line" =~ $pattern ]]
}

# Stop switch: do nothing while docs/project/PAUSED exists on the default branch.
if paused=$(gh api "repos/$REPO/contents/docs/project/PAUSED" --include --silent 2>/dev/null); then
  echo "PAUSED: skip"; exit 0
elif ! is_not_found "$paused"; then
  echo "Unable to read PAUSED state: retry without routing" >&2; exit 1
fi

item=$(gh api "repos/$REPO/issues/$ISSUE")
if ! jq -e '[.labels[].name] | index("needs-human")' <<<"$item" >/dev/null; then
  echo "#$ISSUE has no needs-human label (already routed?): skip"; exit 0
fi

# Parse the destination line. Fall back to role:pm on this item.
line=$(jq -r '.body // ""' <<<"$item" | tr -d '\r' | grep -m1 -E '^[[:space:]]*返事の行き先[:：]' || true)
role=$(grep -oE 'role:[a-z-]+' <<<"$line" | head -n1 || true)
target=$(grep -oE '#[0-9]+' <<<"$line" | head -n1 | tr -d '#' || true)
if [[ -z "$role" || "$ALLOWED" != *" $role "* ]]; then
  role="role:pm"; target=""
fi
target="${target:-$ISSUE}"
if [[ "$target" != "$ISSUE" ]]; then
  if response=$(gh api "repos/$REPO/issues/$target" --include --jq .state 2>/dev/null); then
    target_state="${response##*$'\n'}"
    target_state="${target_state%$'\r'}"
  elif is_not_found "$response"; then
    target_state="missing"
  else
    echo "Unable to read target #$target: retry without routing" >&2; exit 1
  fi
  if [[ "$target_state" == "closed" || "$target_state" == "missing" ]]; then
    echo "target #$target is not open: hand to role:pm on #$ISSUE"
    role="role:pm"; target="$ISSUE"
  elif [[ "$target_state" != "open" ]]; then
    echo "Unknown state for target #$target: retry without routing" >&2; exit 1
  fi
fi

# The target gets exactly one role label (and is no longer backlog).
labels=$(gh api "repos/$REPO/issues/$target" --jq '.labels[].name | select(startswith("role:") or . == "backlog")')
for l in $labels; do
  gh api -X DELETE "repos/$REPO/issues/$target/labels/$(uri "$l")" >/dev/null
done
gh api -X POST "repos/$REPO/issues/$target/labels" -f "labels[]=$role" >/dev/null

src="${COMMENT_URL:-#$ISSUE}"
if [[ "$target" != "$ISSUE" ]]; then
  gh api "repos/$REPO/issues/$target/comments" -f body="オーナーが #$ISSUE に返事しました（$src）。\`$role\` の担当は、返事を読んで作業を続けてください。

$SIGN" >/dev/null
fi
gh api "repos/$REPO/issues/$ISSUE/comments" -f body="オーナーの返事（$src）を \`$role\` の #$target に渡しました。

$SIGN" >/dev/null

# The PM has no event trigger (daily run), so no wake comment for role:pm.
if [[ "$role" != "role:pm" ]]; then
  # Comments by the workflow's own bot do not start the ChatGPT tasks; the owner's token does.
  post_wake() {
    GH_TOKEN="${WAKE_TOKEN:-$GH_TOKEN}" gh api "repos/$REPO/issues/$WAKE_PR/comments" -f body="WAKE $role #$target" >/dev/null
  }
  if ! refusal=$(post_wake 2>&1); then
    echo "$refusal" >&2
    # The wake PR is kept locked, so that nobody outside can start the roles
    # (the repository is public). The job's own account cannot comment on a
    # locked conversation: open it for this one comment and lock it again,
    # whether the comment went out or not.
    locked=$(gh api "repos/$REPO/issues/$WAKE_PR" --jq .locked 2>/dev/null || true)
    if [[ "$locked" != "true" ]]; then
      echo "Unable to post the wake comment: retry" >&2; exit 1
    fi
    gh api -X DELETE "repos/$REPO/issues/$WAKE_PR/lock" >/dev/null
    posted=0
    post_wake || posted=$?
    gh api -X PUT "repos/$REPO/issues/$WAKE_PR/lock" >/dev/null
    if [[ "$posted" -ne 0 ]]; then
      echo "Unable to post the wake comment: retry" >&2; exit 1
    fi
  fi
fi
# Retain the retry marker until every routing operation has succeeded.
gh api -X DELETE "repos/$REPO/issues/$ISSUE/labels/needs-human" >/dev/null
echo "routed #$ISSUE -> $role #$target"
