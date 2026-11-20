//! The fingerprint of a saved state of any machine line.

use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::warm::{FORMAT, host_cpu, qemu_version};
use crate::{Accel, Result, io_at, qmp};

/// Each input that a saved state depends on, in order: a key and a value
/// for each. Two runs with the same text ([`Inputs::text`]) can share a
/// state.
///
/// A key is one word. The caller adds each input of its guest: the image,
/// the program in the guest, the kernel, the memory, the script, and the
/// machine line with each path of a run as a fixed name ([`Inputs::machine`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    entries: Vec<(String, String)>,
}

impl Inputs {
    /// No input.
    pub fn new() -> Inputs {
        Inputs::default()
    }

    /// `key` with `value`, as it is.
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Inputs {
        debug_assert!(!key.is_empty() && !key.contains(char::is_whitespace), "a key is one word: {key:?}");
        self.entries.push((key.to_string(), value.into()));
        self
    }

    /// The emulator and the CPU: `qemu` (the first line of
    /// `EMULATOR --version`, [`qemu_version`]) and `cpu` ([`cpu`]).
    pub fn emulator(self, emulator: &Path, accel: Accel) -> Result<Inputs> {
        let version = qemu_version(emulator)?;
        Ok(self.with("qemu", version).with("cpu", cpu(accel)?))
    }

    /// `key` with the path, the length and the time of the last change of
    /// the file `path`, in nanoseconds: a new build under the same name is
    /// a new input, with no read of its bytes.
    pub fn file(self, key: &str, path: &Path) -> Result<Inputs> {
        let m = std::fs::metadata(path).map_err(|e| io_at(path, e))?;
        let changed =
            m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        Ok(self.with(key, format!("{} {} {changed}", path.display(), m.len())))
    }

    /// `key` with `sha256:HEX` of the bytes of the file `path`.
    pub fn digest(self, key: &str, path: &Path) -> Result<Inputs> {
        let mut f = std::fs::File::open(path).map_err(|e| io_at(path, e))?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf).map_err(|e| io_at(path, e))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(self.with(key, format!("sha256:{}", hex(&h.finalize()))))
    }

    /// One input `machine` for each argument of `line`, in order.
    pub fn machine(mut self, line: &[String]) -> Inputs {
        for arg in line {
            self = self.with("machine", arg.clone());
        }
        self
    }

    /// The inputs, in order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// The text of the fingerprint: [`FORMAT`], then `KEY VALUE` for each
    /// input, the value as a JSON string, so that a value cannot make a
    /// second line.
    pub fn text(&self) -> String {
        let mut out = format!("{FORMAT}\n");
        for (key, value) in &self.entries {
            out.push_str(&format!("{key} {}\n", qmp::json_text(value)));
        }
        out
    }
}

/// The CPU of a saved state: `tcg`; under KVM `host` and the sha256 of the
/// CPU of the host ([`host_cpu`]), then `kernel` and the release of the
/// kernel of the host: `-cpu host` gives the guest the features that both
/// the CPU and the KVM of that kernel have.
pub fn cpu(accel: Accel) -> Result<String> {
    match accel {
        Accel::Tcg => Ok("tcg".to_string()),
        Accel::Kvm => {
            let info = Path::new("/proc/cpuinfo");
            let release = Path::new("/proc/sys/kernel/osrelease");
            let cpuinfo = std::fs::read_to_string(info).map_err(|e| io_at(info, e))?;
            let kernel = std::fs::read_to_string(release).map_err(|e| io_at(release, e))?;
            Ok(format!("{} kernel {}", host_cpu(&cpuinfo), kernel.trim()))
        }
    }
}

/// `bytes` in lower-case hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
