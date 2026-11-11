//! A root file system that runs this binary: a copy of it at
//! `/payload/confine`, each file that this process maps (the C library,
//! the loader) at its own path, the loader also at the path of its
//! `PT_INTERP`, and `/tmp` for the namespace rung. A rule names a guest
//! path, so the image also has `/allowed` and `/secret`, which the host
//! does not have.

use std::path::{Path, PathBuf};

/// The guest path of the binary in the image.
pub const GUEST_EXE: &str = "/payload/confine";

/// The files that this process maps, each by the path that the kernel
/// gives, with no duplicate.
pub fn mapped_files() -> Vec<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").expect("/proc/self/maps");
    let mut out: Vec<PathBuf> = Vec::new();
    for line in maps.lines() {
        // address, mode, offset, device, inode, path.
        let Some(path) = line.split_whitespace().nth(5) else { continue };
        let p = PathBuf::from(path);
        if path.starts_with('/') && p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The directories of `files`, with no duplicate.
pub fn dirs_of(files: &[PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in files {
        let d = f.parent().expect("a file has a directory").to_str().expect("a UTF-8 path").to_string();
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

fn copy_into(root: &Path, guest: &Path, from: &Path) {
    let to = root.join(guest.strip_prefix("/").expect("an absolute path"));
    std::fs::create_dir_all(to.parent().expect("a directory")).expect("the directory in the image");
    std::fs::copy(from, &to).expect("the copy into the image");
}

/// The image under `root`, and the guest directories of its programs and
/// libraries (each needs read and execute).
pub fn build(root: &Path) -> Vec<String> {
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(root.join("tmp")).expect("the image");
    let exe = std::env::current_exe().expect("the test binary");
    let exe = std::fs::canonicalize(exe).expect("the path of the test binary");
    let mut guest = vec![PathBuf::from(GUEST_EXE)];
    copy_into(root, Path::new(GUEST_EXE), &exe);
    for f in mapped_files().into_iter().filter(|f| *f != exe) {
        copy_into(root, &f, &f);
        guest.push(f);
    }
    let interp = pod_enter::abi::Elf::read(exe.to_str().expect("a UTF-8 path")).ok().and_then(|e| e.interp);
    if let Some(interp) = interp {
        let loader = std::fs::canonicalize(&interp).expect("the loader of the test binary");
        copy_into(root, Path::new(&interp), &loader);
        guest.push(PathBuf::from(interp));
    }
    std::fs::write(root.join("allowed"), b"allowed").expect("/allowed");
    std::fs::write(root.join("secret"), b"secret").expect("/secret");
    dirs_of(&guest)
}
