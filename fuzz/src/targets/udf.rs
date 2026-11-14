//! The UDF reader of `pod-udf` (T-591).

use pod_udf::{Extent, Limits, ReadAt, Udf};

/// The size of the sparse image of a seed: a read past it is the end of
/// the image.
const SPARSE_END: u64 = 64 << 20;
/// The most directories and files of one walk of `udf_image`.
const WALK: usize = 64;

/// An image as records `[u32 offset / 512][u16 length][bytes]`, little
/// end first; each byte that no record covers is zero. A seed of some KiB
/// thus holds an image of some hundred KiB (the anchor is at 512 KiB).
struct Sparse(Vec<(u64, Vec<u8>)>);

impl Sparse {
    fn parse(mut data: &[u8]) -> Self {
        let mut records = Vec::new();
        while let Some((head, rest)) = data.split_first_chunk::<6>() {
            let at = u64::from(u32::from_le_bytes([head[0], head[1], head[2], head[3]])) * 512;
            let len = usize::from(u16::from_le_bytes([head[4], head[5]])).min(rest.len());
            records.push((at, rest[..len].to_vec()));
            data = &rest[len..];
        }
        Sparse(records)
    }
}

impl ReadAt for Sparse {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let end = offset.checked_add(buf.len() as u64).filter(|end| *end <= SPARSE_END);
        let end = end.ok_or(std::io::ErrorKind::UnexpectedEof)?;
        buf.fill(0);
        for (at, bytes) in &self.0 {
            let (from, to) = ((*at).max(offset), (at + bytes.len() as u64).min(end));
            if from < to {
                let src = &bytes[(from - at) as usize..(to - at) as usize];
                buf[(from - offset) as usize..(to - offset) as usize].copy_from_slice(src);
            }
        }
        Ok(())
    }
}

/// A UDF image (`pod_udf::Udf`, new in `pod-udf`, T-591) as sparse records:
/// the anchor, the volume descriptor sequences, the partition maps with a
/// metadata partition, the file set, the file entries, the allocation
/// descriptors and the directories. A walk of up to 64 directories and
/// files, twice, gives the same answer; the parts of a file add up to its
/// size, and a file that reads has its size.
pub fn udf_image(data: &[u8]) {
    let first = udf_walk(data);
    assert_eq!(first, udf_walk(data));
}

/// Each directory and file that a walk of `data` reaches, up to [`WALK`]:
/// its path and its size, or the text of its fault.
fn udf_walk(data: &[u8]) -> Vec<String> {
    let limits = Limits { depth: 16, entries: 256, directory_bytes: 1 << 20, extents: 1024, descriptors: 64 };
    let mut udf = match Udf::open_with(Sparse::parse(data), limits) {
        Ok(udf) => udf,
        Err(e) => return vec![e.to_string()],
    };
    let mut out = vec![format!("{} {:x}", udf.block_size(), udf.revision())];
    let mut todo = vec![String::from("/")];
    for _ in 0..WALK {
        let Some(path) = todo.pop() else { break };
        let entries = match udf.list(&path) {
            Ok(entries) => entries,
            Err(e) => {
                out.push(e.to_string());
                continue;
            }
        };
        for entry in entries {
            let child = format!("{}/{}", path.trim_end_matches('/'), entry.name);
            if entry.directory {
                todo.push(child);
                continue;
            }
            match udf.find(&child) {
                Ok(extents) => {
                    let sum: u64 = extents.parts.iter().map(Extent::len).sum();
                    assert_eq!(sum, extents.size, "{child}: {extents:?}");
                    if let Ok(bytes) = udf.read(&extents, 1 << 16) {
                        assert_eq!(bytes.len() as u64, extents.size, "{child}");
                    }
                    out.push(format!("{child}: {} bytes in {} parts", extents.size, extents.parts.len()));
                }
                Err(e) => out.push(e.to_string()),
            }
        }
    }
    out
}
