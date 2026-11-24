//! The helpers of the tests of `flatten`: crafted layers as blobs with
//! their digests, the tree of `extract`, and the comparison of `flatten`
//! with `extract` of the same layers.
//!
//! The comparison extracts the stream of `flatten` as one layer: the same
//! code then reads both sides, and a difference is a difference of the
//! trees, not of two readers.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_extract::flatten::{Flattened, Options, flatten};
use pod_extract::{Layer, Place, Settings, extract};
use sha2::{Digest, Sha256};

/// A directory of one test, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("pod-flatten-{tag}-{}-{n}", std::process::id()));
        let _ = pod_extract::remove_tree(&p);
        std::fs::create_dir_all(&p).expect("a scratch directory");
        Scratch(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = pod_extract::remove_tree(&self.0);
    }
}

/// One entry of a crafted layer.
pub enum E<'a> {
    File(&'a str, &'a [u8], u32),
    /// A file with its owner ids and mode.
    Owned(&'a str, &'a [u8], u64, u64, u32),
    Dir(&'a str, u32),
    Symlink(&'a str, &'a str),
    Hardlink(&'a str, &'a str),
    /// A file whose name goes into the header past the checks of the
    /// writer (`..`).
    Raw(&'a str, &'a [u8]),
    Fifo(&'a str),
    /// A GNU sparse file of the PAX form: the version (`"0.0"`, `"0.1"`,
    /// `"1.0"`), the name, the real size, and each region with its data.
    Sparse(&'a str, &'a str, u64, &'a [(u64, &'a [u8])]),
}

/// One uncompressed layer.
pub fn tar(entries: &[E]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for e in entries {
        let mut h = tar::Header::new_gnu();
        h.set_mtime(1_700_000_000);
        match e {
            E::File(p, data, mode) => file(&mut b, &mut h, p, data, 0, 0, *mode),
            E::Owned(p, data, uid, gid, mode) => file(&mut b, &mut h, p, data, *uid, *gid, *mode),
            E::Dir(p, mode) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(*mode);
                h.set_size(0);
                b.append_data(&mut h, p, std::io::empty()).unwrap();
            }
            E::Symlink(p, t) | E::Hardlink(p, t) => {
                let soft = matches!(e, E::Symlink(..));
                h.set_entry_type(if soft { tar::EntryType::Symlink } else { tar::EntryType::Link });
                h.set_mode(if soft { 0o777 } else { 0o644 });
                h.set_size(0);
                b.append_link(&mut h, p, t).unwrap();
            }
            E::Raw(p, data) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o644);
                h.set_size(data.len() as u64);
                let name = &mut h.as_gnu_mut().expect("a GNU header").name;
                name[..p.len()].copy_from_slice(p.as_bytes());
                h.set_cksum();
                b.append(&h, Cursor::new(*data)).unwrap();
            }
            E::Fifo(p) => {
                h.set_entry_type(tar::EntryType::Fifo);
                h.set_mode(0o644);
                h.set_size(0);
                b.append_data(&mut h, p, std::io::empty()).unwrap();
            }
            E::Sparse(version, name, real, regions) => sparse(&mut b, version, name, *real, regions),
        }
    }
    b.into_inner().unwrap()
}

fn file(b: &mut tar::Builder<Vec<u8>>, h: &mut tar::Header, p: &str, data: &[u8], uid: u64, gid: u64, mode: u32) {
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(mode);
    h.set_uid(uid);
    h.set_gid(gid);
    h.set_size(data.len() as u64);
    b.append_data(h, p, Cursor::new(data)).unwrap();
}

/// One PAX record, its length with its own digits.
fn record(out: &mut Vec<u8>, key: &str, value: &str) {
    let rest = key.len() + value.len() + 3;
    let mut len = rest + 1;
    while len != rest + len.to_string().len() {
        len = rest + len.to_string().len();
    }
    out.extend_from_slice(format!("{len} {key}={value}\n").as_bytes());
}

