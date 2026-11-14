//! The link set of the interposer object, read inside the root file
//! system.
//!
//! The glibc object needs `libdl.so.2`, `libgcc_s.so.1`, `libc.so.6` and
//! `ld-linux-x86-64.so.2`, and its imports come from all four: `dlsym` at
//! `GLIBC_2.2.5` under `libdl.so.2`, `_Unwind_Resume` at `GCC_3.0` under
//! `libgcc_s.so.1`, `__tls_get_addr` at `GLIBC_2.3` under the loader since
//! glibc 2.34. A check against libc only, or against a fixed list beside
//! it, refuses images that the loader accepts.
//!
//! The set of the check is the set that the loader resolves: the
//! interpreter and the whole `DT_NEEDED` closure of the object, each name
//! found in the library directories of the image and resolved with
//! [`resolve_in`]. An absolute link stays under the root, so a file of the
//! host never decides the set.
//!
//! Each version is checked in its own namespace. The loader checks a
//! reference of a version against the file that `.gnu.version_r` names for
//! it: `GLIBC_*` against libc, `libdl.so.2` or the loader, `GCC_*` against
//! `libgcc_s.so.1`. The symbol itself can come from each member, as the
//! loader binds it in the global scope.

use super::*;
use std::path::Path;

/// How many libraries a link set holds. Bounded, because the closure walk
/// reads files that the image controls.
const MAX_MEMBERS: usize = 64;

/// One library of the link set.
#[derive(Debug, Clone)]
pub struct Member {
    /// The name the loader asked for: the interpreter path or a
    /// `DT_NEEDED` entry.
    pub name: String,
    pub elf: Elf,
}

/// The interpreter and the `DT_NEEDED` closure of one object.
#[derive(Debug, Clone)]
pub struct LinkSet {
    /// The interpreter first, then each library in load order.
    pub members: Vec<Member>,
    /// The C library among the members.
    libc: Option<usize>,
}

impl LinkSet {
    /// The member that answers `name`, by the name it was loaded under or
    /// by its `DT_SONAME`, which is how the loader matches a second request.
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name || m.elf.soname.as_deref() == Some(name))
    }

    /// The C library of the set, where it holds one.
    pub fn libc(&self) -> Option<&Elf> {
        self.libc.map(|i| &self.members[i].elf)
    }

    /// Does a member define `want` at its version? A definition with no
    /// version is the default, and the loader takes it for any version.
    fn defines(&self, want: &Versioned) -> bool {
        self.members.iter().any(|m| {
            m.elf.defined.iter().any(|d| {
                d.name == want.name && (d.version.is_none() || want.version.is_none() || d.version == want.version)
            })
        })
    }

    fn paths(&self) -> String {
        let p: Vec<&str> = self.members.iter().map(|m| m.elf.path.as_str()).collect();
        p.join(", ")
    }
}

/// Does musl's loader answer `name` itself? Its `load_library` treats
/// `libc`, `libpthread`, `librt`, `libm`, `libdl`, `libutil` and `libxnet`
/// as the loader, because the loader is the C library there.
fn musl_builtin(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("lib") else {
        return false;
    };
    ["c.", "pthread.", "rt.", "m.", "dl.", "util.", "xnet."].iter().any(|r| rest.starts_with(r))
}

/// The library directories of the image as guest paths, in the order of
/// the search: the directory of the interpreter, then the directories that
/// the loader family puts on `LD_LIBRARY_PATH` ([`crate::userland::lib_dirs`],
/// the one list).
pub fn guest_lib_dirs(rootfs: &str, interp: &str) -> Vec<String> {
    let base = rootfs.trim_end_matches('/');
    crate::userland::lib_dirs(rootfs, interp)
        .into_iter()
        .filter_map(|d| d.strip_prefix(base).map(str::to_string))
        .collect()
}

