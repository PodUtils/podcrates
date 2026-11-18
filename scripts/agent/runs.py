"""Helpers of watch-runs.sh. With DONE_IDS: read a run list (JSON on stdin)
and print RUN lines for the ended runs that are not in DONE_IDS, then a
STATE line (runs, runs not ended). With --jobs: read a job list and print
the jobs that did not pass, with their failed steps.
"""

import json
import sys

text = sys.stdin.read()
try:
    data = json.loads(text)
except ValueError:
    # The mirror can answer with no JSON (a 403 of its rate limit): say so in
    # one line; the loop of watch-runs.sh reads the runs again.
    if sys.argv[1:] == ["--jobs"]:
        print("  the job list did not come (no JSON from the mirror)")
    else:
        print("STATE 0 1")
    sys.exit(0)
if sys.argv[1:] == ["--jobs"]:
    for j in data.get("jobs", []):
        if j["conclusion"] not in ("success", "skipped"):
            steps = [s["name"] for s in j.get("steps", []) if s.get("conclusion") == "failure"]
            print("  job", j["id"], j["name"], j["conclusion"], steps)
    sys.exit(0)
done = set(sys.argv[1].split()) if len(sys.argv) > 1 else set()
runs = data.get("workflow_runs", [])
pending = 0
for r in runs:
    if r["status"] != "completed":
        pending += 1
        continue
    if str(r["id"]) in done:
        continue
    print("RUN", r["id"], r["name"], r["conclusion"], r["html_url"])
print("STATE", len(runs), pending)
