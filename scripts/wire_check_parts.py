"""The layer, the grants and the versions of scripts/check-wire-vectors.py
(T-120), from docs/wire.md alone: sections 5, 6 and 8 of that page."""

from __future__ import annotations

import base64
import hashlib
import hmac
import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import wire_ed25519 as ed  # noqa: E402


class Refused(Exception):
    """Bytes that the page refuses; its text is the vector's error name."""


# --- section 5: the layer's records -------------------------------------

NAMES = ["GREETING", "OPEN", "ACCEPT", "PROOF", "REFUSE", "DATA", "ACK", "PING", "PONG", "CLOSE", "RETIRE", "END"]
TYPE_OF = {name: i + 1 for i, name in enumerate(NAMES)}
MAGIC = b"podssh-session"
FEATURE = re.compile(rb"^[a-z0-9._-]{1,32}$")


def u64(n: int) -> bytes:
    return struct.pack(">Q", n)


def encode_record(name: str, f: dict) -> bytes:
    def hello() -> bytes:
        return MAGIC + bytes([f["version"], f["role"]]) + bytes.fromhex(f["nonce"])

    def features() -> bytes:
        out = bytes([len(f["features"])])
        for n in f["features"]:
            out += bytes([len(n)]) + n.encode()
        return out

    if name == "GREETING":
        body = hello() + features()
    elif name == "OPEN":
        what = b"\x00" + bytes.fromhex(f["public"]) if f["opening"] == "new" else b"\x01" + bytes.fromhex(f["id"])
        body = hello() + what + features()
    elif name == "ACCEPT" and f["acceptance"] == "new":
        body = b"\x00" + bytes.fromhex(f["id"]) + bytes.fromhex(f["public"])
    elif name == "ACCEPT":
        body = b"\x01" + u64(f["offset"]) + bytes.fromhex(f["proof"])
    elif name == "PROOF":
        body = u64(f["offset"]) + bytes.fromhex(f["proof"])
    elif name == "REFUSE":
        body = bytes([f["code"]]) + f["reason"].encode()
    elif name == "DATA":
        body = u64(f["offset"]) + bytes.fromhex(f["bytes"])
    elif name in ("ACK", "END"):
        body = u64(f["offset"])
    elif name == "PING":
        body = u64(f["value"])
    elif name == "PONG":
        body = u64(f["value"]) + u64(f["offset"])
    elif name == "CLOSE":
        body = f["reason"].encode()
    else:
        body = b""
    return bytes([TYPE_OF[name]]) + struct.pack(">I", len(body)) + body


class Body:
    """A cursor over one body; each short read is malformed."""

    def __init__(self, raw: bytes):
        self.raw = raw

    def take(self, n: int) -> bytes:
        if len(self.raw) < n:
            raise Refused("malformed")
        out, self.raw = self.raw[:n], self.raw[n:]
        return out

    def u64(self) -> int:
        return struct.unpack(">Q", self.take(8))[0]

    def hexed(self, n: int) -> str:
        return self.take(n).hex()

    def reason(self) -> str:
        if len(self.raw) > 1024:
            raise Refused("malformed")
        try:
            text = self.take(len(self.raw)).decode("utf-8")
        except UnicodeDecodeError:
            raise Refused("malformed") from None
        return text

    def hello(self) -> dict:
        if self.take(14) != MAGIC:
            raise Refused("malformed")
        version, role = self.take(1)[0], self.take(1)[0]
        return {"version": version, "role": role, "nonce": self.hexed(32)}

    def features(self) -> list[str]:
        count = self.take(1)[0]
        if count > 16:
            raise Refused("malformed")
        names = []
        for _ in range(count):
            name = self.take(self.take(1)[0])
            if not FEATURE.match(name):
                raise Refused("malformed")
            names.append(name.decode())
        return names


