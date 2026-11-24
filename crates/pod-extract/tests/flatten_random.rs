//! `flatten` against `extract` on random layer sets: a small alphabet of
//! names, files, directories, symlinks, hard links (also to a path of an
//! older layer), whiteouts and opaque markers. Both
//! refuse the same sets, and on each other set the tree of the stream is the
//! tree of the layers. The generator has a seed: a failure names its seed.
#![cfg(target_os = "linux")]

mod support;

use support::{E, Scratch, extracted, flattened, plain, same, tar};

/// The cases of one run, and the first seed.
const CASES: u64 = 300;
const SEED: u64 = 0x5eed_f1a7;

/// A linear congruential generator (Knuth's MMIX constants): the test
/// needs a repeatable sequence, not a good one.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

const NAMES: [&str; 3] = ["a", "b", "c"];
const TARGETS: [&str; 6] = ["a", "b/c", "/a/b", "../a", "c", "missing"];
const MODES: [u32; 4] = [0o644, 0o600, 0o755, 0o700];

/// One entry of a generated layer, owned.
#[derive(Debug, Clone)]
enum Op {
    File(String, Vec<u8>, u32),
    Dir(String, u32),
    Symlink(String, String),
    Hardlink(String, String),
    Whiteout(String),
    Opaque(String),
}

fn path(r: &mut Lcg) -> String {
    let depth = 1 + r.below(3);
    (0..depth).map(|_| NAMES[r.below(NAMES.len())]).collect::<Vec<_>>().join("/")
}

/// One layer: each path at most once, as `flatten` writes a path again in
/// the same layer in the order of the layer, and no file or symlink over a
/// path that the layer wrote below before, which `flatten` refuses (each a
/// test of its own).
///
/// With `links`, a hard link names a file of this layer or of an older one
/// (`files`) seven times in eight, else a random path.
fn layer(r: &mut Lcg, files: &mut Vec<String>, links: bool) -> Vec<Op> {
    let mut seen = std::collections::BTreeSet::new();
    let mut written: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for _ in 0..r.below(9) {
        let p = path(r);
        let (dir, base) = match p.rsplit_once('/') {
            Some((d, b)) => (format!("{d}/"), b.to_string()),
            None => (String::new(), p.clone()),
        };
        let op = match r.below(if links { 22 } else { 20 }) {
            0..=7 => Op::File(p, vec![b'0' + r.below(10) as u8; r.below(4)], MODES[r.below(MODES.len())]),
            8..=12 => Op::Dir(p, MODES[r.below(MODES.len())]),
            13..=14 => Op::Symlink(p, TARGETS[r.below(TARGETS.len())].to_string()),
            15..=18 => Op::Whiteout(format!("{dir}.wh.{base}")),
            19..=20 if links && !files.is_empty() && r.below(8) > 0 => {
                Op::Hardlink(p, files[r.below(files.len())].clone())
            }
            19..=20 if links => Op::Hardlink(p, path(r)),
            _ => Op::Opaque(format!("{dir}.wh..wh..opq")),
        };
        let key = match &op {
            Op::File(p, ..)
            | Op::Dir(p, _)
            | Op::Symlink(p, _)
            | Op::Hardlink(p, _)
            | Op::Whiteout(p)
            | Op::Opaque(p) => p.clone(),
        };
        if let Op::File(p, ..) | Op::Symlink(p, _) | Op::Hardlink(p, _) = &op
            && written.iter().any(|w| w.starts_with(&format!("{p}/")))
        {
            continue;
        }
        if seen.insert(key.clone()) {
            if let Op::File(p, ..) = &op {
                files.push(p.clone());
            }
            written.push(key);
            out.push(op);
        }
    }
    out
}

fn build(ops: &[Op]) -> Vec<u8> {
    let entries: Vec<E> = ops
        .iter()
        .map(|op| match op {
            Op::File(p, d, m) => E::File(p, d, *m),
            Op::Dir(p, m) => E::Dir(p, *m),
            Op::Symlink(p, t) => E::Symlink(p, t),
            Op::Hardlink(p, t) => E::Hardlink(p, t),
            Op::Whiteout(p) | Op::Opaque(p) => E::File(p, b"", 0o644),
        })
        .collect();
    tar(&entries)
}

#[test]
fn random_layer_sets_give_the_tree_of_extract() {
    run(SEED, false);
}

/// The same with hard links: to a file of the same layer before or after
/// the link, of an older layer, to a link, to a path that a later layer
/// replaced, removed or never made.
#[test]
fn random_layer_sets_with_hard_links_give_the_tree_of_extract() {
    run(SEED ^ 0x11, true);
}

fn run(first: u64, links: bool) {
    let (mut agreed, mut refused, mut differ) = (0, 0, 0);
    for case in 0..CASES {
        let seed = first.wrapping_add(case);
        let mut r = Lcg(seed);
        let mut files = Vec::new();
        let ops: Vec<Vec<Op>> = (0..1 + r.below(4)).map(|_| layer(&mut r, &mut files, links)).collect();
        let s = Scratch::new("random");
        let layers = plain(s.path(), &ops.iter().map(|l| build(l)).collect::<Vec<_>>());
        let want = extracted(&layers);
        let got = flattened(&layers);
        match (&want, &got) {
            (Ok(_), Ok(_)) => {
                let result = std::panic::catch_unwind(|| same(&layers));
                assert!(result.is_ok(), "seed {seed:#x}: the trees differ for the layers {ops:#?}");
                agreed += 1;
            }
            (Err(a), Err(b)) => {
                // Each is a refusal: a failure of a system call of `extract`
                // would be a defect of `extract`. `extract` reads from the
                // oldest layer and `flatten` from the newest, so a set with
                // two faults and a hard link that waits for the end of its
                // layer can give two kinds: then the layers up to the layer
                // that each names refuse on the other side too.
                let (ra, rb) = (pod_extract::refused(a), pod_extract::refused(b));
                assert!(ra.is_some() && rb.is_some(), "seed {seed:#x}: {} | {}", a.chain(), b.chain());
                let (ra, rb) = (ra.unwrap(), rb.unwrap());
                if std::mem::discriminant(&ra.refusal) != std::mem::discriminant(&rb.refusal) {
                    let at = |r: &pod_extract::Refused| layers.iter().rposition(|l| l.digest == r.layer).unwrap();
                    let (e, f) = (at(ra), at(rb));
                    let both = extracted(&layers[..=f]).is_err() && flattened(&layers[..=e]).is_err();
                    assert!(links && both, "seed {seed:#x}: {} | {}", a.chain(), b.chain());
                    differ += 1;
                }
                refused += 1;
            }
            (Ok(_), Err(f)) => panic!("seed {seed:#x}: flatten refused what extract took ({}): {ops:#?}", f.chain()),
            (Err(f), Ok(_)) => panic!("seed {seed:#x}: extract refused what flatten took ({}): {ops:#?}", f.chain()),
        }
    }
    // The generator must reach both sides, or the test proves less than
    // its name says. A link names a path that the small alphabet often gives
    // to a directory or removes, so fewer sets with links agree.
    let least = if links { CASES / 3 } else { CASES / 2 };
    assert!(agreed > least && refused > differ, "{agreed} agreed, {refused} refused, {differ} of two kinds");
}
