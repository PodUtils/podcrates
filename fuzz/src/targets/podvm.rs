//! The fuzz targets of the parsers of untrusted input that podvm reads
//! through podcrates (T-173, PodUtils/podvm#241): the manifest and the
//! challenge of a registry, a layer of an image, the ELF header of a
//! payload, and the squashfs modloop of a guest kernel (T-206,
//! PodUtils/podvm#260).

use std::sync::atomic::{AtomicU64, Ordering};

use pod_oci::spec::{Fetched, MEDIA_DOCKER_LIST, MEDIA_DOCKER_MANIFEST, MEDIA_OCI_INDEX, MEDIA_OCI_MANIFEST};

/// The `Content-Type` of the answer that the first byte of an input picks.
const HEADERS: [Option<&str>; 6] = [
    None,
    Some(MEDIA_OCI_INDEX),
    Some(MEDIA_OCI_MANIFEST),
    Some(MEDIA_DOCKER_LIST),
    Some(MEDIA_DOCKER_MANIFEST),
    Some("application/json"),
];

/// A manifest or an index of a registry (`pod_oci::spec::Fetched::parse`,
/// from podvm's `podvm-oci`): the bytes that a registry sends, and the
/// `Content-Type` of the answer, picked by the first byte. The document's
/// own `mediaType` decides over the header: a document that names an index
/// type is never read as a manifest, and the reverse. The digest of each
/// descriptor reads with no panic.
pub fn oci_manifest(data: &[u8]) {
    let Some((&pick, bytes)) = data.split_first() else { return };
    let header = HEADERS[usize::from(pick) % HEADERS.len()];
    let Ok(fetched) = Fetched::parse(bytes, header) else { return };
    let named = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("mediaType").and_then(|m| m.as_str()).map(str::to_string));
    let descriptors = match &fetched {
        Fetched::Index(index) => {
            let named = named.as_deref();
            assert!(named != Some(MEDIA_OCI_MANIFEST) && named != Some(MEDIA_DOCKER_MANIFEST), "{named:?}");
            index.manifests.iter().collect::<Vec<_>>()
        }
        Fetched::Manifest(manifest) => {
            let named = named.as_deref();
            assert!(named != Some(MEDIA_OCI_INDEX) && named != Some(MEDIA_DOCKER_LIST), "{named:?}");
            std::iter::once(&manifest.config).chain(&manifest.layers).collect()
        }
    };
    for d in descriptors {
        let _ = d.parsed_digest();
    }
}

/// The `WWW-Authenticate` challenge of a registry
/// (`pod_oci::registry::fuzz_parse_challenge`, from podvm's `podvm-oci`):
/// each key is trimmed and not empty, and a challenge with no space has no
/// parameter.
pub fn oci_challenge(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let params = pod_oci::registry::fuzz_parse_challenge(&text);
    if !text.contains(char::is_whitespace) {
        assert!(params.is_empty(), "{text:?}: {params:?}");
    }
    for key in params.keys() {
        assert!(!key.is_empty() && key.trim() == key, "{key:?} of {text:?}");
    }
    assert!(params.len() <= text.matches(',').count() + 1, "{text:?}: {params:?}");
}

/// A fresh name for the directory of one run of `extract_layer`.
static RUN: AtomicU64 = AtomicU64::new(0);

/// The depth of the root in the directory of a run: an entry that climbs
/// out of the root by up to this many `..` lands in the run, where the
/// check sees it.
const DEPTH: [&str; 4] = ["d", "d/d", "d/d/d", "d/d/d/d"];

/// A layer of an image (`pod_extract::apply::apply_layer`, after
/// `remove::collect` and `remove::apply`, from podvm's `podvm-extract`): a
/// tar that an image author controls, plain, gzip or zstd (the first byte
/// picks). The layer goes into a fresh directory, which the run removes.
/// No entry escapes the root: after the layer, the directory of the run
/// holds the blob, the sidecar and the directories above the root, and
/// nothing more.
pub fn extract_layer(data: &[u8]) {
    let Some((_, seen)) = layer_run(data, |_| ()) else { return };
    assert_eq!(seen, ["blob", "d", "d/d", "d/d/d", "d/d/d/d", "d/d/d/d/root"], "an entry escaped the root");
}

