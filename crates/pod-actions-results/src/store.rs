//! The artifacts of a run (T-119): the ArtifactService of the results API
//! that `actions/upload-artifact@v4` and `actions/download-artifact@v4`
//! call at `ACTIONS_RESULTS_URL` (twirp JSON), and the blob routes of the
//! signed URLs (the block blob calls of the Azure client). One store serves
//! each broker of a run, so a job reads the artifact of an earlier job. The
//! store is outside each unit: a unit writes it only through the broker.
//!
//! The store keeps `DIR/NAME/artifact.zip` (the zip of the client, as
//! GitHub gives it) and `DIR/NAME/artifact.json`. A name is unique in a
//! run, as on GitHub. The finalize checks the size and the SHA-256 that the
//! client sends. The bytes of a run have a limit. A wrapper of protobuf
//! (`google.protobuf.StringValue`) is a plain string in its JSON; a request
//! is read in both forms.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const SERVICE: &str = "/twirp/github.actions.results.api.v1.ArtifactService/";
const BLOBS: &str = "/_artifacts/";

/// An upload that is not finalized.
struct Pending {
    name: String,
    sig: String,
}

#[derive(Default)]
struct Inner {
    next: u64,
    pending: BTreeMap<u64, Pending>,
    /// The bytes of the run: each block and each blob.
    used: u64,
}

/// The artifact store of one run.
pub struct ArtifactStore {
    dir: PathBuf,
    limit: u64,
    inner: Mutex<Inner>,
}

/// The default limit of the bytes of one run.
pub const LIMIT: u64 = 4 << 30;

fn twirp(status: u16, code: &str, msg: String) -> (u16, Vec<u8>) {
    (status, json!({ "code": code, "msg": msg }).to_string().into_bytes())
}

fn ok(v: Value) -> (u16, Vec<u8>) {
    (200, v.to_string().into_bytes())
}

/// A name of an artifact: the characters that the client refuses, and no
/// name that is a path, are refused here too.
fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.chars().any(|c| c.is_control() || "\":<>|*?\\/".contains(c))
}

fn random_hex() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, bytes);
    format!("sha256:{}", d.as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// RFC 3339 in UTC, from seconds of the Unix epoch.
fn rfc3339(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // The civil date of a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The value of `key` in a query string, percent-decoded.
fn query<'a>(q: &'a str, key: &str) -> Option<String> {
    let v: &'a str = q.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))?;
    let mut out = Vec::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())?;
                out.push(h);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The block ids of a block list, in order.
fn block_list(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if matches!(tag, "Latest" | "Committed" | "Uncommitted") {
            let Some(close) = rest.find("</") else { break };
            out.push(rest[..close].trim().to_string());
            rest = &rest[close..];
        }
    }
    out
}

impl ArtifactStore {
    /// A store in `dir` with a limit of `limit` bytes for the run.
    pub fn new(dir: &Path, limit: u64) -> std::io::Result<ArtifactStore> {
        fs::create_dir_all(dir.join(".pending"))?;
        Ok(ArtifactStore { dir: dir.to_path_buf(), limit, inner: Mutex::default() })
    }

