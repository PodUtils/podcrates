//! The inclusion proof of an entry (RFC 6962, section 2.1.1; RFC 9162,
//! section 2.1.3.2): the hash of the entry's body as a leaf, its audit
//! path up to the root of the tree at its size, and the checkpoint that
//! the log signed for that size and root: a signed note
//! (C2SP `signed-note`, `tlog-checkpoint`).

use x509_parser::prelude::*;

use super::{Log, b64, chain, text};

/// The em dash and the space that start each signature line of a note.
const SIGNATURE_LINE: &str = "\u{2014} ";

/// SHA-256 of `parts`, one after the other.
fn hash(parts: &[&[u8]]) -> [u8; 32] {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for part in parts {
        h.update(part);
    }
    h.finalize().into()
}

/// The root of a tree of `size` leaves from the leaf `leaf` at `index` and
/// its audit path `path`, or `None` when the path does not fit the index
/// and the size.
pub(super) fn root(index: u64, size: u64, leaf: [u8; 32], path: &[[u8; 32]]) -> Option<[u8; 32]> {
    if index >= size {
        return None;
    }
    let (mut at, mut last, mut node) = (index, size - 1, leaf);
    for sibling in path {
        if last == 0 {
            return None;
        }
        if at & 1 == 1 || at == last {
            node = hash(&[&[1], sibling, &node]);
            // A right edge with no sibling at this level goes up as it is.
            while at & 1 == 0 && at != 0 {
                at >>= 1;
                last >>= 1;
            }
        } else {
            node = hash(&[&[1], &node, sibling]);
        }
        at >>= 1;
        last >>= 1;
    }
    (last == 0).then_some(node)
}

/// A number of the proof, which protobuf's JSON writes as a string.
fn number(v: &serde_json::Value, name: &str) -> Result<u64, String> {
    let n = v.get(name).ok_or(format!("the inclusion proof has no {name}"))?;
    n.as_u64().or_else(|| n.as_str().and_then(|s| s.parse().ok())).ok_or(format!("the proof's {name} is no number"))
}

/// A hash of 32 bytes, from base64.
fn hash_of(text: &str, what: &str) -> Result<[u8; 32], String> {
    b64(text, what)?.try_into().map_err(|_| format!("{what} is not 32 bytes"))
}

/// The inclusion proof `proof` of the entry whose body is `body`, by the
/// log `log` whose id is `id`.
pub(super) fn check(proof: &serde_json::Value, body: &[u8], log: &Log, id: &[u8; 32]) -> Result<(), String> {
    let (index, size) = (number(proof, "logIndex")?, number(proof, "treeSize")?);
    let top = hash_of(text(proof, &["rootHash"])?, "the proof's root")?;
    let path = proof["hashes"]
        .as_array()
        .ok_or("the inclusion proof has no hashes")?
        .iter()
        .map(|h| hash_of(h.as_str().unwrap_or_default(), "a hash of the proof"))
        .collect::<Result<Vec<_>, _>>()?;
    if root(index, size, hash(&[&[0], body]), &path) != Some(top) {
        return Err("the inclusion proof does not lead to its root".into());
    }
    checkpoint(text(proof, &["checkpoint", "envelope"])?, log, id, size, &top)
        .map_err(|why| format!("the checkpoint: {why}"))
}

/// The checkpoint `envelope` names the tree of `size` and `top`, and the log
/// signed it. Its origin is the log's name, or Rekor v1's form `NAME - TREE`.
/// A line of a signer that is not the log (a witness) is not read.
fn checkpoint(envelope: &str, log: &Log, id: &[u8; 32], size: u64, top: &[u8; 32]) -> Result<(), String> {
    let (note, signatures) = envelope.split_once("\n\n").ok_or("it has no signature")?;
    let mut lines = note.split('\n');
    let origin = lines.next().unwrap_or_default();
    if origin != log.name && !origin.strip_prefix(log.name.as_str()).is_some_and(|tree| tree.starts_with(" - ")) {
        return Err(format!("its origin {origin:?} is not the log {}", log.name));
    }
    if lines.next().and_then(|s| s.parse::<u64>().ok()) != Some(size) {
        return Err("its size is not the size of the proof".into());
    }
    if lines.next().map(|r| b64(r, "its root")).transpose()?.as_deref() != Some(&top[..]) {
        return Err("its root is not the root of the proof".into());
    }
    let (_, key) = SubjectPublicKeyInfo::from_der(&log.key).map_err(|e| format!("the key of the log: {e}"))?;
    // The note's text, with its last newline, is what the log signed.
    let signed = format!("{note}\n");
    for line in signatures.split('\n').filter(|l| !l.is_empty()) {
        let (name, sig) = line
            .strip_prefix(SIGNATURE_LINE)
            .and_then(|l| l.split_once(' '))
            .ok_or("a line after the note is not a signature")?;
        let sig = b64(sig, "a signature of the note")?;
        if name != log.name || sig.len() < 4 || sig[..4] != id[..4] {
            continue;
        }
        return chain::by_key(&key, signed.as_bytes(), &sig[4..]).map_err(|why| format!("the log's signature: {why}"));
    }
    Err(format!("it has no signature of the log {}", log.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree of RFC 9162, section 2.1.5 (seven leaves), with the leaves
    /// as bytes 0 to 6: each leaf's path gives the root of each size.
    #[test]
    fn each_path_of_a_small_tree_gives_its_root() {
        let leaves: Vec<[u8; 32]> = (0..7u8).map(|i| hash(&[&[0], &[i]])).collect();
        // The root of leaves `from..to` by the definition of RFC 6962.
        fn of(leaves: &[[u8; 32]]) -> [u8; 32] {
            if leaves.len() == 1 {
                return leaves[0];
            }
            let k = leaves.len().next_power_of_two() / 2;
            hash(&[&[1], &of(&leaves[..k]), &of(&leaves[k..])])
        }
        fn path(leaves: &[[u8; 32]], at: usize) -> Vec<[u8; 32]> {
            if leaves.len() == 1 {
                return Vec::new();
            }
            let k = leaves.len().next_power_of_two() / 2;
            let (mut p, sibling) = if at < k {
                (path(&leaves[..k], at), of(&leaves[k..]))
            } else {
                (path(&leaves[k..], at - k), of(&leaves[..k]))
            };
            p.push(sibling);
            p
        }
        for size in 1..=leaves.len() {
            let tree = &leaves[..size];
            let top = Some(of(tree));
            for at in 0..size {
                let p = path(tree, at);
                let (index, n) = (at as u64, size as u64);
                assert_eq!(root(index, n, tree[at], &p), top, "leaf {at} of {size}");
                // Another leaf, a changed sibling, or a path one step short.
                assert_ne!(root(index, n, tree[(at + 1) % size], &p), top.filter(|_| size > 1));
                for i in 0..p.len() {
                    let mut changed = p.clone();
                    changed[i][0] ^= 1;
                    assert_ne!(root(index, n, tree[at], &changed), top);
                    assert_ne!(root(index, n, tree[at], &p[..i]), top);
                }
            }
        }
        assert_eq!(root(7, 7, leaves[0], &[]), None);
    }
}