/// One run of `extract_layer`: whether the crate applied the layer, and
/// each path of the run out of the root, sorted, with no sidecar. `look`
/// reads the root before the removal. `None` for an empty input.
fn layer_run(data: &[u8], look: impl FnOnce(&std::path::Path)) -> Option<(bool, Vec<String>)> {
    use pod_extract::apply::{Ids, apply_layer};
    use pod_extract::layer::{Compression, open_hashed};
    use pod_extract::{remove, safety::Dir, sidecar::Sidecar};

    let (&pick, bytes) = data.split_first()?;
    let compression = [Compression::None, Compression::Gzip, Compression::Zstd][usize::from(pick) % 3];
    let n = RUN.fetch_add(1, Ordering::Relaxed);
    let run = std::env::temp_dir().join(format!("podcrates-fuzz-layer-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&run);
    let root_path = run.join(DEPTH[DEPTH.len() - 1]).join("root");
    std::fs::create_dir_all(&root_path).expect("the root of a run");
    let blob = run.join("blob");
    std::fs::write(&blob, bytes).expect("the blob of a run");
    let digest = "sha256:fuzz";
    // A refusal of the crate is a correct answer: the check runs after
    // each one.
    let applied = (|| -> Option<()> {
        let root = Dir::open(&root_path.to_string_lossy()).ok()?;
        let (mut first, _) = open_hashed(&blob, compression).ok()?;
        let ops = remove::collect(digest, &mut first).ok()?;
        remove::apply(&root, &ops).ok()?;
        let (mut second, _) = open_hashed(&blob, compression).ok()?;
        let mut sidecar = Sidecar::create(&run.join("meta.jsonl")).ok()?;
        apply_layer(&root, digest, &mut second, Ids::current(), &mut sidecar).ok()?;
        Some(())
    })()
    .is_some();
    let mut seen = Vec::new();
    for at in [""].iter().chain(&DEPTH) {
        for e in std::fs::read_dir(run.join(at)).expect("a directory of a run") {
            let name = e.expect("an entry").file_name().to_string_lossy().into_owned();
            seen.push(if at.is_empty() { name } else { format!("{at}/{name}") });
        }
    }
    seen.retain(|n| n != "meta.jsonl");
    seen.sort();
    look(&root_path);
    open_up(&run);
    let _ = std::fs::remove_dir_all(&run);
    Some((applied, seen))
}

/// Give the owner each right on each directory under `dir`, so that the
/// removal can enter a directory of mode 0 that a layer made. A symbolic
/// link is not followed: its target can be out of the run.
fn open_up(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) {
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
            open_up(&path);
        }
    }
}

/// The ELF header and the program headers of a payload
/// (`pod_enter::abi::Elf::parse`, from podvm's `podvm-enter`): each name
/// that the parse gives (the interpreter, each needed library, the soname,
/// each section) holds no NUL, and a parse of the same bytes gives the
/// same answer.
pub fn elf_header(data: &[u8]) {
    let first = pod_enter::abi::Elf::parse("fuzz", data);
    let again = pod_enter::abi::Elf::parse("fuzz", data);
    let Ok(elf) = first else {
        assert!(again.is_err());
        return;
    };
    let again = again.expect("the same bytes parse again");
    assert_eq!(format!("{elf:?}"), format!("{again:?}"));
    let names = elf.interp.iter().chain(&elf.needed).chain(&elf.soname).chain(&elf.sections);
    for name in names {
        assert!(!name.contains('\0'), "{name:?}");
    }
}

/// The release and the names of the module set of `squashfs_image`: the
/// seed `modloop` holds them.
const RELEASE: &str = "6.1.0-0-test";
const NAMES: [&str; 3] = ["a", "wire-guard", "code"];
/// The most inodes of one walk of `squashfs_image`.
const WALK: usize = 256;

/// A squashfs image (`pod_guest::squashfs::Image`, T-206), as a modloop
/// that a mirror serves: the superblock and the compressor options, the
/// inode, directory, fragment, id and export tables, and the blocks (xz
/// with and without the x86 filter, stored, sparse). Each call gives a
/// value or a fault: a file that reads has its size, and a second walk of
/// the same bytes gives the same answer. The module set of the release
/// holds each module once, with one line of its `modules.dep` for each.
pub fn squashfs_image(data: &[u8]) {
    let first = squashfs_walk(data);
    assert_eq!(first, squashfs_walk(data));
    if let Ok(set) = pod_guest::modloop::modules(std::io::Cursor::new(data), RELEASE, NAMES) {
        let mut paths: Vec<&str> = set.modules.iter().map(|m| m.path.as_str()).collect();
        let n = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), n, "a module twice");
        assert_eq!(set.dep.lines().count(), n, "{}", set.dep);
    }
}

