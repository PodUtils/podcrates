//! A small zip writer and reader for the relocatable archive. It stores
//! each entry with no compression, which needs no codec and keeps the
//! archive readable on any host.
//!
//! The writer follows the ZIP application note: a local header for each
//! entry, a central directory and an end of central directory record. The
//! reader accepts the method 0 (stored) only.

use crate::error::{Error, Result};
use std::path::Path;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const FLAG_UTF8: u16 = 0x0800;
const METHOD_STORE: u16 = 0;
const DOS_DATE_1980_01_01: u16 = 33;

/// One file of an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// An archive in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zip {
    entries: Vec<ZipEntry>,
}

impl Zip {
    pub fn new() -> Zip {
        Zip { entries: Vec::new() }
    }

    pub fn add(&mut self, name: impl Into<String>, data: impl Into<Vec<u8>>) -> Result<()> {
        let name = name.into();
        if !plain_name(&name) {
            return Err(Error::new(format!("the archive path {name:?} is not relative and plain")));
        }
        if name.len() > u16::MAX as usize {
            return Err(Error::new(format!("the archive path {name:?} is too long")));
        }
        if self.entries.iter().any(|entry| entry.name == name) {
            return Err(Error::new(format!("the archive holds {name} twice")));
        }
        self.entries.push(ZipEntry { name, data: data.into(), executable: false });
        Ok(())
    }

    pub fn add_executable(&mut self, name: impl Into<String>, data: impl Into<Vec<u8>>) -> Result<()> {
        let count = self.entries.len();
        self.add(name, data)?;
        self.entries[count].executable = true;
        Ok(())
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.iter().find(|entry| entry.name == name).map(|entry| entry.data.as_slice())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if self.entries.len() > u16::MAX as usize {
            return Err(Error::new("the archive holds too many entries"));
        }
        let mut local = Vec::new();
        let mut central = Vec::new();
        for entry in &self.entries {
            if entry.data.len() > u32::MAX as usize {
                return Err(Error::new(format!("the archive entry {} is too large", entry.name)));
            }
            let offset = local.len() as u32;
            let crc = crc32(&entry.data);
            let size = entry.data.len() as u32;
            let name = entry.name.as_bytes();
            put_u32(&mut local, LOCAL);
            put_u16(&mut local, 20);
            put_u16(&mut local, FLAG_UTF8);
            put_u16(&mut local, METHOD_STORE);
            put_u16(&mut local, 0);
            put_u16(&mut local, DOS_DATE_1980_01_01);
            put_u32(&mut local, crc);
            put_u32(&mut local, size);
            put_u32(&mut local, size);
            put_u16(&mut local, name.len() as u16);
            put_u16(&mut local, 0);
            local.extend_from_slice(name);
            local.extend_from_slice(&entry.data);

            let mode: u32 = if entry.executable { 0o100_755 } else { 0o100_644 };
            put_u32(&mut central, CENTRAL);
            put_u16(&mut central, 0x031e);
            put_u16(&mut central, 20);
            put_u16(&mut central, FLAG_UTF8);
            put_u16(&mut central, METHOD_STORE);
            put_u16(&mut central, 0);
            put_u16(&mut central, DOS_DATE_1980_01_01);
            put_u32(&mut central, crc);
            put_u32(&mut central, size);
            put_u32(&mut central, size);
            put_u16(&mut central, name.len() as u16);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u32(&mut central, mode << 16);
            put_u32(&mut central, offset);
            central.extend_from_slice(name);
        }
        let central_offset = local.len() as u32;
        let central_size = central.len() as u32;
        let mut out = local;
        out.extend_from_slice(&central);
        put_u32(&mut out, END);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, self.entries.len() as u16);
        put_u16(&mut out, self.entries.len() as u16);
        put_u32(&mut out, central_size);
        put_u32(&mut out, central_offset);
        put_u16(&mut out, 0);
        Ok(out)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_bytes()?).map_err(|e| Error::io("write the archive", path, e))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Zip> {
        let end = find_end(bytes)?;
        let count = read_u16(bytes, end + 10)? as usize;
        let central_offset = read_u32(bytes, end + 16)? as usize;
        let mut zip = Zip::new();
        let mut at = central_offset;
        for _ in 0..count {
            if read_u32(bytes, at)? != CENTRAL {
                return Err(Error::new("the central directory has a bad signature"));
            }
            let flags = read_u16(bytes, at + 8)?;
            let method = read_u16(bytes, at + 10)?;
            let crc = read_u32(bytes, at + 16)?;
            let size = read_u32(bytes, at + 24)? as usize;
            let name_len = read_u16(bytes, at + 28)? as usize;
            let extra_len = read_u16(bytes, at + 30)? as usize;
            let comment_len = read_u16(bytes, at + 32)? as usize;
            let external = read_u32(bytes, at + 38)?;
            let local = read_u32(bytes, at + 42)? as usize;
            if method != METHOD_STORE {
                return Err(Error::new(format!("the archive method {method} is not stored")));
            }
            if flags & 0x0008 != 0 {
                return Err(Error::new("a data descriptor is not read"));
            }
            let name = std::str::from_utf8(slice(bytes, at + 46, name_len)?)
                .map_err(|_| Error::new("an archive name is not UTF-8"))?
                .to_string();
            if read_u32(bytes, local)? != LOCAL {
                return Err(Error::new(format!("the local header of {name} is bad")));
            }
            let local_name = read_u16(bytes, local + 26)? as usize;
            let local_extra = read_u16(bytes, local + 28)? as usize;
            let start = local + 30 + local_name + local_extra;
            let data = slice(bytes, start, size)?.to_vec();
            if crc32(&data) != crc {
                return Err(Error::new(format!("the CRC of {name} does not match")));
            }
            zip.entries.push(ZipEntry { name, data, executable: external >> 16 & 0o111 != 0 });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(zip)
    }

    pub fn read(path: &Path) -> Result<Zip> {
        let bytes = std::fs::read(path).map_err(|e| Error::io("read the archive", path, e))?;
        Zip::from_bytes(&bytes)
    }

    /// Write each entry under `dir`. A parent directory is made as needed.
    /// A path that leaves `dir` is refused.
    pub fn extract(&self, dir: &Path) -> Result<()> {
        for entry in &self.entries {
            if !plain_name(&entry.name) {
                return Err(Error::new(format!("the archive path {:?} is not relative and plain", entry.name)));
            }
            let path = dir.join(&entry.name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io("create the archive directory", parent, e))?;
            }
            std::fs::write(&path, &entry.data).map_err(|e| Error::io("write the archive entry", &path, e))?;
            #[cfg(unix)]
            if entry.executable {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| Error::io("set the archive mode", &path, e))?;
            }
        }
        Ok(())
    }
}

fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('/') && !name.split('/').any(|part| part == "..")
}

/// The CRC-32 of the ZIP specification (IEEE 802.3).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn find_end(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 22 {
        return Err(Error::new("the file is too short for a zip"));
    }
    let first = bytes.len().saturating_sub(22 + u16::MAX as usize);
    let mut at = bytes.len() - 22;
    loop {
        if read_u32(bytes, at)? == END {
            return Ok(at);
        }
        if at == first {
            return Err(Error::new("the file has no end of central directory"));
        }
        at -= 1;
    }
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    let end = at.checked_add(len).ok_or_else(|| Error::new("a zip offset overflows"))?;
    bytes.get(at..end).ok_or_else(|| Error::new("a zip field runs past the end of the file"))
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16> {
    let bytes = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let bytes = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crc_is_the_check_value_of_the_specification() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn an_archive_round_trips_with_its_mode() {
        let mut zip = Zip::new();
        zip.add("run.sh", "#!/bin/sh\nexit 0\n").unwrap();
        zip.add("expected/a.json", "{\"score\":1}").unwrap();
        zip.add_executable("simulator/run", "binary").unwrap();
        let bytes = zip.to_bytes().unwrap();
        let read = Zip::from_bytes(&bytes).unwrap();
        assert_eq!(read.get("run.sh"), Some(&b"#!/bin/sh\nexit 0\n"[..]));
        assert!(read.entries().iter().find(|e| e.name == "simulator/run").unwrap().executable);
        assert!(!read.entries().iter().find(|e| e.name == "run.sh").unwrap().executable);
    }

    #[test]
    fn a_path_that_is_not_plain_is_refused() {
        let mut zip = Zip::new();
        assert!(zip.add("../x", "").is_err());
        assert!(zip.add("/x", "").is_err());
        zip.add("a", "1").unwrap();
        assert!(zip.add("a", "2").is_err());
    }

    #[test]
    fn a_damaged_archive_is_refused() {
        assert!(Zip::from_bytes(b"").is_err());
        let mut zip = Zip::new();
        zip.add("a", "hello").unwrap();
        let mut bytes = zip.to_bytes().unwrap();
        let at = bytes.len() - 22;
        bytes[at] ^= 0xff;
        assert!(Zip::from_bytes(&bytes).is_err());
    }
}
