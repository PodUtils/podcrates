//! A host of the test: each run is recorded and each probe is given, so
//! that an install of the user or of the system runs with no root and no
//! service manager.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pod_core::process::Ran;
use pod_core::{Failure, Stage};
use pod_unit::install::{FileMeta, Host};

/// A run whose line starts with a word of `fail` ends with 1, and one that
/// starts with a word of `missing` does not start, as a program that is not
/// there (the error of the system, with its code); `crontab -l`
/// gives `table`; `schtasks /Query` gives `task`; `sc.exe qdescription`
/// gives `service`; `id -u NAME` gives the uid of `accounts`; a file of
/// `files` has its owner and mode, and each other file is not there.
#[derive(Default)]
pub struct Fake {
    pub runs: RefCell<Vec<String>>,
    pub fail: Vec<&'static str>,
    pub missing: Vec<&'static str>,
    pub table: RefCell<Option<String>>,
    pub task: RefCell<Option<String>>,
    pub service: RefCell<Option<String>>,
    pub admin: bool,
    pub booted: bool,
    pub accounts: Vec<(&'static str, u32)>,
    pub files: HashMap<PathBuf, FileMeta>,
}

impl Fake {
    /// A host where the caller is root, systemd runs, and `alice` is uid 1000.
    pub fn root() -> Fake {
        Fake { admin: true, booted: true, accounts: vec![("alice", 1000), ("toor", 0)], ..Fake::default() }
    }
}

impl Host for Fake {
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure> {
        let line = format!("{program} {}", args.join(" "));
        self.runs.borrow_mut().push(line.clone());
        let ran = |code: i32, stdout: String, stderr: &str| Ran { code: Some(code), stdout, stderr: stderr.into() };
        if self.missing.iter().any(|m| line.starts_with(m)) {
            return Err(Failure::io(Stage::Host, line, std::io::Error::from_raw_os_error(2)));
        }
        if self.fail.iter().any(|f| line.starts_with(f)) {
            return Ok(ran(1, String::new(), "refused by the test"));
        }
        Ok(match (program, args) {
            ("crontab", ["-l"]) => match self.table.borrow().clone() {
                Some(table) => ran(0, table, ""),
                None => ran(1, String::new(), "no crontab for ci"),
            },
            ("crontab", ["-"]) => {
                *self.table.borrow_mut() = Some(String::from_utf8_lossy(input.unwrap_or_default()).into_owned());
                ran(0, String::new(), "")
            }
            ("schtasks", ["/Query", _, _, "/XML"]) => match self.task.borrow().clone() {
                Some(xml) => ran(0, xml, ""),
                None => ran(1, String::new(), "ERROR: The system cannot find the file specified."),
            },
            ("schtasks", ["/Create", _, _, "/XML", file, "/F"]) => {
                let bytes = std::fs::read(file).expect("the XML of the task");
                let units: Vec<u16> = bytes[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                *self.task.borrow_mut() = Some(String::from_utf16_lossy(&units));
                ran(0, String::new(), "")
            }
            ("schtasks", ["/Delete", ..]) => {
                *self.task.borrow_mut() = None;
                ran(0, String::new(), "")
            }
            ("sc.exe", ["qdescription", name, _]) => match self.service.borrow().clone() {
                Some(text) => ran(0, format!("SERVICE_NAME: {name}\nDESCRIPTION:  {text}\n"), ""),
                None => ran(1060, String::new(), "OpenService FAILED 1060"),
            },
            ("sc.exe", ["description", _, text]) => {
                *self.service.borrow_mut() = Some(text.to_string());
                ran(0, String::new(), "")
            }
            ("sc.exe", ["delete", _]) => {
                *self.service.borrow_mut() = None;
                ran(0, String::new(), "")
            }
            ("id", ["-u", name]) => match self.accounts.iter().find(|(n, _)| n == name) {
                Some((_, uid)) => ran(0, format!("{uid}\n"), ""),
                None => ran(1, String::new(), &format!("id: '{name}': no such user")),
            },
            _ => ran(0, String::new(), ""),
        })
    }

    fn admin(&self) -> bool {
        self.admin
    }

    fn systemd_booted(&self) -> bool {
        self.booted
    }

    fn file_meta(&self, path: &Path) -> std::io::Result<FileMeta> {
        // ENOENT, and ERROR_FILE_NOT_FOUND on Windows: an error with the
        // code of the system, as the host gives.
        self.files.get(path).copied().ok_or_else(|| std::io::Error::from_raw_os_error(2))
    }
}

/// Each file under `dir`.
pub fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path.display().to_string());
        }
    }
    out
}
