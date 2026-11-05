#!/usr/bin/env python3
"""Publish the crates of the workspace to crates.io with trusted publishing,
in the job `publish` of .github/workflows/publish.yml.

  python3 -I scripts/crates-io-publish.py [CARGO_ARGUMENT...]

1. Get the OIDC token of the job from GitHub, for the audience crates.io
   (the job needs `id-token: write`).
2. Give it to crates.io. crates.io compares the repository, the workflow
   file and the environment with the trusted publisher of each crate, and
   gives a token that lasts 30 minutes.
3. Run `cargo publish --workspace --locked` with that token in
   CARGO_REGISTRY_TOKEN, then revoke the token.

No token goes into argv, a file or the log: the runner masks each one
before it is used. An error names the step and the answer of the service,
never a token.
"""

import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

TOKENS = "https://crates.io/api/v1/trusted_publishing/tokens"
AGENT = "podcrates-publish (https://github.com/PodUtils/podcrates)"


class Refused(Exception):
    pass


def call(step: str, method: str, url: str, headers: dict, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method=method, headers={"User-Agent": AGENT, **headers})
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            text = answer.read()
    except urllib.error.HTTPError as e:
        # The body of an error of GitHub or crates.io holds no token.
        detail = e.read(2000).decode("utf-8", "replace").strip()
        raise Refused(f"{step}: HTTP {e.code}: {detail}") from None
    except (urllib.error.URLError, TimeoutError) as e:
        raise Refused(f"{step}: {type(e).__name__}: {getattr(e, 'reason', e)}") from None
    return json.loads(text) if text else None


def mask(value: str) -> None:
    # A workflow command: the runner hides the value in each later line.
    print(f"::add-mask::{value}", flush=True)


def main(argv: list[str]) -> int:
    url = os.environ.get("ACTIONS_ID_TOKEN_REQUEST_URL")
    request = os.environ.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
    if not url or not request:
        print("crates-io-publish: no OIDC request in the environment: the job needs id-token: write", file=sys.stderr)
        return 2
    sep = "&" if "?" in url else "?"
    try:
        jwt = call("the OIDC token of GitHub", "GET", f"{url}{sep}audience=crates.io",
                   {"Authorization": f"bearer {request}"})["value"]
        mask(jwt)
        token = call("the token of crates.io", "POST", TOKENS, {}, {"jwt": jwt})["token"]
        mask(token)
    except (Refused, KeyError, TypeError, ValueError) as e:
        print(f"crates-io-publish: {e}", file=sys.stderr)
        return 1
    try:
        env = dict(os.environ, CARGO_REGISTRY_TOKEN=token)
        rc = subprocess.call(["cargo", "publish", "--workspace", "--locked", *argv], env=env)
    finally:
        try:
            call("the revocation of the token", "DELETE", TOKENS, {"Authorization": f"Bearer {token}"})
        except Refused as e:
            print(f"crates-io-publish: {e}; the token ends in 30 minutes", file=sys.stderr)
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
