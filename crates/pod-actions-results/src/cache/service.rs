//! The twirp JSON calls of `github.actions.results.api.v1.CacheService`.

use std::fs;

use serde_json::{Value, json};

use super::{BLOBS, CacheStore, KEY_MAX, Pending, now_nanos, order, refused};
use crate::wire::{Answer, int64, ok, rfc3339, sha256_file, twirp};

impl CacheStore {
    pub(super) fn call(&self, call: &str, v: &Value, base: &str) -> Answer {
        let key = v["key"].as_str().unwrap_or("");
        let version = v["version"].as_str().unwrap_or("");
        match call {
            "CreateCacheEntry" => self.create(key, version, base),
            "FinalizeCacheEntryUpload" => self.finalize(key, version, v),
            "GetCacheEntryDownloadURL" => {
                let restore: Vec<&str> = v["restore_keys"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                match self.lookup(key, &restore, version) {
                    Some(e) => {
                        let (id, sig) = (e["id"].as_str().unwrap_or(""), e["sig"].as_str().unwrap_or(""));
                        ok(json!({
                            "ok": true, "matched_key": e["key"],
                            "signed_download_url": format!("{base}{}download/{id}?sig={sig}", &BLOBS[1..]),
                        }))
                    }
                    None => ok(json!({ "ok": false, "signed_download_url": "", "matched_key": "" })),
                }
            }
            _ => twirp(404, "bad_route", format!("no method {call}")),
        }
    }

    fn create(&self, key: &str, version: &str, base: &str) -> Answer {
        if key.is_empty() || key.len() > KEY_MAX || key.contains(',') || version.is_empty() || version.len() > KEY_MAX {
            return refused(format!("the key {key:?} or its version is not valid"));
        }
        let mut st = self.lock();
        if self.entry_dir(key, version).join("entry.json").is_file() {
            return refused(format!("a cache entry of the key {key:?} and its version exists"));
        }
        // A new upload of a key and a version drops the earlier one, with
        // its blocks.
        let old: Vec<u64> =
            st.pending.iter().filter(|(_, p)| p.key == key && p.version == version).map(|(id, _)| *id).collect();
        for id in old {
            if let Some(p) = st.pending.remove(&id) {
                let _ = fs::remove_dir_all(self.dir.join(".pending").join(&p.dir));
            }
        }
        let (sig, dir) = match (self.random(&mut st, 16), self.random(&mut st, 8)) {
            (Ok(sig), Ok(dir)) => (sig, dir),
            (Err(e), _) | (_, Err(e)) => return refused(format!("no random bytes: {e}")),
        };
        if fs::create_dir_all(self.dir.join(".pending").join(&dir).join("blocks")).is_err() {
            return refused("the store cannot write");
        }
        st.next += 1;
        let id = st.next;
        let pending = Pending { key: key.into(), version: version.into(), sig: sig.clone(), dir, bytes: 0 };
        st.pending.insert(id, pending);
        ok(json!({ "ok": true, "signed_upload_url": format!("{base}{}upload/{id}?sig={sig}", &BLOBS[1..]) }))
    }

    fn finalize(&self, key: &str, version: &str, v: &Value) -> Answer {
        let mut st = self.lock();
        let found = st.pending.iter().find(|(_, p)| p.key == key && p.version == version).map(|(i, _)| *i);
        let Some(id) = found else {
            return refused(format!("no upload of the key {key:?}"));
        };
        let up = self.dir.join(".pending").join(&st.pending[&id].dir);
        let Ok((len, digest)) = sha256_file(&up.join("blob")) else {
            return refused(format!("the upload of {key:?} has no blob"));
        };
        let size = int64(&v["size_bytes"]);
        if size != Some(len) {
            return refused(format!("{key:?}: size {size:?}, the blob has {len}"));
        }
        let out = self.entry_dir(key, version);
        if out.join("entry.json").is_file() {
            return refused(format!("a cache entry of the key {key:?} and its version exists"));
        }
        // Random: two runs over one store must not give one id. The id is
        // an int64 of protobuf: decimal digits, never hex.
        let entry_id = match self.random(&mut st, 8).map(|h| u64::from_str_radix(&h, 16)) {
            Ok(Ok(n)) => (n & i64::MAX as u64).to_string(),
            Ok(Err(e)) => return refused(e.to_string()),
            Err(e) => return refused(format!("no random bytes: {e}")),
        };
        let Some(pending) = st.pending.remove(&id) else {
            return refused(format!("no upload of the key {key:?}"));
        };
        // Strictly above the newest entry: "newest first" holds for two
        // entries in one tick of the clock.
        let newest = self.entries().first().map_or(0, order);
        let now = now_nanos().max(newest.saturating_add(1));
        let secs = now / 1_000_000_000;
        let meta = json!({
            "id": entry_id, "key": key, "version": version, "scope": self.scopes[0], "size": len,
            "created": secs, "order": now, "created_at": rfc3339(secs), "digest": digest, "sig": pending.sig,
        });
        // The record goes in last, by a rename: an entry with a record has
        // its blob.
        let r = fs::create_dir_all(&out)
            .and_then(|()| fs::write(up.join("entry.json"), meta.to_string()))
            .and_then(|()| fs::rename(up.join("blob"), out.join("blob")))
            .and_then(|()| fs::rename(up.join("entry.json"), out.join("entry.json")));
        let _ = fs::remove_dir_all(&up);
        if let Err(e) = r {
            return refused(e.to_string());
        }
        drop(st);
        self.evict();
        ok(json!({ "ok": true, "entry_id": entry_id }))
    }
}
