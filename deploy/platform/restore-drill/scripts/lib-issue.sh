#!/bin/sh
# PLATFORM SOURCE, hand-written (#360). Shared issue-filing helper for the drill scripts
# (PROP-20260806-223656 s2b practice 4: a failed rehearsal FILES AN ISSUE -- a log line nobody
# reads is not an alert). Sourced, not executed. Requires: GITHUB_TOKEN, GITHUB_REPO, curl, jq.
#
# file_issue "<title>" "<body>"
#   Files a GitHub issue unless an OPEN issue with the same exact title already exists
#   (hourly checks must not spam one incident into 24 issues). If the dedup search itself
#   fails, the issue is filed anyway: a duplicate beats silence. If FILING fails, the calling
#   script still exits non-zero -- the job failure remains visible in the CronJob history.

file_issue() {
  fi_title="$1"
  fi_body="$2"

  fi_existing=$(
    curl -sS -G "https://api.github.com/search/issues" \
      -H "Authorization: Bearer ${GITHUB_TOKEN}" \
      -H "Accept: application/vnd.github+json" \
      --data-urlencode "q=repo:${GITHUB_REPO} is:issue is:open in:title \"${fi_title}\"" \
      2>/dev/null | jq -r '.total_count // 0' 2>/dev/null || echo 0
  )
  if [ "${fi_existing}" != "0" ] && [ -n "${fi_existing}" ]; then
    echo "open issue with this title already exists (${fi_existing}) -- not filing a duplicate: ${fi_title}"
    return 0
  fi

  if curl -sS -f -X POST "https://api.github.com/repos/${GITHUB_REPO}/issues" \
    -H "Authorization: Bearer ${GITHUB_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    -d "$(jq -n --arg t "${fi_title}" --arg b "${fi_body}" '{title: $t, body: $b}')" \
    -o /tmp/issue-response.json; then
    echo "issue filed: $(jq -r '.html_url // "?"' /tmp/issue-response.json) -- ${fi_title}"
  else
    echo "ISSUE FILING FAILED for: ${fi_title} -- the underlying failure below still stands" >&2
  fi
}
