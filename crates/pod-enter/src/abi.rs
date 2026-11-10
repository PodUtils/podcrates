//! Choose the interposer by `DT_NEEDED`, and refuse on the version
//! predicate.
//!
//! A reader: nothing here executes to find a fact. Each C library needs
//! its own object. Which object a payload gets is a question of facts at
//! the level of `readelf`. A load to see what happens costs a failed
//! container and gives an error that names a symbol, which sends the
//! reader to the symbol and not to the mechanism.
//!
//! Four facts, each measured:
//!
//! 1. The SONAME tells the C libraries apart. The libc of glibc declares
//!    SONAME `libc.so.6`; the libc of musl declares none, so an object
//!    linked against it records `libc.so` in `DT_NEEDED`. No heuristic.
//! 2. The two wrong pairs fail in both directions: a glibc object in a
//!    musl payload is `__snprintf_chk: symbol not found`, and a musl object
//!    in a glibc payload is `libc.so: invalid ELF header`, because on a
//!    glibc host that name is a linker script.
//! 3. The version predicate: the object imported up to `GLIBC_2.34`, the
//!    libc of the payload declared up to `GLIBC_2.31`, the prediction was
//!    "refused", and the loader said
//!    `version 'GLIBC_2.34' not found (required by /i.so)`.
//! 4. The trap of a reader that refuses each object: a shipped
//!    `libc.so.6` is stripped. Its `.symtab` has 0 defined symbols, and its
//!    `.dynsym` has 3136. A reader that asks `.symtab` finds that the libc
//!    of the payload defines nothing, and refuses each object, and that
//!    looks like a check that works. [`Elf::defined`] reads `.dynsym`, and
//!    this module never reads `.symtab`.
//!
//! The reader finds the libc, and has no list of names. There is no table
//! of `lib/x86_64-linux-gnu/libc.so.6` here: the `PT_INTERP` of the
//! payload names the loader that runs it. For musl that file is the libc;
//! for glibc the libc is in the directory of the loader. A table would be
//! a list of the distributions that someone knew.

use std::collections::BTreeMap;

/// The C library of a file.
///
/// `Unknown` is a third state, never one of the two others. The caller
/// refuses a preload into a root file system whose libc is unknown, with
/// that as the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    Glibc,
    Musl,
    Unknown,
}

impl Flavour {
    pub fn word(self) -> &'static str {
        match self {
            Flavour::Glibc => "glibc",
            Flavour::Musl => "musl",
            Flavour::Unknown => "unknown",
        }
    }

    /// What the `DT_NEEDED` of an object asks for.
    ///
    /// `libc.so.6` is the SONAME of glibc, and an object linked against
    /// musl records `libc.so`, because the libc of musl declares no SONAME.
    pub fn of_needed(needed: &[String]) -> Flavour {
        for n in needed {
            match Flavour::of_libc_name(n) {
                Flavour::Unknown => continue,
                f => return f,
            }
        }
        Flavour::Unknown
    }

    /// What one library name means.
    ///
    /// `libc.musl-*.so.1` and `ld-musl-*.so.1` are both of musl, and on musl
    /// they are the same file: the loader is the C library.
    pub fn of_libc_name(name: &str) -> Flavour {
        let base = name.rsplit('/').next().unwrap_or(name);
        if base.starts_with("ld-musl-") || base.starts_with("libc.musl-") || base == "libc.so" {
            return Flavour::Musl;
        }
        if base == "libc.so.6" || base.starts_with("ld-linux") {
            return Flavour::Glibc;
        }
        Flavour::Unknown
    }
}