/// Each inode that a walk of `data` reaches, up to [`WALK`]: its path and
/// the size of its bytes, or the text of its fault.
fn squashfs_walk(data: &[u8]) -> Vec<String> {
    use pod_guest::squashfs::{Image, Kind};
    let mut out = Vec::new();
    let mut image = match Image::open(std::io::Cursor::new(data)) {
        Ok(image) => image,
        Err(e) => return vec![e.to_string()],
    };
    out.push(format!("{:?}", image.ids()));
    let mut todo = match image.root() {
        Ok(root) => vec![(String::new(), root)],
        Err(e) => return vec![e.to_string()],
    };
    // A bound on the inodes, not on the lines: a listing can name its own
    // directory, and a directory adds no line.
    for _ in 0..WALK {
        let Some((path, inode)) = todo.pop() else { break };
        match inode.kind {
            Kind::Directory(d) => match image.entries(&d) {
                Ok(entries) => {
                    for e in entries {
                        let name = format!("{path}/{}", String::from_utf8_lossy(&e.name));
                        match image.inode(e.inode) {
                            Ok(i) => todo.push((name, i)),
                            Err(f) => out.push(format!("{name}: {f}")),
                        }
                    }
                }
                Err(f) => out.push(format!("{path}: {f}")),
            },
            Kind::File(f) => match image.read(&f) {
                Ok(bytes) => {
                    assert_eq!(bytes.len() as u64, f.size(), "{path}");
                    out.push(format!("{path}: {} bytes", bytes.len()));
                }
                Err(e) => out.push(format!("{path}: {e}")),
            },
            other => out.push(format!("{path}: {other:?}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(target: &str, name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds").join(target).join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn a_challenge_keeps_a_comma_in_quotes() {
        let params = pod_oci::registry::fuzz_parse_challenge(
            r#"Bearer realm="https://auth.example/token",service="registry.example",scope="repository:a/b:pull,push""#,
        );
        assert_eq!(params["scope"], "repository:a/b:pull,push");
        oci_challenge(br#"Bearer realm="r",scope="a,b""#);
    }

    /// The seeds reach past the first check of each parser: the documents
    /// parse as their type says, the layers apply in each compression, and
    /// the two programs parse.
    #[test]
    fn the_seeds_of_the_podvm_targets_read() {
        let index = |name: &str| {
            let data = seed("oci_manifest", name);
            Fetched::parse(&data[1..], HEADERS[usize::from(data[0]) % HEADERS.len()])
                .map(|f| matches!(f, Fetched::Index(_)))
        };
        for (name, is_index) in [
            ("oci-index", true),
            ("oci-manifest", false),
            ("docker-list-by-header", true),
            ("docker-manifest-by-header", false),
            ("index-named-but-header-says-manifest", true),
        ] {
            assert_eq!(index(name).ok(), Some(is_index), "{name}");
        }
        assert!(index("schema-1").is_err());
        for name in ["tar", "gzip", "zstd-raw"] {
            let mut file = None;
            let (applied, _) = layer_run(&seed("extract_layer", name), |root| {
                file = std::fs::read_to_string(root.join("etc/hostname")).ok();
            })
            .unwrap();
            assert!(applied, "{name}: the layer did not apply");
            assert_eq!(file.as_deref(), Some("seed\n"), "{name}");
        }
        for name in ["escape-by-dots", "escape-by-link", "absolute"] {
            extract_layer(&seed("extract_layer", name));
        }
        let parse = |name: &str| pod_enter::abi::Elf::parse(name, &seed("elf_header", name));
        assert!(parse("static").expect("the static program").interp.is_none());
        assert!(parse("dynamic").expect("the dynamic program").interp.is_some_and(|i| i.contains("ld-")));
        assert!(parse("not-elf").is_err());
    }

    /// A listing that names its own directory: the walk ends at its bound
    /// of inodes, with no line of a file.
    #[test]
    fn a_directory_that_lists_itself_ends_the_walk() {
        let mut img = seed("squashfs_image", "options");
        let root = u64::from_le_bytes(img[32..40].try_into().unwrap());
        // The one listing: a header (count, block, number), then the entry
        // (offset, delta, type, name size, name) of `code.ko`.
        let listing = u64::from_le_bytes(img[72..80].try_into().unwrap()) as usize + 2;
        img[listing + 4..listing + 8].copy_from_slice(&((root >> 16) as u32).to_le_bytes());
        img[listing + 12..listing + 14].copy_from_slice(&((root & 0xffff) as u16).to_le_bytes());
        assert_eq!(squashfs_walk(&img), ["Ok([0])"]);
        squashfs_image(&img);
    }

    /// The seeds of `squashfs_image` reach past the superblock: each image
    /// walks with no fault, and the seed `modloop` gives its module set.
    #[test]
    fn the_seeds_of_the_squashfs_target_read() {
        for name in ["modloop", "tree", "options"] {
            let walk = squashfs_walk(&seed("squashfs_image", name));
            assert!(walk.iter().any(|l| l.ends_with(" bytes")), "{name}: {walk:?}");
            assert!(!walk.iter().any(|l| l.contains("squashfs:")), "{name}: {walk:?}");
        }
        let modloop = seed("squashfs_image", "modloop");
        let set = pod_guest::modloop::modules(std::io::Cursor::new(&modloop), RELEASE, NAMES).expect("the module set");
        let paths: Vec<&str> = set.modules.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "kernel/lib/c.ko",
                "kernel/lib/b.ko",
                "kernel/net/a.ko",
                "kernel/drivers/net/wire-guard.ko",
                "kernel/arch/x86/code.ko"
            ]
        );
    }
}