def decode_record(raw: bytes) -> tuple[str, dict] | None:
    """The one record of `raw`; None while more bytes are due."""
    if not raw:
        return None
    if not 0x01 <= raw[0] <= 0x0C:
        raise Refused("unknown-type")
    if len(raw) < 5:
        return None
    length = struct.unpack(">I", raw[1:5])[0]
    if length > 65536:
        raise Refused("too-long")
    if len(raw) < 5 + length:
        return None
    if len(raw) > 5 + length:
        raise Refused("bytes after the record")
    name, b, f = NAMES[raw[0] - 1], Body(raw[5:]), {}
    if name in ("GREETING", "OPEN"):
        f = b.hello()
        if name == "OPEN":
            how = b.take(1)[0]
            if how == 0:
                f.update(opening="new", public=b.hexed(32))
            elif how == 1:
                f.update(opening="resume", id=b.hexed(16))
            else:
                raise Refused("malformed")
        f["features"] = b.features()
    elif name == "ACCEPT":
        how = b.take(1)[0]
        if how == 0:
            f = {"acceptance": "new", "id": b.hexed(16), "public": b.hexed(32)}
        elif how == 1:
            f = {"acceptance": "resume", "offset": b.u64(), "proof": b.hexed(32)}
        else:
            raise Refused("malformed")
    elif name == "PROOF":
        f = {"offset": b.u64(), "proof": b.hexed(32)}
    elif name == "REFUSE":
        f = {"code": b.take(1)[0], "reason": b.reason()}
    elif name == "DATA":
        f = {"offset": b.u64(), "bytes": b.take(len(b.raw)).hex()}
    elif name in ("ACK", "END"):
        f = {"offset": b.u64()}
    elif name == "PING":
        f = {"value": b.u64()}
    elif name == "PONG":
        f = {"value": b.u64(), "offset": b.u64()}
    elif name == "CLOSE":
        f = {"reason": b.reason()}
    if b.raw:
        raise Refused("malformed")
    return name, f


def secret_of(f: dict) -> bytes:
    salt = bytes.fromhex(f["far_nonce"]) + bytes.fromhex(f["client_nonce"])
    prk = hmac.new(salt, bytes.fromhex(f["shared"]), hashlib.sha256).digest()
    info = b"podssh-session v1 secret\x00" + b"".join(
        bytes.fromhex(f[k]) for k in ("id", "client_public", "far_public"))
    return hmac.new(prk, info + b"\x01", hashlib.sha256).digest()


def proof_of(secret: bytes, label: bytes, f: dict, offset: int) -> str:
    message = label + b"\x00" + b"".join(bytes.fromhex(f[k]) for k in ("id", "far_nonce", "client_nonce"))
    return hmac.new(secret, message + u64(offset), hashlib.sha256).hexdigest()


def check_layer(v: dict) -> list[str]:
    kind = v["kind"]
    if kind == "secret":
        secret = secret_of(v["fields"])
        problems = [] if secret.hex() == v["secret"] else ["the page derives another secret"]
        for p in v["proofs"]:
            for side, label in (("client", b"podssh-session v1 client proof"), ("far", b"podssh-session v1 far proof")):
                if proof_of(secret, label, v["fields"], p["offset"]) != p[side]:
                    problems.append(f"the {side} proof at offset {p['offset']} is not the page's")
        return problems
    raw = bytes.fromhex(v["hex"])
    try:
        got = decode_record(raw)
    except Refused as why:
        if kind == "refused" and str(why) == v["error"]:
            return []
        return [f"refused as {why}"]
    if kind == "partial":
        return [] if got is None else ["a whole record, where the page waits for more"]
    if kind == "refused":
        return [f"read, where the page refuses it as {v['error']}"]
    problems = [] if encode_record(v["record"], v["fields"]) == raw else ["its fields do not encode to its bytes"]
    if got != (v["record"], v["fields"]):
        problems.append(f"its bytes decode to {got}")
    # Byte by byte: no record before its last byte.
    if any(decode_record(raw[:n]) is not None for n in range(len(raw))):
        problems.append("a record before its last byte")
    return problems


# --- section 6: grants --------------------------------------------------

CERT = b"ssh-ed25519-cert-v01@openssh.com"


def string(raw: bytes) -> bytes:
    return struct.pack(">I", len(raw)) + raw