fn sparse(b: &mut tar::Builder<Vec<u8>>, version: &str, name: &str, real: u64, regions: &[(u64, &[u8])]) {
    let mut recs = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let count = regions.len().to_string();
    let mut member = name.to_string();
    match version {
        "0.0" => {
            record(&mut recs, "GNU.sparse.size", &real.to_string());
            record(&mut recs, "GNU.sparse.numblocks", &count);
            for (off, d) in regions {
                record(&mut recs, "GNU.sparse.offset", &off.to_string());
                record(&mut recs, "GNU.sparse.numbytes", &d.len().to_string());
            }
        }
        "0.1" => {
            let map: Vec<String> = regions.iter().map(|(o, d)| format!("{o},{}", d.len())).collect();
            record(&mut recs, "GNU.sparse.size", &real.to_string());
            record(&mut recs, "GNU.sparse.numblocks", &count);
            record(&mut recs, "GNU.sparse.map", &map.join(","));
            record(&mut recs, "GNU.sparse.name", name);
            member = format!("GNUSparseFile.0/{name}");
        }
        _ => {
            record(&mut recs, "GNU.sparse.major", "1");
            record(&mut recs, "GNU.sparse.minor", "0");
            record(&mut recs, "GNU.sparse.name", name);
            record(&mut recs, "GNU.sparse.realsize", &real.to_string());
            member = format!("GNUSparseFile.0/{name}");
            data.extend_from_slice(format!("{count}\n").as_bytes());
            for (off, d) in regions {
                data.extend_from_slice(format!("{off}\n{}\n", d.len()).as_bytes());
            }
            data.resize(data.len().div_ceil(512) * 512, 0);
        }
    }
    for (_, d) in regions {
        data.extend_from_slice(d);
    }
    let mut x = tar::Header::new_ustar();
    x.set_entry_type(tar::EntryType::XHeader);
    x.set_size(recs.len() as u64);
    b.append_data(&mut x, "././@PaxHeader", Cursor::new(recs)).unwrap();
    let mut h = tar::Header::new_ustar();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(0o644);
    h.set_size(data.len() as u64);
    b.append_data(&mut h, member, Cursor::new(data)).unwrap();
}

/// How a blob is packed, with its media type.
#[derive(Clone, Copy, Debug)]
pub enum Pack {
    Plain,
    Gzip,
    /// Two gzip members in one blob.
    Gzip2,
    Zstd,
}

/// Write each layer as a blob under `dir`, with its digest.
pub fn blobs(dir: &Path, layers: &[(Vec<u8>, Pack)]) -> Vec<Layer> {
    let mut out = Vec::new();
    for (n, (tar, pack)) in layers.iter().enumerate() {
        let gz = |part: &[u8]| {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(part).unwrap();
            e.finish().unwrap()
        };
        let (bytes, media) = match pack {
            Pack::Plain => (tar.clone(), "application/vnd.oci.image.layer.v1.tar"),
            Pack::Gzip => (gz(tar), "application/vnd.oci.image.layer.v1.tar+gzip"),
            Pack::Gzip2 => {
                let cut = (tar.len() / 1024) * 512;
                ([gz(&tar[..cut]), gz(&tar[cut..])].concat(), "application/vnd.oci.image.layer.v1.tar+gzip")
            }
            Pack::Zstd => (
                ruzstd::encoding::compress_to_vec(&tar[..], ruzstd::encoding::CompressionLevel::Fastest),
                "application/vnd.oci.image.layer.v1.tar+zstd",
            ),
        };
        let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        let blob = dir.join(format!("blob-{n}"));
        std::fs::write(&blob, &bytes).unwrap();
        out.push(Layer { digest: format!("sha256:{hex}"), media_type: media.into(), size: bytes.len() as u64, blob });
    }
    out
}

/// Plain blobs of uncompressed layers.
pub fn plain(dir: &Path, tars: &[Vec<u8>]) -> Vec<Layer> {
    let packed: Vec<(Vec<u8>, Pack)> = tars.iter().map(|t| (t.clone(), Pack::Plain)).collect();
    blobs(dir, &packed)
}

/// One path of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    File { mode: u32, data: Vec<u8> },
    Dir { mode: u32 },
    Symlink { target: String },
}

/// A tree: each path with its node, the ids of its last sidecar row, and
/// the names of each inode with more than one name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    pub nodes: BTreeMap<String, Node>,
    pub owners: BTreeMap<String, (u64, u64)>,
    pub links: BTreeSet<Vec<String>>,
}

/// The tree of a directory, each path relative to it.
pub fn walk(root: &Path) -> BTreeMap<String, Node> {
    let mut out = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            let md = std::fs::symlink_metadata(&p).unwrap();
            let mode = md.permissions().mode() & 0o7777;
            let node = if md.file_type().is_symlink() {
                Node::Symlink { target: std::fs::read_link(&p).unwrap().to_string_lossy().into_owned() }
            } else if md.is_dir() {
                todo.push(p.clone());
                Node::Dir { mode }
            } else {
                Node::File { mode, data: std::fs::read(&p).unwrap() }
            };
            out.insert(rel, node);
        }
    }
    out
}

/// The names of each inode of a directory that has more than one name,
/// each list sorted.
pub fn inodes(root: &Path) -> BTreeSet<Vec<String>> {
    use std::os::unix::fs::MetadataExt;
    let mut by: BTreeMap<(u64, u64), Vec<String>> = BTreeMap::new();
    for path in walk(root).keys() {
        let md = std::fs::symlink_metadata(root.join(path)).unwrap();
        if !md.is_dir() && md.nlink() > 1 {
            by.entry((md.dev(), md.ino())).or_default().push(path.clone());
        }
    }
    by.into_values().filter(|v| v.len() > 1).collect()
}

