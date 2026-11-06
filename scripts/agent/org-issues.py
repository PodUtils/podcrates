"""Read the issue lists of the repositories of PodUtils (one JSON array of
the GitHub API for each repository, on stdin, each line `REPO<TAB>JSON`) and
print one line for each message to ME (a repository name of PodUtils:
podcrates, podssh, podnet, podvm) that is new or changed since the last
run:

* each issue of PodUtils/ME;
* each issue of another repository of the organization whose title holds
  `(to ME)` or `(to all)`, or that has the label ME.

A message is changed when its `updated_at` moved: the writer amended its
body (docs/maintenance.md, "Talk between sessions"). STATE keeps
`REPO#NUMBER UPDATED_AT` for each message seen; with ANNOUNCE=0 the run
records and prints nothing (the first run). Each message is kept in
issues/OWNER_REPO_NUMBER.json beside STATE, as GitHub deletes the issues of
a suspended account.

    python3 -I scripts/agent/org-issues.py STATE ANNOUNCE [ME]

ME is podcrates when it is not given.
"""

import json
import os
import sys


def meant(me: str, repo: str, issue: dict) -> bool:
    if repo == f"PodUtils/{me}":
        return True
    labels = [label.get("name") for label in issue.get("labels") or []]
    title = issue.get("title") or ""
    return f"(to {me})" in title or "(to all)" in title or me in labels


def main() -> int:
    state_path, announce = sys.argv[1], sys.argv[2] == "1"
    me = sys.argv[3] if len(sys.argv) > 3 else "podcrates"
    seen = {}
    if os.path.exists(state_path):
        for line in open(state_path, encoding="utf-8"):
            key, _, stamp = line.strip().partition(" ")
            if key:
                seen[key] = stamp
    keep = os.path.join(os.path.dirname(os.path.abspath(state_path)), "issues")
    os.makedirs(keep, exist_ok=True)
    for line in sys.stdin:
        repo, _, text = line.rstrip("\n").partition("\t")
        try:
            items = json.loads(text)
        except ValueError:
            continue
        if not isinstance(items, list):
            continue
        for i in items:
            if not isinstance(i, dict) or "pull_request" in i or not meant(me, repo, i):
                continue
            key = f"{repo}#{i['number']}"
            stamp = i.get("updated_at") or ""
            copy = {k: i.get(k) for k in ("number", "title", "body", "state", "created_at", "updated_at", "html_url")}
            copy["repo"] = repo
            copy["user"] = (i.get("user") or {}).get("login")
            copy["labels"] = [label.get("name") for label in i.get("labels") or []]
            path = os.path.join(keep, f"{repo.replace('/', '_')}_{i['number']}.json")
            with open(path + ".tmp", "w", encoding="utf-8") as f:
                json.dump(copy, f, indent=1)
            os.replace(path + ".tmp", path)
            if seen.get(key) == stamp:
                continue
            word = "NEW" if key not in seen else "AMENDED"
            seen[key] = stamp
            if announce:
                print(f"{word} {key} [{i.get('state')}] by {copy['user']}: {i.get('title')}", flush=True)
    with open(state_path + ".tmp", "w", encoding="utf-8") as f:
        for key in sorted(seen):
            f.write(f"{key} {seen[key]}\n")
    os.replace(state_path + ".tmp", state_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