/// A symbol, and its symbol version.
///
/// The version is half of the answer. A check of the names only misses the
/// usual real failure, a build host newer than the target: each name
/// resolves, and the loader still refuses with the name of a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned {
    pub name: String,
    /// `None` where the symbol carries no version, which is `VER_NDX_GLOBAL`.
    pub version: Option<String>,
    /// `STB_WEAK`. An undefined weak symbol is not a requirement. An object
    /// that gcc builds imports `_ITM_deregisterTMCloneTable`,
    /// `_ITM_registerTMCloneTable` and `__gmon_start__`, which `libc.so.6`
    /// does not define, and it loads: the `crtbegin` of gcc makes them
    /// weak, so the loader binds them to 0. A reader that requires them
    /// refuses each object of gcc, and that looks like a check that works.
    pub weak: bool,
    /// For a versioned import: the file that `.gnu.version_r` names as the
    /// provider of the version (`vn_file`), such as `libdl.so.2` for
    /// `dlsym@GLIBC_2.2.5`. The loader checks the version against that
    /// file. `None` for a definition and for an import with no version.
    pub file: Option<String>,
}

/// Why an ELF file could not be read.
///
/// A named reason, never dropped. A libc that the reader cannot parse is a
/// refusal with a reason, never a pass: a skip must not read as a denial
/// or as a pass.
#[derive(Debug)]
pub struct Unreadable(pub String);

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The parts of an ELF file that the checks read.
#[derive(Debug, Clone)]
pub struct Elf {
    /// Where it was read from, for the refusal message.
    pub path: String,
    /// `e_machine`, so a rootfs of another architecture is refused by name
    /// rather than by a relocation error.
    pub machine: u16,
    /// `PT_INTERP`, where there is one. A shared object usually has none;
    /// an executable that is not static-pie always has one.
    pub interp: Option<String>,
    pub needed: Vec<String>,
    pub soname: Option<String>,
    /// Each section name, from the section table. A caller reads these for
    /// the build markers of Go, which no program header names.
    pub sections: Vec<String>,
    /// From `.dynsym`, never `.symtab` (fact 4 of the module).
    pub defined: Vec<Versioned>,
    pub imported: Vec<Versioned>,
    /// Each version name that this file declares, from `.gnu.version_d`.
    pub declares: Vec<String>,
}

mod elf;

/// `PT_INTERP` from bytes already in hand, without a section table.
///
/// [`Elf::parse`] refuses a file with no readable section table, which is the
/// right answer when classifying a libc and the wrong gate for the memfd rung:
/// a static-PIE payload has no `PT_INTERP` whatever its sections say, and the
/// ladder answers that from the program headers alone. A script (`#!`) or a
/// non-ELF file is an [`Unreadable`] naming that, never a silent `None`.
///
/// Head-safe: `b` can be the first [`HEAD_BYTES`] of a file. Where the
/// program header table or the `PT_INTERP` path lies past `b`, the answer
/// is an [`Unreadable`] ("unknown"), never `Ok(None)` ("static"). The same
/// holds for a whole file whose headers point past its end.
pub fn interp_of(path: &str, b: &[u8]) -> Result<Option<String>, Unreadable> {
    let why = |s: String| Unreadable(format!("{path}: {s}"));
    Image::open(Src::Mem(b)).map_err(why)?.interp().map_err(why)
}

/// [`interp_of`] for a file on disk, with a bounded read: the header, the
/// program header table and the path, never the whole file.
pub fn interp_of_file(path: &str) -> Result<Option<String>, Unreadable> {
    let why = |s: String| Unreadable(format!("{path}: {s}"));
    let file = std::fs::File::open(path).map_err(|e| why(format!("{e}")))?;
    let len = file.metadata().map_err(|e| why(format!("{e}")))?.len();
    Image::open(Src::file(&file, len)).map_err(why)?.interp().map_err(why)
}

/// What [`admits`] answers.
///
/// Two outcomes, and a caller can act on each. A refusal names the libc of
/// the root file system and the libc that the object was built against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Admitted,
    Refused(String),
}

impl Verdict {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Verdict::Admitted)
    }
}

