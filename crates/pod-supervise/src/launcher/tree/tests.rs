use super::*;

use std::path::PathBuf;

/// A directory in the form of procfs, with a `stat` for each pair
/// (pid, parent) and, with `files`, the `children` files of each task.
fn fake_proc(name: &str, pairs: &[(i64, i64)], files: bool) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pod-supervise-proc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for &(pid, ppid) in pairs {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(dir.join("task").join(pid.to_string())).unwrap();
        // A name with a space and a `)`, as a payload can set.
        std::fs::write(dir.join("stat"), format!("{pid} (a b) c) S {ppid} {pid} {pid} 0 -1 4194560")).unwrap();
        if files {
            let kids: Vec<String> = pairs.iter().filter(|(_, p)| *p == pid).map(|(c, _)| c.to_string()).collect();
            let text = if kids.is_empty() { String::new() } else { format!("{} ", kids.join(" ")) };
            std::fs::write(dir.join("task").join(pid.to_string()).join("children"), text).unwrap();
        }
    }
    root
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort();
    v
}

#[test]
fn the_parent_is_read_after_the_last_parenthesis() {
    assert_eq!(parent_of("12 (sh) S 7 12 12 0"), Some(7));
    assert_eq!(parent_of("12 (a b) c) S 9 12 12 0"), Some(9));
    assert_eq!(parent_of("12 (x) Z"), None);
    assert_eq!(parent_of("no parenthesis"), None);
}

/// The tree 100 -> 101 -> 102, 100 -> 103, and 200 outside it.
const TREE: &[(i64, i64)] = &[(100, 1), (101, 100), (102, 101), (103, 100), (200, 1)];

#[test]
fn the_descendants_come_from_the_children_files() {
    let proc = fake_proc("files", TREE, true);
    assert_eq!(sorted(descendants(&proc, 100).unwrap()), vec![101, 102, 103]);
    assert_eq!(descendants(&proc, 102).unwrap(), Vec::<i64>::new());
    let _ = std::fs::remove_dir_all(proc);
}

#[test]
fn with_no_children_file_the_descendants_come_from_each_stat() {
    let proc = fake_proc("scan", TREE, false);
    assert_eq!(sorted(descendants(&proc, 100).unwrap()), vec![101, 102, 103]);
    assert_eq!(sorted(descendants(&proc, 1).unwrap()), vec![100, 101, 102, 103, 200]);
    let _ = std::fs::remove_dir_all(proc);
}

#[test]
fn with_no_proc_the_descendants_are_not_known() {
    let gone = std::env::temp_dir().join(format!("pod-supervise-proc-none-{}", std::process::id()));
    assert_eq!(descendants(&gone, 100), None);
    std::fs::create_dir_all(&gone).unwrap();
    assert_eq!(descendants(&gone, 100), None, "an empty directory is no procfs");
    let _ = std::fs::remove_dir_all(gone);
}

#[test]
fn the_descendants_of_this_process_are_read_from_the_real_proc() {
    let proc = Path::new("/proc");
    if !proc.join("self").exists() {
        return;
    }
    let mut child = std::process::Command::new("sleep").arg("30").spawn().expect("sleep");
    let me = std::process::id() as i64;
    let tree = descendants(proc, me).expect("the descendants of this process");
    let _ = child.kill();
    let _ = child.wait();
    assert!(tree.contains(&(child.id() as i64)), "{tree:?}");
}