def key_string(key: bytes) -> bytes:
    return string(b"ssh-ed25519") + string(key)


def fingerprint(key: bytes) -> str:
    return "SHA256:" + base64.b64encode(hashlib.sha256(key_string(key)).digest()).rstrip(b"=").decode()


def signed_part(v: dict) -> bytes:
    out = string(CERT) + string(bytes.fromhex(v["nonce"])) + string(bytes.fromhex(v["key"]))
    out += u64(v["serial"]) + struct.pack(">I", 1) + string(v["key_id"].encode())
    out += string(b"".join(string(p.encode()) for p in v["principals"]))
    out += u64(v["valid_after"]) + u64(v["valid_before"])
    options = b"".join(string(n.encode()) + string(bytes.fromhex(d)) for n, d in sorted(v["critical"]))
    return out + string(options) + string(b"") + string(b"") + string(key_string(bytes.fromhex(v["issuer"])))


def read_grant(raw: bytes) -> tuple[dict, bytes, bytes]:
    """The fields, the signed bytes and the signature of a certificate."""
    b = Body(raw)

    def s() -> bytes:
        return b.take(struct.unpack(">I", b.take(4))[0])

    def strings(blob: bytes) -> list[bytes]:
        inner, out = Body(blob), []
        while inner.raw:
            out.append(inner.take(struct.unpack(">I", inner.take(4))[0]))
        return out

    if s() != CERT:
        raise Refused("type")
    f = {"nonce": s().hex(), "key": s().hex(), "serial": b.u64(), "type": struct.unpack(">I", b.take(4))[0],
         "key_id": s().decode(), "principals": [p.decode() for p in strings(s())],
         "valid_after": b.u64(), "valid_before": b.u64()}
    opts = strings(s())
    f["critical"] = [[opts[i].decode(), opts[i + 1].hex()] for i in range(0, len(opts), 2)]
    f["extensions"], f["reserved"] = s(), s()
    issuer = strings(s())
    signed = raw[: len(raw) - len(b.raw)]
    sig = strings(s())
    if b.raw or issuer[:1] != [b"ssh-ed25519"] or len(issuer) != 2 or sig[:1] != [b"ssh-ed25519"] or len(sig) != 2:
        raise Refused("form")
    f["issuer"] = issuer[1].hex()
    return f, signed, sig[1]


BEARER = b"pod-bearer/1\n"
BINDING_LABEL = b"pod-e2e grant binding\0"


def read_bearer(raw: bytes) -> dict:
    """The fields of a bearer; the signed bytes are each byte before `sig`."""
    if len(raw) != len(BEARER) + 208 or not raw.startswith(BEARER):
        raise Refused("form")
    b = Body(raw[len(BEARER):])
    return {"signer": b.take(32), "holder": b.take(32), "iat": b.u64(), "exp": b.u64(), "nonce": b.take(32),
            "jti": b.take(32), "sig": b.take(64), "signed": raw[:-64]}


def bearer_refusal(raw: bytes, binding: bytes) -> str | None:
    """The first check of section 6 that the bearer fails on the channel of
    `binding`, with a signer that the node takes and at its `iat`."""
    try:
        f = read_bearer(raw)
    except Refused:
        return "form"
    if not ed.verify(f["signer"], f["signed"], f["sig"]):
        return "signature"
    if not 0 < f["exp"] - f["iat"] <= 30:
        return "life"
    if not hmac.compare_digest(hmac.new(binding, f["nonce"], hashlib.sha256).digest(), f["jti"]):
        return "binding"
    return None


