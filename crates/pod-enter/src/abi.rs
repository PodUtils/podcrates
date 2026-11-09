//! [`TODO/interpose.md`](../../../TODO/interpose.md) T-0709: choose the
//! interposer by `DT_NEEDED`, and refuse on the version predicate.
//!
//! ⭐ **A READER, and nothing here is executed to find anything out.** T-0702
//! establishes that one object per libc is required. Which object a payload
//! gets is then answered by `readelf`-level facts, and answering it instead by
//! loading one and seeing what happens costs a failed container and produces an
//! error naming a symbol, which sends the reader to debug the symbol rather
//! than the mechanism.
//!
//! Four facts, all of them measured by `experiments/80-interposer-abi.sh` and
//! recorded in `experiments/results/interposer-abi.txt`:
//!
//! 1. **the SONAME discriminator.** glibc's libc declares SONAME `libc.so.6`;
//!    musl's declares none, so an object linked against it records `libc.so` in
//!    `DT_NEEDED`. ⛔ There is no ambiguity and no heuristic;
//! 2. **the cross-libc arms**, in both directions: a glibc object in a musl
//!    payload is `__snprintf_chk: symbol not found`, and a musl object in a
//!    glibc payload is `libc.so: invalid ELF header`, because on a glibc host
//!    that name is a linker script;
//! 3. **the version predicate**, predicted from ELF and then confirmed by the
//!    loader: the object imported up to `GLIBC_2.34`, the payload libc declared
//!    up to `GLIBC_2.31`, the prediction was `refused`, and the loader said
//!    `version 'GLIBC_2.34' not found (required by /i.so)`;
//! 4. ⛔ **the trap that makes a naive reader refuse everything.** A shipped
//!    `libc.so.6` is stripped: on this host `.symtab` has **0** defined symbols
//!    and `.dynsym` has **3136**. A reader asking `.symtab` concludes the
//!    payload's libc defines nothing, refuses every artefact, and reads exactly
//!    like a check that works. [`Elf::defined`] reads `.dynsym` and this module
//!    never looks at `.symtab` at all.
//!
//! ⚠ **The libc is FOUND rather than named.** There is no table of
//! `lib/x86_64-linux-gnu/libc.so.6` here: the payload's own `PT_INTERP` says
//! which loader will run it, and for musl that file IS the libc while for glibc
//! the libc sits in the loader's own directory. A table would be a list of the
//! distributions somebody thought of.

use std::collections::BTreeMap;

/// Which C library an artefact belongs to.
///
/// ⛔ `Unknown` is a third state and never folded into either. A rootfs whose
/// libc podvm could not identify is one podvm refuses to preload into, with
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

    /// What an object's `DT_NEEDED` says it wants.
    ///
    /// ⭐ Check A of `experiments/results/interposer-abi.txt`: `libc.so.6` is
    /// glibc's SONAME and `libc.so` is what a musl-linked object records,
    /// because musl's libc declares no SONAME at all.
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
    /// ⚠ `libc.musl-*.so.1` and `ld-musl-*.so.1` are both musl's, and on musl
    /// they are the same file: the loader IS the C library.
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

/// A symbol, and the symbol version attached to it.
///
/// ⛔ The version is half the answer and not a detail. Comparing names alone
/// misses the common real failure, which is a build host newer than the target:
/// every name resolves and the loader still refuses, naming a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned {
    pub name: String,
    /// `None` where the symbol carries no version, which is `VER_NDX_GLOBAL`.
    pub version: Option<String>,
    /// ⛔ **`STB_WEAK`, and an undefined one of these is not a requirement.**
    /// Measured on 2026-09-09: the glibc build of
    /// `references/VHSgunzo__pathmap/tree/path-mapping.c` imports
    /// `_ITM_deregisterTMCloneTable`, `_ITM_registerTMCloneTable` and
    /// `__gmon_start__`, none of which this host's `libc.so.6` defines, and it
    /// loads. GCC's `crtbegin` emits them weak so the loader binds them to 0;
    /// a reader that treated them as requirements refuses **every object gcc
    /// produces**, which reads exactly like a check that works.
    pub weak: bool,
    /// For a versioned import: the file that `.gnu.version_r` names as the
    /// provider of the version (`vn_file`), such as `libdl.so.2` for
    /// `dlsym@GLIBC_2.2.5`. The loader checks the version against that
    /// file, T-1644. `None` for a definition and for an unversioned import.
    pub file: Option<String>,
}

