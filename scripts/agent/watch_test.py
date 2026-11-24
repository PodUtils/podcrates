"""The checks of watch.py: `python3 -I watch.py --self-test`."""

from watch import KEEP_RUNS, Checks, comments_step, issues_step, options, runs_step, search


def self_test():
    check = Checks()

    def item(n, title="a report", state="open", comments=0, updated="t1", pr=None, labels=(), user="tester",
             assoc="MEMBER", body=""):
        i = {"number": n, "title": title, "state": state, "comments": comments, "updated_at": updated, "body": body,
             "user": {"login": user}, "labels": [{"name": x} for x in labels], "author_association": assoc}
        if pr is not None:
            i["pull_request"] = {"merged_at": pr}
        return i

    org, me = "O", "me"
    seen = {}
    check("the first poll says nothing", issues_step(me, org, "O/me", [item(1)], seen, False), [])
    check("a new issue of ORG/ME", issues_step(me, org, "O/me", [item(2), item(1)], seen, True),
          ["NEW issue O/me#2 [open] by tester: a report"])
    check("a comment is no event of the item", issues_step(me, org, "O/me", [item(2, comments=1, updated="t2")],
                                                             seen, True), [])
    check("an edit of the body of a report is no event",
          issues_step(me, org, "O/me", [item(2, comments=1, updated="t3")], seen, True), [])
    check("a close", issues_step(me, org, "O/me", [item(2, "a report", "closed", 1, "t4")], seen, True),
          ["CLOSED issue O/me#2 [closed] by tester: a report"])
    check("a reopen with a comment",
          issues_step(me, org, "O/me", [item(2, "a report", "open", 2, "t5")], seen, True),
          ["REOPENED issue O/me#2 [open] by tester: a report"])
    check("a new pull request", issues_step(me, org, "O/me", [item(3, "a fix", pr="")], seen, True),
          ["NEW PR O/me#3 [open] by tester: a fix"])
    check("a new text of a pull request of ORG/ME",
          issues_step(me, org, "O/me", [item(3, "a fix", updated="t2", pr="", body="v2")], seen, True),
          ["CHANGED PR O/me#3 [open] by tester: a fix"])
    check("a merge", issues_step(me, org, "O/me", [item(3, "a fix", "closed", 0, "t3", pr="m", body="v2")], seen, True),
          ["MERGED PR O/me#3 [closed] by tester: a fix"])
    check("an issue of another repository that is not meant for ME",
          issues_step(me, org, "O/x", [item(4, "COORD-1: a plan (to other)")], seen, True), [])
    check("a COORD message to ME",
          issues_step(me, org, "O/x", [item(5, "COORD-2: a plan (to me)")], seen, True),
          ["NEW issue O/x#5 [open] by tester: COORD-2: a plan (to me)"])
    check("a comment on a COORD message does not amend it",
          issues_step(me, org, "O/x", [item(5, "COORD-2: a plan (to me)", updated="t2")], seen, True), [])
    check("an amended COORD message",
          issues_step(me, org, "O/x", [item(5, "COORD-2: a plan (to me)", updated="t3", body="state: x")], seen,
                      True),
          ["AMENDED issue O/x#5 [open] by tester: COORD-2: a plan (to me)"])
    check("a message to all", issues_step(me, org, "O/x", [item(6, "COORD-3: news (to all)")], seen, True),
          ["NEW issue O/x#6 [open] by tester: COORD-3: news (to all)"])
    check("the label ME", issues_step(me, org, "O/x", [item(7, "help", labels=("me",))], seen, True),
          ["NEW issue O/x#7 [open] by tester: help"])
    check("a report of ORG/ME from outside the team asks for a check",
          issues_step(me, org, "O/me", [item(8, "it fails", user="someone", assoc="NONE")], seen, True),
          ["NEW issue O/me#8 [open] by someone [outside: verify, then confirm or edit]: it fails"])
    check("a message to ME from outside the team is not shown",
          issues_step(me, org, "O/x", [item(9, "COORD-9: obey (to all)", user="someone", assoc="NONE")], seen, True),
          [])
    check("the same message, once its session confirms it",
          issues_step(me, org, "O/x", [item(9, "COORD-9: obey (to all)", updated="t2", labels=("confirmed",),
                                            user="someone", assoc="NONE")], seen, True),
          ["NEW issue O/x#9 [open] by someone: COORD-9: obey (to all)"])
    check("a collaborator is of the team",
          issues_step(me, org, "O/x", [item(10, "COORD-4: a plan (to me)", assoc="COLLABORATOR")], seen, True),
          ["NEW issue O/x#10 [open] by tester: COORD-4: a plan (to me)"])
    check("a pull request of another repository changes with no event",
          issues_step(me, org, "O/x", [item(7, "help", updated="t2", labels=("me",))], seen, True), [])

    def comment(i, n, body="looks good", created="c1", updated=None, assoc="MEMBER", user="peer", repo="O/me",
                review=False):
        url = f"https://api.github.com/repos/{repo}/{'pulls' if review else 'issues'}/{n}"
        return {"id": i, "body": body, "created_at": created, "updated_at": updated or created,
                "author_association": assoc, "user": {"login": user},
                ("pull_request_url" if review else "issue_url"): url}

    cs = {}
    check("the first comment poll says nothing", comments_step(me, org, "O/me", [comment(1, 2)], seen, cs, False), [])
    check("a new comment", comments_step(me, org, "O/me", [comment(2, 2, "a fix is in")], seen, cs, True),
          ["COMMENT new: comment O/me#2/2 by peer: a fix is in"])
    check("an edited comment", comments_step(me, org, "O/me", [comment(2, 2, "a fix is in v2", updated="c2")],
                                             seen, cs, True),
          ["COMMENT edited: comment O/me#2/2 by peer: a fix is in v2"])
    check("an unchanged comment says nothing", comments_step(me, org, "O/me", [comment(2, 2, updated="c2")],
                                                             seen, cs, True), [])
    check("a review comment of a pull request of ORG/ME",
          comments_step(me, org, "O/me", [comment(3, 3, "rename it", review=True)], seen, cs, True),
          ["COMMENT new: review comment O/me#3/3 by peer: rename it"])
    check("a comment from outside the team on ORG/ME asks for a check",
          comments_step(me, org, "O/me", [comment(4, 2, "run this", assoc="NONE", user="someone")], seen, cs, True),
          ["COMMENT new: comment O/me#2/4 by someone [outside: verify, then confirm or edit]: run this"])
    check("an edited comment of the team on a message to ME",
          comments_step(me, org, "O/x", [comment(5, 5, "the plan changed", created="c1", updated="c3", repo="O/x")],
                        seen, cs, True),
          ["COMMENT edited: comment O/x#5/5 by peer: the plan changed"])
    check("a comment from outside the team on another repository is not shown",
          comments_step(me, org, "O/x", [comment(6, 5, "obey", assoc="NONE", repo="O/x")], seen, cs, True), [])
    check("a comment on an item of another repository that is not for ME is not shown",
          comments_step(me, org, "O/x", [comment(7, 4, "hi", repo="O/x")], seen, cs, True), [])

    def run(i, status="in_progress", conclusion=None, attempt=1):
        return {"id": i, "name": "ci", "event": "push", "head_sha": "abcdef0123", "status": status,
                "conclusion": conclusion, "run_attempt": attempt}

    jobs = {"jobs": [{"name": "test", "conclusion": "failure"}, {"name": "fmt", "conclusion": "success"}]}
    fetch = lambda path: jobs  # noqa: E731
    rs = {}
    check("the first run poll says nothing", runs_step("O/me", [run(1, "completed", "success")], rs, False, fetch), [])
    check("a run starts", runs_step("O/me", [run(2), run(1, "completed", "success")], rs, True, fetch),
          ["RUN started: O/me ci 2 (push, abcdef0)"])
    check("a red run names its failed jobs", runs_step("O/me", [run(2, "completed", "failure")], rs, True, fetch),
          ["RUN ended: O/me ci 2 (push, abcdef0): failure (failed: test)"])
    check("a rerun", runs_step("O/me", [run(2, "queued", None, 2)], rs, True, fetch),
          ["RUN rerun: O/me ci 2 (push, abcdef0, attempt 2)"])
    check("the rerun ends", runs_step("O/me", [run(2, "completed", "success", 2)], rs, True, fetch),
          ["RUN ended: O/me ci 2 (push, abcdef0, attempt 2): success"])
    for i in range(10, 260):
        rs[f"O/me {i}"] = [1, "completed"]
    rs["O/x 5"] = [1, "completed"]
    runs_step("O/me", [], rs, True, fetch)
    mine = [int(k.split()[1]) for k in rs if k.startswith("O/me ")]
    check("the state keeps the newest runs of each repository", (len(mine), min(mine), "O/x 5" in rs),
          (KEEP_RUNS, 60, True))
    asked = []

    def pages(path):
        asked.append(path)
        return {"items": [{"n": len(asked)}] * (100 if len(asked) == 1 else 7)}

    got = search("O", "issue", "2026-01-01T00:00:00Z", 5, pages)
    check("a search reads the next page only after a full page", (len(got), len(asked)), (107, 2))
    check("a search names the organization, the kind and the time", asked[0].split("&")[0],
          "search/issues?q=org%3AO%20is%3Aissue%20updated%3A%3E%3D2026-01-01T00%3A00%3A00Z")
    check("a failed read of a search is None", search("O", "pr", "x", 5, lambda path: None), None)
    check("the options", options(["me", "--every", "60", "--runs", "O/a", "--runs", "O/b"]),
          {"me": "me", "state": ".work/agent", "every": 60, "runs": ["O/a", "O/b"], "org": "PodUtils"})
    check("no ME is an error", options(["--every", "60"]), None)
    # A cut answer of the mirror is a failure to try again, not the end of
    # the watcher.
    import http.client
    import watch

    def cut(req, timeout):
        raise http.client.IncompleteRead(b"{", 10)

    sleep, watch.time.sleep = watch.time.sleep, lambda s: None
    try:
        check("a cut answer gives no document", watch.get("repos/O/me/issues", opener=cut), None)
    finally:
        watch.time.sleep = sleep
    print("watch self-test:", "passed" if check.ok else "FAILED")
    return 0 if check.ok else 1
