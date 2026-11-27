use super::*;

/// A file in the first directory of `dirs` that holds `n` attributes
/// `user.NNNN` padded to `len` bytes, each with the value `v`; `None` when
/// no directory holds them (ext4 keeps the attributes of a file in one
/// block, tmpfs holds more).
fn with_names(dirs: &[std::path::PathBuf], tag: &str, n: usize, len: usize) -> Option<std::path::PathBuf> {
    for dir in dirs {
        let f = dir.join(format!("pod-extract-xattr-{}-{tag}", std::process::id()));
        if std::fs::write(&f, b"x").is_err() {
            continue;
        }
        let c = CBuf::from_bytes(f.as_os_str().as_bytes()).unwrap();
        let all = (0..n).all(|i| {
            let name = format!("user.{i:04}");
            let name = format!("{name:x<len$}");
            sys::lsetxattr(&c, &CBuf::new(&name).unwrap(), b"v", 0).is_ok()
        });
        if all {
            return Some(f);
        }
        let _ = std::fs::remove_file(&f);
    }
    None
}

/// PodUtils/podvm#683: one row for each size of the list of names. A
/// buffer of 4096 bytes lost each attribute of a file with more; the reader
/// gives each one, or an error with the path.
#[test]
fn the_attributes_of_a_path_are_read_in_full_for_each_size_of_the_list() {
    let tmp = std::env::temp_dir();
    let shm = std::path::PathBuf::from("/dev/shm");
    // (the row, the count of names, the length of a name, the directories)
    let rows: [(&str, usize, usize, Vec<std::path::PathBuf>); 4] = [
        ("none", 0, 0, vec![tmp.clone()]),
        ("few", 3, 20, vec![tmp.clone(), shm.clone()]),
        ("past 4096 bytes", 60, 100, vec![tmp.clone(), shm.clone()]),
        // Only tmpfs holds this many: the row needs /dev/shm.
        ("past 32 KiB", 300, 120, vec![shm.clone()]),
    ];
    for (label, n, len, dirs) in rows {
        let Some(f) = with_names(&dirs, &format!("{n}"), n, len) else {
            pod_testkit::host::skip_line(&format!(
                "{label}: no directory of {dirs:?} holds {n} user attributes of {len} bytes"
            ));
            continue;
        };
        let got = of_path(&f);
        let _ = std::fs::remove_file(&f);
        let got = got.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(got.len(), n, "{label}: each attribute is read");
        if n > 0 {
            let list: usize = got.iter().map(|(name, _)| name.len() + 1).sum();
            assert_eq!(list, n * (len + 1), "{label}");
            // The order is the order of the file system (tmpfs gives the
            // last name first): compare the sorted names.
            let mut names: Vec<&str> = got.iter().map(|(name, _)| name.as_str()).collect();
            names.sort_unstable();
            let want: Vec<String> = (0..n).map(|i| format!("{:x<len$}", format!("user.{i:04}"))).collect();
            assert_eq!(names, want, "{label}");
        }
        assert!(got.iter().all(|(name, v)| name.starts_with("user.") && v == b"v"), "{label}");
    }
}

/// A missing path is `NotFound`, and the error names the path.
#[test]
fn a_missing_path_is_not_found_with_its_name() {
    let gone = std::env::temp_dir().join(format!("pod-extract-xattr-gone-{}", std::process::id()));
    let e = of_path(&gone).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound, "{e}");
    let text = e.to_string();
    assert!(text.contains(&gone.display().to_string()) && text.contains("llistxattr"), "{text}");
}

/// The reader does not follow a symlink: the attributes of the target are
/// not the attributes of the link.
#[test]
fn a_symlink_is_read_as_the_link() {
    let Some(target) = with_names(&[std::env::temp_dir(), "/dev/shm".into()], "target", 1, 20) else {
        pod_testkit::host::skip_line("no directory holds a user attribute");
        return;
    };
    let link = target.with_extension("link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let got = of_path(&link);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_file(&target);
    // A link has no `user.` attributes (the kernel refuses them on a link).
    assert!(got.unwrap().iter().all(|(n, _)| !n.starts_with("user.")));
}
