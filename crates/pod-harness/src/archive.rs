//! The relocatable archive: the fixtures, the models, the simulator, the
//! expected scores and a `run.sh`, in one zip.
//!
//! The archive holds no repository path and no toolchain. `run.sh` runs a
//! prebuilt `pod-harness` binary that the caller gives it, and names a skip
//! when there is none.

use crate::error::{Error, Result};
use crate::fixture::{Fixtures, Run};
use crate::matrix::{Outcome, Profile, Row};
use crate::zip::Zip;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

/// The script that a host without a toolchain runs.
pub const RUN_SH: &str = r#"#!/bin/sh
# The harness of the archive. No repository and no toolchain are needed.
# usage: sh run.sh BINARY [ARG...]
# A run with no binary names a skip; it does not report a pass.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$#" -eq 0 ]; then
  echo "skip: no prebuilt pod-harness binary given"
  echo "usage: sh run.sh /path/to/pod-harness"
  echo "manifest:"
  cat "$here/manifest.txt"
  exit 0
fi
if [ "$1" = "--list" ]; then
  cat "$here/manifest.txt"
  exit 0
fi
binary=$1
shift
if [ ! -x "$binary" ]; then
  echo "skip: the binary $binary is not executable"
  exit 0
fi
exec "$binary" --archive "$here" "$@"
"#;

const README: &str = r#"# pod-harness archive

This directory is the relocatable form of one harness run. It holds the
fixtures, the classifier models, the middlebox simulator source and its
decisions, the expected scores of each row, and `run.sh`.

The archive needs no repository and no toolchain. It does run a harness
binary, and no binary is in this zip:

    sh run.sh /path/to/pod-harness

A run with no binary prints a named skip and the manifest. It does not
report a pass.

`manifest.txt` lists each file with its size and its SHA-256.

The result is the table in `expected/table.txt`, not a pass count. Each row
of the table has a file in `expected/`, with its scores or its skip. The
false-positive report of the benign cover site is at the end of the table.

`simulator/middlebox.rs` is the source of the fault model. The harness
binary runs the same model; `simulator/decisions.json` holds the decisions
of this build.
"#;

/// The archive of one run, before it is written.
#[derive(Clone, Debug)]
pub struct Archive {
    zip: Zip,
    names: BTreeSet<String>,
}

impl Archive {
    /// Build the archive from the fixtures and a finished run.
    pub fn build(fixtures: &Fixtures, run: &Run) -> Result<Archive> {
        let mut zip = Zip::new();
        let mut names = BTreeSet::new();
        let mut files = Vec::new();
        walk(&fixtures.dir, &fixtures.dir, &mut files)?;
        files.sort();
        for path in &files {
            let relative = path
                .strip_prefix(&fixtures.dir)
                .map_err(|e| Error::new(format!("the fixture path {}: {e}", path.display())))?
                .to_string_lossy()
                .replace('\\', "/");
            let data = std::fs::read(path).map_err(|e| Error::io("read the fixture", path, e))?;
            zip.add(relative.clone(), data)?;
            names.insert(relative);
        }
        // One expected file for each row, scored or skipped.
        let mut seen = BTreeSet::new();
        for row in &run.table.rows {
            let name = expected_name(&row.road, row.profile);
            if !seen.insert(name.clone()) {
                return Err(Error::new(format!("two rows have the expected name {name}")));
            }
            zip.add(name.clone(), expected_json(row))?;
            names.insert(name);
        }
        zip.add("expected/table.txt", run.table.render())?;
        names.insert("expected/table.txt".to_string());
        zip.add("expected/table.json", run.table.json())?;
        names.insert("expected/table.json".to_string());
        zip.add("simulator/middlebox.rs", include_str!("middlebox.rs"))?;
        names.insert("simulator/middlebox.rs".to_string());
        zip.add("simulator/README.md", simulator_readme())?;
        names.insert("simulator/README.md".to_string());
        zip.add("simulator/decisions.json", decisions_json(run)?)?;
        names.insert("simulator/decisions.json".to_string());
        zip.add("README.md", README)?;
        names.insert("README.md".to_string());
        zip.add("LICENSE", include_str!("../LICENSE"))?;
        names.insert("LICENSE".to_string());
        zip.add_executable("run.sh", RUN_SH)?;
        names.insert("run.sh".to_string());

        let mut manifest = String::new();
        for name in &names {
            let data = zip.get(name).ok_or_else(|| Error::new(format!("the manifest lost {name}")))?;
            manifest.push_str(&format!("{} {:>8} {}\n", sha256(data), data.len(), name));
        }
        zip.add("manifest.txt", manifest.as_bytes())?;
        Ok(Archive { zip, names })
    }

    pub fn zip(&self) -> &Zip {
        &self.zip
    }

    pub fn names(&self) -> &BTreeSet<String> {
        &self.names
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        self.zip.write(path)
    }

    pub fn open(path: &Path) -> Result<Archive> {
        let zip = Zip::read(path)?;
        let names = zip.entries().iter().map(|entry| entry.name.clone()).collect();
        Ok(Archive { zip, names })
    }

    pub fn extract(&self, dir: &Path) -> Result<()> {
        self.zip.extract(dir)
    }

    /// The manifest that a host reads after the extract.
    pub fn manifest(&self) -> Result<String> {
        let data = self.zip.get("manifest.txt").ok_or_else(|| Error::new("the archive has no manifest"))?;
        String::from_utf8(data.to_vec()).map_err(|_| Error::new("the manifest is not UTF-8"))
    }
}

/// The expected file of one row, relative to the archive root.
pub fn expected_name(road: &str, profile: Profile) -> String {
    let road: String = road
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    format!("expected/{road}-{}.json", profile.as_str())
}

fn expected_json(row: &Row) -> Vec<u8> {
    let value = match &row.outcome {
        Outcome::Scores(scores) => json!({
            "road": row.road,
            "profile": row.profile.as_str(),
            "date": row.date,
            "state": "scored",
            "scores": scores
                .iter()
                .map(|s| json!({"classifier": s.classifier, "score": s.score, "reason": s.reason}))
                .collect::<Vec<_>>(),
        }),
        Outcome::Skipped(reason) => json!({
            "road": row.road,
            "profile": row.profile.as_str(),
            "date": row.date,
            "state": "skipped",
            "reason": reason,
        }),
    };
    let mut text = serde_json::to_string_pretty(&value).expect("an expected score is JSON");
    text.push('\n');
    text.into_bytes()
}

fn decisions_json(run: &Run) -> Result<Vec<u8>> {
    let value = json!({
        "schema": "pod-harness.decisions/1",
        "cases": run
            .decisions
            .iter()
            .map(|(name, decisions)| json!({
                "case": name,
                "decisions": decisions
                    .iter()
                    .map(|d| json!({"packet": d.packet, "fault": d.fault, "detail": d.detail}))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    });
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::capture::encode_hex(&Sha256::digest(data))
}

fn simulator_readme() -> String {
    "The middlebox simulator of this archive is the source in `middlebox.rs`.\n\
     The harness binary runs the same fault model. `decisions.json` holds the\n\
     decisions that this build recorded, one case for each road and profile.\n"
        .to_string()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io("read the fixture directory", dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io("read the fixture directory", dir, e))?;
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| Error::io("read the fixture", &path, e))?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if relative.to_string_lossy().split('/').any(|part| part == "archive.zip") {
                continue;
            }
            out.push(path);
        }
    }
    Ok(())
}
