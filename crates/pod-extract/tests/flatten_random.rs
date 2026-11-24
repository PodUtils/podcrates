//! `flatten` against `extract` on random layer sets: a small alphabet of
//! names, files, directories, symlinks, whiteouts and opaque markers. Both
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
    Whiteout(String),
    Opaque(String),
}

fn path(r: &mut Lcg) -> String {
    let depth = 1 + r.below(3);
    (0..depth).map(|_| NAMES[r.below(NAMES.len())]).collect::<Vec<_>>().join("/")
}

/// One layer: each path at most once, as `flatten` writes a path again in
/// the same layer in the order of the layer (a test of its own).
fn layer(r: &mut Lcg) -> Vec<Op> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for _ in 0..r.below(9) {
        let p = path(r);
        let (dir, base) = match p.rsplit_once('/') {
            Some((d, b)) => (format!("{d}/"), b.to_string()),
            None => (String::new(), p.clone()),
        };
        let op = match r.below(20) {
            0..=7 => Op::File(p, vec![b'0' + r.below(10) as u8; r.below(4)], MODES[r.below(MODES.len())]),
            8..=12 => Op::Dir(p, MODES[r.below(MODES.len())]),
            13..=14 => Op::Symlink(p, TARGETS[r.below(TARGETS.len())].to_string()),
            15..=18 => Op::Whiteout(format!("{dir}.wh.{base}")),
            _ => Op::Opaque(format!("{dir}.wh..wh..opq")),
        };
        let key = match &op {
            Op::File(p, ..) | Op::Dir(p, _) | Op::Symlink(p, _) | Op::Whiteout(p) | Op::Opaque(p) => p.clone(),
        };
        if seen.insert(key) {
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
            Op::Whiteout(p) | Op::Opaque(p) => E::File(p, b"", 0o644),
        })
        .collect();
    tar(&entries)
}

#[test]
fn random_layer_sets_give_the_tree_of_extract() {
    let (mut agreed, mut refused) = (0, 0);
    for case in 0..CASES {
        let seed = SEED.wrapping_add(case);
        let mut r = Lcg(seed);
        let ops: Vec<Vec<Op>> = (0..1 + r.below(4)).map(|_| layer(&mut r)).collect();
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
            (Err(_), Err(_)) => refused += 1,
            (Ok(_), Err(f)) => panic!("seed {seed:#x}: flatten refused what extract took ({}): {ops:#?}", f.chain()),
            (Err(f), Ok(_)) => panic!("seed {seed:#x}: extract refused what flatten took ({}): {ops:#?}", f.chain()),
        }
    }
    // The generator must reach both sides, or the test proves less than
    // its name says.
    assert!(agreed > CASES / 2 && refused > 0, "{agreed} agreed, {refused} refused");
}