    fn meta(&self, name: &str) -> Option<Value> {
        let text = fs::read_to_string(self.dir.join(name).join("artifact.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Each artifact of the run, by name.
    pub fn list(&self) -> Vec<Value> {
        let mut out: Vec<Value> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| self.meta(&e.file_name().to_string_lossy()))
            .collect();
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    fn pending_dir(&self, id: u64) -> PathBuf {
        self.dir.join(".pending").join(id.to_string())
    }

    /// The answer to a request of the service or of a blob, or `None` for
    /// another route. `path` keeps its query.
    pub fn route(&self, method: &str, path: &str, body: &[u8], base: &str) -> Option<(u16, Vec<u8>)> {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        if let Some(call) = p.strip_prefix(SERVICE) {
            if method != "POST" {
                return Some((405, Vec::new()));
            }
            let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            return Some(self.call(call, &v, base));
        }
        let rest = p.strip_prefix(BLOBS)?;
        let (kind, id) = rest.split_once('/')?;
        let id: u64 = id.parse().ok()?;
        let sig = query(q, "sig").unwrap_or_default();
        Some(match (method, kind) {
            ("PUT", "upload") => self.put(id, &sig, q, body),
            ("GET", "download") => self.download(id, &sig),
            _ => (404, Vec::new()),
        })
    }

    fn call(&self, call: &str, v: &Value, base: &str) -> (u16, Vec<u8>) {
        let name = v["name"].as_str().unwrap_or("");
        match call {
            "CreateArtifact" => {
                if !valid(name) {
                    return twirp(400, "invalid_argument", format!("the artifact name {name:?} is not valid"));
                }
                if self.meta(name).is_some() {
                    let msg = format!("an artifact with the name {name:?} already exists on the workflow run");
                    return twirp(409, "already_exists", msg);
                }
                let mut st = self.inner.lock().expect("the store");
                st.pending.retain(|_, p| p.name != name);
                st.next += 1;
                let id = st.next;
                let sig = random_hex();
                let _ = fs::remove_dir_all(self.pending_dir(id));
                if let Err(e) = fs::create_dir_all(self.pending_dir(id).join("blocks")) {
                    return twirp(500, "internal", e.to_string());
                }
                st.pending.insert(id, Pending { name: name.to_string(), sig: sig.clone() });
                ok(json!({ "ok": true, "signed_upload_url": format!("{base}_artifacts/upload/{id}?sig={sig}") }))
            }
            "FinalizeArtifact" => self.finalize(name, v),
            "ListArtifacts" => {
                let by_name = v["name_filter"]["value"].as_str().or(v["name_filter"].as_str());
                let by_id = v["id_filter"]["value"].as_str().or(v["id_filter"].as_str()).map(str::to_string);
                let list: Vec<Value> = self
                    .list()
                    .into_iter()
                    .filter(|a| by_name.is_none_or(|n| a["name"] == n))
                    .filter(|a| by_id.as_ref().is_none_or(|i| a["database_id"] == i.as_str()))
                    .map(|mut a| {
                        // The signature of the download stays in the store.
                        a.as_object_mut().map(|o| o.remove("sig"));
                        a
                    })
                    .collect();
                ok(json!({ "artifacts": list }))
            }
            "GetSignedArtifactURL" => match self.meta(name) {
                Some(m) => {
                    let id = m["database_id"].as_str().unwrap_or("0");
                    let sig = m["sig"].as_str().unwrap_or("");
                    ok(json!({ "signed_url": format!("{base}_artifacts/download/{id}?sig={sig}") }))
                }
                None => twirp(404, "not_found", format!("no artifact {name:?} in the run")),
            },
            "DeleteArtifact" => match self.meta(name) {
                Some(m) => {
                    let _ = fs::remove_dir_all(self.dir.join(name));
                    ok(json!({ "ok": true, "artifact_id": m["database_id"] }))
                }
                None => twirp(404, "not_found", format!("no artifact {name:?} in the run")),
            },
            _ => twirp(404, "bad_route", format!("no method {call}")),
        }
    }

    /// A block, a block list, or a whole blob of an upload.
    fn put(&self, id: u64, sig: &str, q: &str, body: &[u8]) -> (u16, Vec<u8>) {
        let mut st = self.inner.lock().expect("the store");
        if st.pending.get(&id).is_none_or(|p| p.sig != sig) {
            return (403, Vec::new());
        }
        let dir = self.pending_dir(id);
        let comp = query(q, "comp").unwrap_or_default();
        if comp != "blocklist" {
            if st.used + body.len() as u64 > self.limit {
                return (413, format!("the artifacts of the run are over {} bytes", self.limit).into_bytes());
            }
            st.used += body.len() as u64;
        }
        let r = match comp.as_str() {
            "block" => {
                let Some(block) = query(q, "blockid") else { return (400, Vec::new()) };
                let file: String = block.bytes().map(|b| format!("{b:02x}")).collect();
                fs::write(dir.join("blocks").join(file), body)
            }
            "blocklist" => {
                let ids = block_list(&String::from_utf8_lossy(body));
                let mut all = Vec::new();
                for block in ids {
                    let file: String = block.bytes().map(|b| format!("{b:02x}")).collect();
                    match fs::read(dir.join("blocks").join(file)) {
                        Ok(b) => all.extend_from_slice(&b),
                        Err(_) => return (400, format!("no block {block}").into_bytes()),
                    }
                }
                fs::write(dir.join("blob"), all)
            }
            "" => fs::write(dir.join("blob"), body),
            _ => return (400, Vec::new()),
        };
        match r {
            Ok(()) => (201, Vec::new()),
            Err(e) => (500, e.to_string().into_bytes()),
        }
    }

    fn finalize(&self, name: &str, v: &Value) -> (u16, Vec<u8>) {
        let mut st = self.inner.lock().expect("the store");
        let Some((&id, _)) = st.pending.iter().find(|(_, p)| p.name == name) else {
            return twirp(404, "not_found", format!("no upload of {name:?}"));
        };
        let dir = self.pending_dir(id);
        let Ok(blob) = fs::read(dir.join("blob")) else {
            return twirp(400, "failed_precondition", format!("the upload of {name:?} has no blob"));
        };
        let size = v["size"].as_str().and_then(|s| s.parse().ok()).or(v["size"].as_u64());
        if size != Some(blob.len() as u64) {
            return twirp(400, "invalid_argument", format!("{name:?}: size {size:?}, the blob has {}", blob.len()));
        }
        let digest = sha256(&blob);
        if let Some(h) = v["hash"]["value"].as_str().or(v["hash"].as_str())
            && h != digest
        {
            return twirp(400, "invalid_argument", format!("{name:?}: hash {h}, the blob has {digest}"));
        }
        let pending = st.pending.remove(&id).expect("a pending upload");
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let meta = json!({
            "workflow_run_backend_id": v["workflow_run_backend_id"], "workflow_job_run_backend_id": v["workflow_job_run_backend_id"],
            "database_id": id.to_string(), "name": name, "size": blob.len().to_string(),
            "created_at": rfc3339(now), "digest": digest, "sig": pending.sig,
        });
        let out = self.dir.join(name);
        let r = fs::create_dir_all(&out)
            .and_then(|_| fs::rename(dir.join("blob"), out.join("artifact.zip")))
            .and_then(|_| fs::write(out.join("artifact.json"), meta.to_string()));
        let _ = fs::remove_dir_all(&dir);
        match r {
            Ok(()) => ok(json!({ "ok": true, "artifact_id": id.to_string() })),
            Err(e) => twirp(500, "internal", e.to_string()),
        }
    }

    fn download(&self, id: u64, sig: &str) -> (u16, Vec<u8>) {
        let found = self.list().into_iter().find(|m| m["database_id"] == id.to_string().as_str());
        match found {
            Some(m) if m["sig"] == sig => {
                match fs::read(self.dir.join(m["name"].as_str().unwrap_or("")).join("artifact.zip")) {
                    Ok(b) => (200, b),
                    Err(_) => (404, Vec::new()),
                }
            }
            Some(_) => (403, Vec::new()),
            None => (404, Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests;
