//! The program that a dynamic loader runs.
//!
//! Landlock checks `EXECUTE` on the file that `execve` names. The dynamic
//! loader of an image (`ld-linux-x86-64.so.2`, `ld-musl-x86_64.so.1`) must
//! be executable, and it runs the program of its first argument: it only
//! reads and maps that program, so `ld.so /tmp/prog` runs a program that
//! `exec=` denies. Where a caller sees the exec (the entry with no chroot,
//! an interposer, the handler of a syscall tier), the exec of a loader
//! takes the exec rule of each argument that the loader can take as its
//! program.
//!
//! Pure over bytes: no call of the system, no allocation, no item of std
//! and no other item of the crate. An object that takes no dependency (an
//! interposer) keeps a copy of this file as it is.

/// True where the base name of a path names a dynamic loader:
/// `ld-linux-x86-64.so.2`, `ld-linux-aarch64.so.1`, `ld-2.28.so` (the file
/// of an older glibc), `ld-musl-x86_64.so.1`, `ld64.so.2`, `ld.so.1`, and
/// musl's `libc.so` and `libc.musl-x86_64.so.1`: musl's `make install`
/// makes `ld-musl-ARCH.so.1` a link to `libc.so`, which is the loader.
/// glibc's `libc.so.6` runs no program. The caller asks for the path as
/// given and for the path with each link resolved, so each name of the
/// same file counts.
pub fn is_loader(path: &[u8]) -> bool {
    let base = path.rsplit(|&b| b == b'/').next().unwrap_or(path);
    let named = base.starts_with(b"ld-") || base.starts_with(b"ld.") || base.starts_with(b"ld64.");
    let musl = base == b"libc.so" || base.starts_with(b"libc.musl-");
    musl || (named && base.windows(3).any(|w| w == b".so"))
}

/// The options of glibc's and musl's loaders that take the next argument
/// as their value.
pub const VALUED: [&[u8]; 7] = [
    b"--library-path",
    b"--inhibit-rpath",
    b"--audit",
    b"--preload",
    b"--argv0",
    b"--glibc-hwcaps-prefix",
    b"--glibc-hwcaps-mask",
];

/// The options of the loaders that take no value.
pub const FLAGS: [&[u8]; 7] =
    [b"--list", b"--verify", b"--inhibit-cache", b"--list-tunables", b"--list-diagnostics", b"--help", b"--version"];

/// Give `check` each argument that a loader can take as its program, from
/// the arguments after its own name, and answer false at the first one
/// that `check` refuses. True where the loader runs no program (`--help`)
/// or `check` lets each one run.
///
/// The loaders do not read the options the same way, so each reading
/// counts (fail safe):
///
/// - a valued option (`--library-path X`) skips its value;
/// - `--name=value` is an option for musl and a program for a glibc that
///   does not know the form: both;
/// - `--` ends the options for musl and a newer glibc, and is a program
///   for an older glibc: both, and the next argument;
/// - another `--name` is a program for an older glibc, and a refusal of
///   the others;
/// - each other argument is the program.
///
/// A program with no `/` gets false with no call of `check`: glibc's
/// loader searches such a name as a library (`--library-path`,
/// `LD_LIBRARY_PATH`, its cache), which the check cannot follow.
pub fn each_program<'a, I, F>(args: I, mut check: F) -> bool
where
    I: IntoIterator<Item = &'a [u8]>,
    F: FnMut(&'a [u8]) -> bool,
{
    let mut one = |p: &'a [u8]| p.contains(&b'/') && check(p);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if VALUED.contains(&a) {
            it.next();
            continue;
        }
        if FLAGS.contains(&a) {
            continue;
        }
        if a == b"--" {
            return one(a) && it.next().is_none_or(&mut one);
        }
        let valued =
            a.starts_with(b"--") && a.iter().position(|&b| b == b'=').is_some_and(|eq| VALUED.contains(&&a[..eq]));
        if valued {
            if !one(a) {
                return false;
            }
            continue;
        }
        return one(a);
    }
    true
}
