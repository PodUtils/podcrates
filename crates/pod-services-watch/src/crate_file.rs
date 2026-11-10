//! `crate-file`: a source file of the newest release of a crate of
//! crates.io holds the hosts of endpoints (iroh's default relays).

use std::io::Read;

use pod_services::{Service, Watch};

use crate::args::Args;
use crate::{Checked, Finding, Net, Outcome};

/// `crate-file`: the file `path` of the newest stable release of `crate`
/// holds the host of each endpoint (of `endpoint` or `role`).
pub async fn check(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (name, path) = (args.text("crate")?, args.text("path")?);
    let about = net.json(&format!("{}/api/v1/crates/{name}", net.bases.crates_api)).await?;
    let version = ["max_stable_version", "newest_version", "max_version"]
        .iter()
        .find_map(|k| about.pointer(&format!("/crate/{k}")).and_then(|v| v.as_str()))
        .ok_or_else(|| Finding::failed(format!("crates.io names no version of {name}")))?;
    let url = format!("{}/crates/{name}/{name}-{version}.crate", net.bases.crates_static);
    let archive = net.get_ok(&url, "application/gzip").await?.body;
    let inside = format!("{name}-{version}/{path}");
    let text = member(&archive, &inside).map_err(|e| Finding::failed(format!("{name} {version}: {e}")))?;
    let mut out = Outcome::default();
    for endpoint in args.endpoints()? {
        let host = endpoint.host().trim_end_matches('.');
        if text.contains(host) {
            out.saw(format!("{name} {version} {path} holds {host}"));
        } else {
            out.found(Finding::moved(
                format!("endpoint {}", endpoint.name()),
                host,
                format!("no {host:?} in {name} {version} {path}"),
            ));
        }
    }
    Ok(out)
}

/// The text of the member `inside` of a `.crate` archive (tar in gzip).
pub fn member(archive: &[u8], inside: &str) -> Result<String, String> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let entries = tar.entries().map_err(|e| format!("the archive: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("the archive: {e}"))?;
        let path = entry.path().map_err(|e| format!("a path of the archive: {e}"))?;
        if path.to_string_lossy() == inside {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| format!("{inside}: {e}"))?;
            return Ok(text);
        }
    }
    Err(format!("the archive has no {inside}"))
}