/// Read the link set of `object` inside `root`.
///
/// `interp` is the payload's `PT_INTERP`, a guest path. `dirs` are guest
/// library directories in search order. `read` reads one resolved file;
/// production passes [`Elf::read`]. A name the closure needs and the image
/// does not hold is an error that names it and the library that needs it:
/// the loader would refuse the preload there.
pub fn link_set(
    root: &Path,
    object: &Elf,
    interp: &str,
    dirs: &[String],
    read: &dyn Fn(&str) -> Result<Elf, Unreadable>,
) -> Result<LinkSet, String> {
    let host = resolve_in(root, interp)
        .map_err(|k| format!("the interpreter {interp} does not resolve inside the image ({k:?})"))?;
    let loader = read(&host.display().to_string()).map_err(|e| format!("{e}"))?;
    let musl = Flavour::of_libc_name(interp) == Flavour::Musl;
    let mut set = LinkSet { members: vec![Member { name: interp.to_string(), elf: loader }], libc: musl.then_some(0) };
    let mut queue: std::collections::VecDeque<(String, String)> =
        object.needed.iter().map(|n| (n.clone(), object.path.clone())).collect();
    while let Some((name, by)) = queue.pop_front() {
        if set.member(&name).is_some() || (musl && musl_builtin(&name)) {
            continue;
        }
        if set.members.len() >= MAX_MEMBERS {
            return Err(format!("the DT_NEEDED closure of {} holds more than {MAX_MEMBERS} libraries", object.path));
        }
        let elf = find(root, &name, &by, dirs, object.machine, read)?;
        queue.extend(elf.needed.iter().map(|n| (n.clone(), elf.path.clone())));
        if set.libc.is_none() && (name == "libc.so.6" || elf.soname.as_deref() == Some("libc.so.6")) {
            set.libc = Some(set.members.len());
        }
        set.members.push(Member { name, elf });
    }
    Ok(set)
}

/// One `DT_NEEDED` name, found the way the loader finds it: a name with a
/// `/` is a path, any other name is looked for in `dirs`, and a file of
/// another machine is passed over for the next directory.
fn find(
    root: &Path,
    name: &str,
    by: &str,
    dirs: &[String],
    machine: u16,
    read: &dyn Fn(&str) -> Result<Elf, Unreadable>,
) -> Result<Elf, String> {
    let guests: Vec<String> = if name.contains('/') {
        vec![name.to_string()]
    } else {
        dirs.iter().map(|d| format!("{}/{name}", d.trim_end_matches('/'))).collect()
    };
    let mut first_error: Option<String> = None;
    for guest in &guests {
        let Ok(host) = resolve_in(root, guest) else {
            continue;
        };
        match read(&host.display().to_string()) {
            Ok(e) if e.machine == machine => return Ok(e),
            Ok(_) => continue,
            Err(e) => {
                first_error.get_or_insert_with(|| format!("{e}"));
            }
        }
    }
    Err(match first_error {
        Some(e) => format!("{name}, which {by} needs, does not read: {e}"),
        None => format!(
            "{name}, which {by} needs, is in none of the image's library \
             directories ({}), read inside the image",
            dirs.join(" ")
        ),
    })
}

/// May `object` be preloaded where `set` is what the loader resolves?
///
/// The architecture and libc gates of [`admits`] first, against the set's
/// C library. Then each import that is not weak: its version against the
/// file that the reference names, and its name against every member. A
/// refusal names the provider that would satisfy the import.
pub fn admits_set(object: &Elf, set: &LinkSet) -> Verdict {
    // Before the C library of the set: a static object needs none, so "no C
    // library" would name the wrong cause.
    if let Some(v) = static_gate(object) {
        return v;
    }
    let Some(libc) = set.libc() else {
        return Verdict::Refused(format!(
            "the link set read inside the image for {} holds no C library ({})",
            object.path,
            set.paths()
        ));
    };
    if let Some(v) = pair_gates(object, libc) {
        return v;
    }
    for want in object.imported.iter().filter(|w| !w.weak) {
        let from = want.file.as_deref();
        let at = want.version.as_deref();
        if let Some(v) = at {
            // The own namespace of the version: the file that the reference
            // names.
            // Where the object names none, any member may declare it.
            let declared = match from.and_then(|f| set.member(f)) {
                Some(p) => p.elf.declares.iter().any(|d| d == v),
                None => set.members.iter().any(|m| m.elf.declares.iter().any(|d| d == v)),
            };
            if !declared {
                let (who, declares) = match from.and_then(|f| set.member(f)) {
                    Some(p) => (p.elf.path.clone(), p.elf.declares.clone()),
                    None => (set.paths(), Vec::new()),
                };
                return Verdict::Refused(format!(
                    "{} imports `{}` at version `{v}` from {}, and {who} declares {}. \
                     The loader checks the version against that file",
                    object.path,
                    want.name,
                    from.unwrap_or("the link set"),
                    if declares.is_empty() {
                        "no such version".to_string()
                    } else {
                        format!("up to `{}`", newest(&declares))
                    }
                ));
            }
        }
        if !set.defines(want) {
            return Verdict::Refused(format!(
                "{} imports `{}`{}{}, which no library of the link set defines ({}). \
                 The check reads .dynsym, because a shipped library is stripped \
                 and its .symtab defines nothing",
                object.path,
                want.name,
                at.map(|v| format!(" at version `{v}`")).unwrap_or_default(),
                from.map(|f| format!(" from {f}")).unwrap_or_default(),
                set.paths()
            ));
        }
    }
    Verdict::Admitted
}

#[cfg(test)]
mod tests;