/// The ids of the last sidecar row of each path.
fn owners(sidecar: &Path) -> BTreeMap<String, (u64, u64)> {
    let text = std::fs::read_to_string(sidecar).unwrap_or_default();
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let field = |key: &str| line.split(&format!("\"{key}\":")).nth(1).map(str::to_string);
        let path = field("path").and_then(|v| v.split('"').nth(1).map(str::to_string));
        let id = |key: &str| field(key).and_then(|v| v.split([',', '}']).next()?.parse::<u64>().ok());
        if let (Some(p), Some(u), Some(g)) = (path, id("uid"), id("gid")) {
            out.insert(p, (u, g));
        }
    }
    out
}

/// The tree of `extract` of `layers`, in a new directory.
pub fn extracted(layers: &[Layer]) -> pod_extract::Result<Tree> {
    let s = Scratch::new("tree");
    let place = Place::under(&s.path().join("out"));
    extract(&Settings::default(), "test", layers, &place)?;
    Ok(Tree { nodes: walk(&place.rootfs), owners: owners(&place.sidecar), links: inodes(&place.rootfs) })
}

/// The stream of `flatten` of `layers`.
pub fn flattened(layers: &[Layer]) -> pod_extract::Result<(Vec<u8>, Flattened)> {
    let mut out = Vec::new();
    let f = flatten(layers, &mut out, &Options::default())?;
    Ok((out, f))
}

/// One entry of a stream: path, kind, mode, uid, gid, link target.
pub type Entry = (String, tar::EntryType, u32, u64, u64, Option<String>);

/// Each entry of a stream as the `tar` reader gives it.
pub fn entries(stream: &[u8]) -> Vec<Entry> {
    let mut ar = tar::Archive::new(Cursor::new(stream));
    let mut out = Vec::new();
    for e in ar.entries().unwrap() {
        let e = e.unwrap();
        let h = e.header();
        let path = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        let link = e.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned());
        out.push((path, h.entry_type(), h.mode().unwrap(), h.uid().unwrap(), h.gid().unwrap(), link));
    }
    out
}

/// `flatten` and `extract` of the same layers agree: the tree of the
/// stream (extracted as one layer) is the tree of the layers, and the ids
/// of each entry of the stream are those of its last sidecar row. Each
/// path is in the stream once. Gives the stream.
pub fn same(layers: &[Layer]) -> Vec<u8> {
    let want = extracted(layers).unwrap_or_else(|f| panic!("extract refused the layers: {}", f.chain()));
    let (stream, _) = flattened(layers).unwrap_or_else(|f| panic!("flatten refused the layers: {}", f.chain()));
    let s = Scratch::new("stream");
    let got = extracted(&plain(s.path(), std::slice::from_ref(&stream))).expect("the stream extracts");
    assert_eq!(got.nodes, want.nodes, "the tree of the stream differs from the tree of the layers");
    assert_eq!(got.links, want.links, "the names of the inodes of the stream differ from those of the layers");
    let listed = entries(&stream);
    let mut seen = std::collections::BTreeSet::new();
    for (path, kind, mode, uid, gid, _) in &listed {
        assert!(seen.insert(path.clone()), "{path} is in the stream two times: {listed:?}");
        // A directory that a file below it made has no row: the stream
        // gives it the mode and the owner that `extract` gives it.
        let ids = want.owners.get(path).copied();
        let implied = (*kind, *mode, *uid, *gid) == (tar::EntryType::Directory, 0o755, 0, 0);
        assert!(ids == Some((*uid, *gid)) || ids.is_none() && implied, "the ids of {path}: {ids:?}");
    }
    stream
}

/// The [`pod_extract::Refused`] cause of a failure; a success or another
/// failure panics.
pub fn refusal(r: pod_extract::Result<impl std::fmt::Debug>) -> pod_extract::Refused {
    match r {
        Err(f) => pod_extract::refused(&f).cloned().unwrap_or_else(|| panic!("not a refusal: {}", f.chain())),
        Ok(v) => panic!("not refused: {v:?}"),
    }
}

/// The paths of a stream, in their order.
pub fn paths(stream: &[u8]) -> Vec<String> {
    entries(stream).into_iter().map(|e| e.0).collect()
}

/// Unpack `stream` into `out` with GNU tar and the arguments of incusd
/// (`lxc/incus:shared/archive/archive.go`, `Unpack`). `-p` keeps the modes
/// as a user that is not root, as root does. `Err` names a host with no
/// GNU tar.
pub fn gnu_tar(stream: &[u8], out: &Path) -> Result<std::process::ExitStatus, String> {
    pod_testkit::host::tool("tar")?;
    std::fs::create_dir_all(out).unwrap();
    let mut child = std::process::Command::new("tar")
        .args(["--anchored", "--wildcards", "--exclude=dev/*", "--restrict", "--force-local", "-C"])
        .arg(out)
        .args(["--numeric-owner", "--xattrs-include=*", "-p", "-xf", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(stream).unwrap();
    drop(child.stdin.take());
    Ok(child.wait().unwrap())
}