/// Why an ELF file could not be read.
///
/// ⛔ Named rather than swallowed. A libc podvm cannot parse is a **refusal
/// with a reason**, never a pass: T-0109 rule 1, that a skip may not read as a
/// denial or as a pass, applies to a reader exactly as it does to a probe.
#[derive(Debug)]
pub struct Unreadable(pub String);

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The parts of an ELF file T-0709 asks about.
#[derive(Debug, Clone)]
pub struct Elf {
    /// Where it was read from, for the refusal message.
    pub path: String,
    /// `e_machine`, so a rootfs of another architecture is refused by name
    /// rather than by a relocation error.
    pub machine: u16,
    /// `PT_INTERP`, where there is one. ⚠ A shared object usually has none;
    /// an executable that is not static-pie always does.
    pub interp: Option<String>,
    pub needed: Vec<String>,
    pub soname: Option<String>,
    /// Every section name, from the section table. T-0706 reads these for Go
    /// build markers, which no program header names.
    pub sections: Vec<String>,
    /// ⛔ From `.dynsym`, never `.symtab`. Point 4 of this module's header.
    pub defined: Vec<Versioned>,
    pub imported: Vec<Versioned>,
    /// Every version name this file DECLARES, from `.gnu.version_d`.
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
pub fn interp_of(path: &str, b: &[u8]) -> Result<Option<String>, Unreadable> {
    let why = |s: String| Unreadable(format!("{path}: {s}"));
    if b.len() < 64 || &b[0..4] != b"\x7fELF" {
        return Err(why("not an ELF file: the magic is not \\x7fELF".into()));
    }
    let class64 = match b[4] {
        1 => false,
        2 => true,
        c => return Err(why(format!("unknown ELF class {c}"))),
    };
    let msb = match b[5] {
        1 => false,
        2 => true,
        d => return Err(why(format!("unknown ELF data encoding {d}"))),
    };
    Ok(Bytes { b, msb, class64 }.interp())
}

/// What [`admits`] answers.
///
/// ⛔ Two outcomes and each is actionable. A refusal names the libc the rootfs
/// carries and the one the object was built against, which is
/// [`TODO/interpose.md`](../../../TODO/interpose.md) T-0706's channel and
/// T-0108's wording.
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
/// ⛔ **Select and assert, never try.** The assertion is cheap, it runs before
/// anything is executed, and it turns a runtime failure inside somebody's
/// container into a refusal with a reason.
///
/// Four gates, in the order a loader would hit them:
///
/// 1. the **architecture**, because a relocation error is what an unchecked
///    mismatch produces;
/// 2. the **libc**, from `DT_NEEDED` against the libc's own SONAME. ⚠ This is
///    the coarse guard T-0709 asks for by name: a rootfs carrying `ld-musl-*`
///    handed a glibc object says so in those words;
/// 3. every **imported symbol** is defined by the libc, read from `.dynsym`;
/// 4. every **imported version** is one the libc declares.
pub fn admits(object: &Elf, libc: &Elf) -> Verdict {
    let refuse = |s: String| Verdict::Refused(s);
    if let Some(v) = pair_gates(object, libc) {
        return v;
    }
    for want in &object.imported {
        // ⛔ A WEAK undefined symbol is not a requirement: the loader binds it
        // to 0 and runs. See [`Versioned::weak`] for the three gcc emits into
        // every object it produces.
        if want.weak {
            continue;
        }
        // ⛔ **THE VERSION FIRST, and the order is the loader's own.** Measured
        // on 2026-09-09 by `experiments/80-interposer-abi.sh` check E against
        // the pinned glibc 2.31 payload: the object imports `dlsym@GLIBC_2.34`,
        // because glibc 2.34 merged `libdl` into `libc` and the build host is
        // 2.39, so the symbol is BOTH undefined there and at a version that
        // libc does not declare. The loader says `version 'GLIBC_2.34' not
        // found`; a reader that asked "is it defined" first said `dlsym` is
        // missing, which is true and sends the reader to look for libdl instead
        // of at the build host.
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
             podvm refuses to preload an object of another architecture",
            object.path, object.machine, libc.path, libc.machine
        ));
    }
    let (want, have) = (Flavour::of_needed(&object.needed), libc.flavour());
    if have == Flavour::Unknown {
        return refuse(format!(
            "podvm could not identify the C library in {}: it declares no \
             SONAME podvm recognises and its name is not one either. refused: podvm \
             refuses rather than preloading an object it cannot check",
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

/// The highest version among `declares`, for the message.
///
/// ⚠ A LEXICAL maximum over the numeric fields, which is right for the
/// `GLIBC_2.34` family and is only ever used in a sentence: nothing branches on
/// it, so a tie broken the other way costs a word rather than a decision.
fn newest(declares: &[String]) -> String {
    let key = |s: &String| -> (String, Vec<u64>) {
        let (name, rest) = s.split_once('_').unwrap_or((s.as_str(), ""));
        (
            name.to_string(),
            rest.split('.').filter_map(|p| p.parse().ok()).collect(),
        )
    };
    declares
        .iter()
        .max_by_key(|s| key(s))
        .cloned()
        .unwrap_or_default()
}

mod search;
pub use search::*;

mod closure;
pub use closure::*;

mod bytes;
use bytes::*;

#[cfg(test)]
mod tests;
