//! A sweep of the staging directory by another process beside the writes
//! of the table (PodUtils/podvm#568). The sweep is a thread here: the crate
//! starts no thread of its own (`launcher::tests`), and this test is not on
//! its spawn path.

#[cfg(target_os = "linux")]
#[test]
fn a_sweep_never_takes_the_temp_file_of_a_write() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use pod_supervise::table::{self, Any, Container, State, Table};

    let s =
        pod_supervise::Settings::new(std::env::temp_dir().join(format!("pod-supervise-sweep-{}", std::process::id())));
    let staging = s.root.join(table::STAGING_DIR);
    std::fs::create_dir_all(&staging).unwrap();
    let (stop, taken) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
    // As podvm's sweep: delete each `.partial` whose lock it gets.
    let sweeper = {
        let (staging, stop, taken) = (staging.clone(), stop.clone(), taken.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                for entry in std::fs::read_dir(&staging).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if !path.to_string_lossy().ends_with(".partial") {
                        continue;
                    }
                    let Ok(file) = std::fs::File::open(&path) else { continue };
                    let fd = std::os::fd::AsRawFd::as_raw_fd(&file) as i64;
                    let op = pod_hostprobe::sys::LOCK_EX | pod_hostprobe::sys::LOCK_NB;
                    if pod_hostprobe::sys::flock(fd, op).is_ok() && std::fs::remove_file(&path).is_ok() {
                        taken.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        })
    };
    let record = |i: usize| -> Container<Any> {
        let v = serde_json::json!({
            "id": format!("id{i:04}"), "name": format!("n{i}"), "argv": ["true"], "env": [],
            "working_dir": "/", "created_at": "now", "started_at": null, "finished_at": null,
            "state": "exited", "pid": null, "launcher_pid": null, "exit_code": 0, "noticed": null,
        });
        serde_json::from_value(v).expect("a record")
    };
    for i in 0..200 {
        table::update(&s, |t: &mut Table<Any>| {
            t.containers.push(record(i));
            Ok::<_, pod_core::Failure>(())
        })
        .unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    sweeper.join().unwrap();
    assert_eq!(taken.load(Ordering::SeqCst), 0, "the sweep took the temp file of a write");
    let t = table::read::<Any>(&s).unwrap();
    assert_eq!(t.containers.len(), 200, "each write landed");
    assert!(t.containers.iter().all(|c| c.state == State::Exited));
    let _ = std::fs::remove_dir_all(&s.root);
}
