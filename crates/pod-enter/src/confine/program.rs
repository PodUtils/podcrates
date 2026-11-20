//! The rule `exec=` for each file that the kernel would execute for one
//! argv, on the entry with no chroot: the program, the interpreter of each
//! `#!` line, and the program of a dynamic loader
//! ([`pod_confine::loader`]). There the kernel executes the loader of the
//! image (or a memfd) and only maps the program, so Landlock checks none of
//! these files, and the caller asks before the spawn.

use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use pod_confine::{PathTable, loader};

use crate::userland::Maps;
use crate::{Error, Plan, Result};

/// How many `#!` lines one exec follows: the rewrites of the kernel
/// (`fs/exec.c`, `exec_binprm`: `depth > 5` is `ELOOP`, so five rewrites
/// run, whatever its comment says). One more is `ELOOP` there.
const MOST_SCRIPTS: usize = 5;

/// [`super::check_exec`] for each file that the kernel executes for `argv`
/// (host paths, each resolved as [`super::check_exec`] resolves it): the
/// program, the interpreter of each `#!` line (with its one argument and
/// the script, as the kernel gives them), and where one of these is a
/// dynamic loader ([`loader::is_loader`], by the name as given and as
/// resolved), each argument that the loader can take as its program
/// ([`loader::each_program`]).
///
/// `Ok` with no confinement and for an empty argv. A file outside each
/// execute path is [`Error::CannotInvoke`] (126, `EACCES`), and so is a
/// program of the loader that does not resolve, one with no `/` (the
/// loader searches it as a library), and a loader as the program of a
/// loader. A program or an interpreter that does not resolve is
/// [`Error::NotFound`] (127).
pub fn check_exec_argv(plan: &Plan, argv: &[String]) -> Result<()> {
    let Some(spec) = &plan.confine else { return Ok(()) };
    let resolve = |p: &str| std::fs::canonicalize(p).ok();
    each_file(&spec.exec_table().resolved(), argv.to_vec(), &resolve)
}

/// [`check_exec_argv`] for an argv of guest paths, through the table of the
/// run `maps`: a name with no `/` is the first file of the `PATH` of the
/// plan (`plan.path_dirs`, as [`crate::ladder::guest_invoked`] finds it), a
/// relative name is under the working directory of the plan, and each path
/// resolves through `maps` with the links of the image
/// ([`crate::abi::resolve_in`]), as do the interpreter of each `#!` line
/// and each program of a loader.
///
/// A program or an interpreter that does not resolve is `Ok`: the spawn
/// names it (127). Each other answer is the one of [`check_exec_argv`].
pub fn check_exec_guest(plan: &Plan, maps: &Maps, argv: &[String]) -> Result<()> {
    let Some(spec) = &plan.confine else { return Ok(()) };
    let Some(first) = argv.first() else { return Ok(()) };
    let Some(first) = crate::ladder::guest_invoked(maps, first, &plan.path_dirs) else { return Ok(()) };
    let wd = plan.working_dir.trim_end_matches('/');
    let resolve = |p: &str| {
        let guest = if p.starts_with('/') { p.to_string() } else { format!("{wd}/{p}") };
        crate::abi::resolve_in(maps, &guest).ok()
    };
    let argv: Vec<String> = std::iter::once(first).chain(argv[1..].iter().cloned()).collect();
    match each_file(&spec.exec_table().resolved(), argv, &resolve) {
        Err(Error::NotFound(_)) => Ok(()),
        r => r,
    }
}

/// The walk of [`check_exec_argv`] with the resolution of the caller.
fn each_file(table: &PathTable, mut argv: Vec<String>, resolve: &dyn Fn(&str) -> Option<PathBuf>) -> Result<()> {
    for _ in 0..=MOST_SCRIPTS {
        let Some(name) = argv.first().cloned() else { return Ok(()) };
        let Some(host) = resolve(&name) else {
            return Err(Error::NotFound(format!("{name}: no file of this name to execute")));
        };
        let real = real(&host);
        if !table.allows(&real) {
            return Err(Error::CannotInvoke(format!(
                "{name}: the confinement allows no exec of {real}: it is outside each execute path (EACCES)"
            )));
        }
        if is_loader(&name, &host, &real) {
            return loader_programs(table, &name, &argv[1..], resolve);
        }
        let Some((interp, arg)) = head(&host).as_deref().and_then(crate::memfd::shebang_parts) else {
            return Ok(());
        };
        // The argv of the kernel for a script: the interpreter, its one
        // argument, the script as invoked, then the arguments after it.
        let rest = argv.split_off(1);
        argv = std::iter::once(interp).chain(arg).chain(std::iter::once(name)).chain(rest).collect();
    }
    Err(Error::CannotInvoke(format!(
        "{}: more than {MOST_SCRIPTS} levels of #! interpreters (ELOOP)",
        argv.first().map(String::as_str).unwrap_or_default()
    )))
}

/// The rule for each argument that the loader `name` can take as its
/// program.
fn loader_programs(
    table: &PathTable,
    name: &str,
    args: &[String],
    resolve: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<()> {
    let mut refused: Option<Error> = None;
    let held = loader::each_program(args.iter().map(|a| a.as_bytes()), |p| {
        let p = String::from_utf8_lossy(p);
        let Some(host) = resolve(&p) else {
            refused = Some(Error::CannotInvoke(format!(
                "{name} {p}: the program of the dynamic loader does not resolve, so the confinement cannot judge it \
                 (EACCES)"
            )));
            return false;
        };
        let real = real(&host);
        if !table.allows(&real) {
            refused = Some(Error::CannotInvoke(format!(
                "{name} {p}: the confinement allows no exec of {real}, the program of the dynamic loader: it is \
                 outside each execute path (EACCES)"
            )));
            return false;
        }
        // A loader that the loader runs reads its own arguments, which
        // this walk does not give it.
        if is_loader(&p, &host, &real) {
            refused = Some(Error::CannotInvoke(format!(
                "{name} {p}: the program of the dynamic loader is a dynamic loader, which the confinement cannot \
                 judge (EACCES)"
            )));
            return false;
        }
        true
    });
    if held {
        return Ok(());
    }
    Err(refused.unwrap_or_else(|| {
        Error::CannotInvoke(format!(
            "{name}: the dynamic loader would search its program as a library, which the confinement cannot judge \
             (EACCES)"
        ))
    }))
}

/// True where a name of the file is the name of a loader: as given, as
/// the table resolved it, or with each link of the host resolved.
fn is_loader(name: &str, host: &Path, real: &str) -> bool {
    loader::is_loader(name.as_bytes())
        || loader::is_loader(host.as_os_str().as_bytes())
        || loader::is_loader(real.as_bytes())
}

/// The path with each link of the host resolved, or the path as it is.
fn real(host: &Path) -> String {
    std::fs::canonicalize(host).unwrap_or_else(|_| host.to_path_buf()).to_string_lossy().into_owned()
}

/// The bytes of a file that the kernel reads for a `#!` line, or `None`
/// where it does not read: the spawn then names the file.
fn head(path: &Path) -> Option<Vec<u8>> {
    let mut b = Vec::with_capacity(crate::memfd::SHEBANG_HEAD);
    std::fs::File::open(path).ok()?.take(crate::memfd::SHEBANG_HEAD as u64).read_to_end(&mut b).ok()?;
    Some(b)
}

#[cfg(test)]
mod tests;