/// May `object` be preloaded into a payload served by `libc`?
///
/// Select and check; never try. The check costs little, it runs before
/// anything executes, and it turns a failure inside a container into a
/// refusal with a reason.
///
/// Four gates, in the order a loader would hit them:
///
/// 1. the **architecture**, because a relocation error is what an unchecked
///    mismatch produces;
/// 2. the **libc**, from `DT_NEEDED` against the own SONAME of the libc: a
///    root file system with `ld-musl-*` that gets a glibc object says so in
///    those words;
/// 3. every **imported symbol** is defined by the libc, read from `.dynsym`;
/// 4. every **imported version** is one the libc declares.
pub fn admits(object: &Elf, libc: &Elf) -> Verdict {
    let refuse = |s: String| Verdict::Refused(s);
    if let Some(v) = pair_gates(object, libc) {
        return v;
    }
    for want in &object.imported {
        // An undefined weak symbol is not a requirement: the loader binds it
        // to 0 and runs ([`Versioned::weak`]).
        if want.weak {
            continue;
        }
        // The version first, in the order of the loader. An object built on
        // glibc 2.39 imports `dlsym@GLIBC_2.34`, because glibc 2.34 merged
        // `libdl` into `libc`. Against glibc 2.31 the symbol is both
        // undefined and at a version that the libc does not declare. The
        // loader says `version 'GLIBC_2.34' not found`; a reader that asks
        // "is it defined" first says that `dlsym` is missing, which is true,
        // and sends the reader to libdl and not to the build host.
        if let Some(v) = &want.version
            && !libc.declares.iter().any(|d| d == v)
        {
            return refuse(format!(
                "{} imports `{}` at version `{v}`, and {} declares {}. refused: This \
                     is a build host newer than the target: the loader refuses \
                     naming the version, whatever the NAME resolves to",
                object.path,
                want.name,
                libc.path,
                if libc.declares.is_empty() {
                    "no versions at all".to_string()
                } else {
                    format!("up to `{}`", newest(&libc.declares))
                }
            ));
        }
        if !libc.defines(want) {
            return refuse(format!(
                "{} imports `{}`, which {} does not define. note: Read from .dynsym: \
                 a shipped libc is stripped and its .symtab defines nothing",
                object.path, want.name, libc.path
            ));
        }
    }
    Verdict::Admitted
}

/// Gates 1 and 2 of [`admits`]: the architecture, then the libc family.
///
/// `None` where both pass. [`admits_set`] runs the same two gates against
/// the C library of a link set, so the two paths cannot disagree on them.
fn pair_gates(object: &Elf, libc: &Elf) -> Option<Verdict> {
    let refuse = |s: String| Some(Verdict::Refused(s));
    if object.machine != libc.machine {
        return refuse(format!(
            "{} is ELF machine 0x{:x} and this rootfs's C library {} is 0x{:x}. \
             An object of another architecture is not preloaded",
            object.path, object.machine, libc.path, libc.machine
        ));
    }
    let (want, have) = (Flavour::of_needed(&object.needed), libc.flavour());
    if have == Flavour::Unknown {
        return refuse(format!(
            "the C library in {} could not be identified: it declares no known \
             SONAME and its name is not a known one either. refused: an object that \
             cannot be checked is not preloaded",
            libc.path
        ));
    }
    if want != have {
        return refuse(format!(
            "{} names {} in DT_NEEDED, so it was built against {}, and this \
             rootfs carries {} ({}). refused: A preloaded object is loaded by the \
             payload's OWN loader and resolves its imports against the payload's \
             libc, so this one cannot serve it",
            object.path,
            object.needed.join(", "),
            want.word(),
            have.word(),
            libc.path
        ));
    }
    None
}

/// The highest version in `declares`, for the message.
///
/// A maximum over the numeric fields, which is right for the `GLIBC_2.34`
/// family. Only a sentence uses it: nothing decides on it.
fn newest(declares: &[String]) -> String {
    let key = |s: &String| -> (String, Vec<u64>) {
        let (name, rest) = s.split_once('_').unwrap_or((s.as_str(), ""));
        (name.to_string(), rest.split('.').filter_map(|p| p.parse().ok()).collect())
    };
    declares.iter().max_by_key(|s| key(s)).cloned().unwrap_or_default()
}

mod search;
pub use search::*;

mod closure;
pub use closure::*;

mod bytes;
use bytes::*;

mod image;
pub use image::{HEAD_BYTES, PHDR_TABLE_MAX, READ_CEILING};
use image::{Image, Src};

#[cfg(test)]
mod tests;
