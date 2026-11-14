//! The twirp JSON calls of `github.actions.results.api.v1.ArtifactService`.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::store::{ArtifactStore, Pending, upload_dir};
use crate::wire::{Answer, hex, int64, ok, plain, rfc3339, sha256_file, twirp};

fn bad_name(name: &str) -> Answer {
    twirp(400, "invalid_argument", format!("the artifact name {name:?} is not valid"))
}

fn no_artifact(name: &str) -> Answer {
    twirp(404, "not_found", format!("no artifact {name:?} in the run"))
}

fn exists(name: &str) -> Answer {
    twirp(409, "already_exists", format!("an artifact with the name {name:?} already exists on the workflow run"))
}

impl ArtifactStore {
    pub(crate) fn call(&self, call: &str, v: &Value, base: &str) -> Answer {
        let name = v["name"].as_str().unwrap_or("");
        match call {
            "CreateArtifact" => self.create(name, base),
            "FinalizeArtifact" => self.finalize(name, v),
            "ListArtifacts" => self.list_call(v),
            "GetSignedArtifactURL" => {
                if self.at(name).is_none() {
                    return bad_name(name);
                }
                match self.meta(name) {
                    Some(m) => {
                        let id = m["database_id"].as_str().unwrap_or("0");
                        let sig = m["sig"].as_str().unwrap_or("");
                        ok(json!({ "signed_url": format!("{base}{}download/{id}?sig={sig}", &crate::BLOBS[1..]) }))
                    }
                    None => no_artifact(name),
                }
            }
            "DeleteArtifact" => {
                let Some(at) = self.at(name) else { return bad_name(name) };
                let _st = self.lock();
                match self.meta(name) {
                    Some(m) => match fs::remove_dir_all(&at) {
                        Ok(()) => ok(json!({ "ok": true, "artifact_id": m["database_id"] })),
                        Err(e) => twirp(500, "internal", e.to_string()),
                    },
                    None => no_artifact(name),
                }
            }
            _ => twirp(404, "bad_route", format!("no method {call}")),
        }
    }

    fn create(&self, name: &str, base: &str) -> Answer {
        let Some(at) = self.at(name) else { return bad_name(name) };
        let mut st = self.lock();
        if self.meta(name).is_some() {
            return exists(name);
        }
        // A new upload of a name drops the earlier one that was not
        // finalized, with its blocks.
        let old: Vec<u64> = st.pending.iter().filter(|(_, p)| p.name == name).map(|(id, _)| *id).collect();
        for id in old {
            st.pending.remove(&id);
            let _ = fs::remove_dir_all(upload_dir(&at, id));
        }
        let mut b = [0u8; 16];
        if let Err(e) = st.entropy.fill(&mut b) {
            return twirp(500, "internal", e.to_string());
        }
        let sig = hex(&b);
        st.next += 1;
        let id = st.next;
        let up = upload_dir(&at, id);
        let _ = fs::remove_dir_all(&up);
        if let Err(e) = fs::create_dir_all(up.join("blocks")) {
            return twirp(500, "internal", e.to_string());
        }
        st.pending.insert(id, Pending { name: name.to_string(), sig: sig.clone() });
        let url = format!("{base}{}upload/{id}?sig={sig}", &crate::BLOBS[1..]);
        ok(json!({ "ok": true, "signed_upload_url": url }))
    }

    fn list_call(&self, v: &Value) -> Answer {
        let by_name = plain(&v["name_filter"]).as_str();
        let by_id = int64(plain(&v["id_filter"])).map(|i| i.to_string());
        let list: Vec<Value> = self
            .list()
            .into_iter()
            .filter(|a| by_name.is_none_or(|n| a["name"] == n))
            .filter(|a| by_id.as_ref().is_none_or(|i| a["database_id"] == i.as_str()))
            .collect();
        ok(json!({ "artifacts": list }))
    }

    fn finalize(&self, name: &str, v: &Value) -> Answer {
        let Some(at) = self.at(name) else { return bad_name(name) };
        let mut st = self.lock();
        let Some(id) = st.pending.iter().find(|(_, p)| p.name == name).map(|(id, _)| *id) else {
            return twirp(404, "not_found", format!("no upload of {name:?}"));
        };
        if self.meta(name).is_some() {
            return exists(name);
        }
        let up = upload_dir(&at, id);
        let Ok((len, digest)) = sha256_file(&up.join("blob")) else {
            return twirp(412, "failed_precondition", format!("the upload of {name:?} has no blob"));
        };
        let size = int64(&v["size"]);
        if size != Some(len) {
            return twirp(400, "invalid_argument", format!("{name:?}: size {size:?}, the blob has {len}"));
        }
        if let Some(h) = plain(&v["hash"]).as_str()
            && h != digest
        {
            return twirp(400, "invalid_argument", format!("{name:?}: hash {h}, the blob has {digest}"));
        }
        let Some(pending) = st.pending.remove(&id) else {
            return twirp(404, "not_found", format!("no upload of {name:?}"));
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        // `digest` is a `google.protobuf.StringValue`: download-artifact@v4
        // refuses it in any form but a plain string.
        let meta = json!({
            "workflow_run_backend_id": v["workflow_run_backend_id"],
            "workflow_job_run_backend_id": v["workflow_job_run_backend_id"],
            "database_id": id.to_string(), "name": name, "size": len.to_string(),
            "created_at": rfc3339(now), "digest": digest, "sig": pending.sig,
        });
        // The record goes in last, by a rename: an artifact with a record
        // has its zip.
        let r = fs::write(up.join("artifact.json"), meta.to_string())
            .and_then(|()| fs::rename(up.join("blob"), at.join("artifact.zip")))
            .and_then(|()| fs::rename(up.join("artifact.json"), at.join("artifact.json")));
        let _ = fs::remove_dir_all(&up);
        match r {
            Ok(()) => ok(json!({ "ok": true, "artifact_id": id.to_string() })),
            Err(e) => twirp(500, "internal", e.to_string()),
        }
    }
}