def check_bearer(v: dict) -> list[str]:
    raw = bytes.fromhex(v["hex"])
    if v["kind"] == "bearer-refused":
        got = bearer_refusal(raw, bytes.fromhex(v["binding"]))
        return [] if got == v["reason"] else [f"the page refuses it as {got}, the vector says {v['reason']}"]
    problems = []
    signer_seed, holder_seed = bytes.fromhex(v["signer_seed"]), bytes.fromhex(v["holder_seed"])
    if ed.public(signer_seed).hex() != v["signer"] or ed.public(holder_seed).hex() != v["holder"]:
        problems.append("a key is not the key of its seed")
    binding = hashlib.sha256(BINDING_LABEL + bytes.fromhex(v["handshake_hash"])).digest()
    if binding.hex() != v["binding"]:
        problems.append("the binding is not the page's hash of the handshake hash")
    nonce = bytes.fromhex(v["nonce"])
    jti = hmac.new(binding, nonce, hashlib.sha256).digest()
    if jti.hex() != v["jti"]:
        problems.append("the jti is not the HMAC of the nonce under the binding")
    signed = (BEARER + bytes.fromhex(v["signer"]) + bytes.fromhex(v["holder"]) + u64(v["iat"]) + u64(v["exp"])
              + nonce + jti)
    if signed + ed.sign(signer_seed, signed) != raw:
        problems.append("its fields do not encode to its bytes")
    if v["exp"] != v["iat"] + 30:
        problems.append("a bearer of the vectors lives 30 s")
    if bearer_refusal(raw, binding) is not None:
        problems.append(f"the page refuses it as {bearer_refusal(raw, binding)}")
    return problems


def check_grants(v: dict) -> list[str]:
    if v["kind"].startswith("bearer"):
        return check_bearer(v)
    raw = bytes.fromhex(v["hex"])
    if v["kind"] == "grant-refused":
        try:
            read_grant(raw)
        except Refused:
            return []
        return ["a certificate that the page refuses was read"]
    if v["kind"] == "grant-forged":
        f, signed, sig = read_grant(raw)
        ok = f["serial"] == v["serial"] and not ed.verify(bytes.fromhex(f["issuer"]), signed, sig)
        return [] if ok else ["its serial or its failed signature is not the page's"]
    problems = []
    if ed.public(bytes.fromhex(v["issuer_seed"])).hex() != v["issuer"]:
        problems.append("the issuer is not the key of its seed")
    if ed.public(bytes.fromhex(v["key_seed"])).hex() != v["key"]:
        problems.append("the key is not the key of its seed")
    node = ed.public(bytes.fromhex(v["node_seed"]))
    principal = "node:" + fingerprint(node) + (f"/{v['route']}" if v["route"] else "")
    if v["principals"] != [principal]:
        problems.append(f"the page names the node {principal}")
    signed = signed_part(v)
    sig = ed.sign(bytes.fromhex(v["issuer_seed"]), signed)
    if signed + string(key_string(sig)) != raw:
        problems.append("its fields do not encode to its bytes")
    if hashlib.sha256(signed).hexdigest() != v["digest"]:
        problems.append("its digest is not the page's")
    f, read_signed, read_sig = read_grant(raw)
    names = ("nonce", "key", "serial", "key_id", "principals", "valid_after", "valid_before", "critical", "issuer")
    if any(f[k] != v[k] for k in names) or f["type"] != 1 or f["extensions"] or f["reserved"]:
        problems.append("its bytes decode to other fields")
    if not ed.verify(bytes.fromhex(v["issuer"]), read_signed, read_sig):
        problems.append("its signature fails")
    if ["once@podssh", ""] not in v["critical"]:
        problems.append("a grant has the critical option once@podssh")
    return problems


# --- section 8: versions ------------------------------------------------

def page_versions(page: str) -> tuple[int | None, list[tuple[int, str]]]:
    """The page's version line, and each row of its table of versions."""
    line = re.search(r"^Wire version: (\d+)$", page, re.M)
    rows = re.findall(r"^\| (\d+) \| \d{4}-\d{2}-\d{2} \| ([0-9a-f]{64}|[A-Z]+) \|$", page, re.M)
    return (int(line.group(1)) if line else None), [(int(n), d) for n, d in rows]


def digest_of(folder: Path, parts) -> str:
    """SHA-256 of each file's name, a newline, and its bytes with no CR."""
    h = hashlib.sha256()
    for part in sorted(parts):
        name = f"{part}.json"
        h.update(name.encode() + b"\n" + (folder / name).read_bytes().replace(b"\r", b""))
    return h.hexdigest()
