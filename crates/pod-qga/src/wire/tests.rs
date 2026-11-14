use super::*;
use serde_json::json;

#[test]
fn a_request_names_its_call_and_leaves_out_no_arguments() {
    assert_eq!(request("guest-ping", None), r#"{"execute":"guest-ping"}"#);
    let line = request("guest-sync-delimited", Some(json!({"id": 7})));
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v, json!({"execute": "guest-sync-delimited", "arguments": {"id": 7}}));
}

/// The text of a path with a quote, a line end and a NUL is escaped: the
/// request stays one line, and the agent reads the same path.
#[test]
fn a_path_with_a_quote_and_a_line_end_stays_in_one_line() {
    let mut e = Exec::new("C:\\a \"b\"\nc\0");
    e.args = vec!["/c".into(), "echo\n".into()];
    let line = request("guest-exec", Some(e.arguments()));
    assert!(!line.contains('\n'), "{line}");
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["arguments"]["path"], "C:\\a \"b\"\nc\0");
    assert_eq!(v["arguments"]["arg"], json!(["/c", "echo\n"]));
}

#[test]
fn the_arguments_of_exec_carry_the_input_in_base64_and_leave_out_empty_lists() {
    let mut e = Exec::new("/bin/sh");
    assert_eq!(e.arguments(), json!({"path": "/bin/sh", "capture-output": true}));
    e.env = vec!["A=1".into()];
    e.input = Some(b"hi\n".to_vec());
    e.capture_output = false;
    assert_eq!(
        e.arguments(),
        json!({"path": "/bin/sh", "env": ["A=1"], "input-data": "aGkK", "capture-output": false})
    );
}

#[test]
fn an_answer_is_a_return_or_an_error_and_nothing_else() {
    assert_eq!(answer(b"{\"return\": {}}\n").unwrap(), Answer::Return(json!({})));
    assert_eq!(answer(b"  {\"return\": 42}\r\n").unwrap(), Answer::Return(json!(42)));
    assert_eq!(
        answer(br#"{"error": {"class": "CommandNotFound", "desc": "The command guest-x has not been found"}}"#)
            .unwrap(),
        Answer::Error { class: "CommandNotFound".into(), desc: "The command guest-x has not been found".into() }
    );
    // An answer with both members is a refusal.
    assert!(matches!(answer(br#"{"return": 1, "error": {"class": "c", "desc": "d"}}"#), Ok(Answer::Error { .. })));
    for bad in
        [&b"{}"[..], b"[1]", b"42", b"not json", b"{\"error\": \"text\"}", b"{\"error\": {\"desc\": \"d\"}}", b"\xff"]
    {
        assert!(answer(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
    }
}

/// The return of `guest-exec-status` on Linux and on Windows: the codes,
/// the output and the cut flags.
#[test]
fn an_exec_status_reads_its_code_its_output_and_its_cut_flags() {
    let running = ExecStatus::from_return(&json!({"exited": false})).unwrap();
    assert_eq!(running, ExecStatus::default());
    let done = ExecStatus::from_return(&json!({
        "exited": true, "exitcode": 3, "out-data": "b3V0", "err-data": "ZXJy", "out-truncated": true
    }))
    .unwrap();
    assert_eq!(done.exit_code, Some(3));
    assert_eq!((done.stdout.as_slice(), done.stderr.as_slice()), (&b"out"[..], &b"err"[..]));
    assert!(done.stdout_truncated && !done.stderr_truncated);
    let killed = ExecStatus::from_return(&json!({"exited": true, "signal": 9})).unwrap();
    assert_eq!((killed.exit_code, killed.signal), (None, Some(9)));
    for bad in [
        json!({}),
        json!(1),
        json!({"exited": "yes"}),
        json!({"exited": true, "exitcode": 4294967296_i64}),
        json!({"exited": true, "out-data": "not base64!"}),
        json!({"exited": true, "out-truncated": 1}),
    ] {
        assert!(ExecStatus::from_return(&bad).is_err(), "{bad}");
    }
}

#[test]
fn a_chunk_holds_as_many_bytes_as_its_count_says() {
    let c = Chunk::from_return(&json!({"count": 5, "buf-b64": "aGVsbG8=", "eof": true})).unwrap();
    assert_eq!(c, Chunk { data: b"hello".to_vec(), eof: true });
    assert_eq!(Chunk::from_return(&json!({"count": 0, "buf-b64": ""})).unwrap(), Chunk::default());
    assert!(Chunk::from_return(&json!({"count": 4, "buf-b64": "aGVsbG8="})).is_err());
    assert!(Chunk::from_return(&json!({"count": -1, "buf-b64": ""})).is_err());
    assert!(Chunk::from_return(&json!({"buf-b64": ""})).is_err());
}

#[test]
fn the_bare_returns_are_integers() {
    assert_eq!(sync_id(&json!(12345)).unwrap(), 12345);
    assert!(sync_id(&json!(-1)).is_err() && sync_id(&json!("1")).is_err());
    assert_eq!(file_handle(&json!(1000)).unwrap(), 1000);
    assert!(file_handle(&json!({})).is_err());
    assert_eq!(exec_pid(&json!({"pid": 4242})).unwrap(), 4242);
    assert!(exec_pid(&json!(4242)).is_err());
    assert_eq!(written(&json!({"count": 3, "eof": false})).unwrap(), 3);
    assert!(written(&json!({"count": -3})).is_err());
}
