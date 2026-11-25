//! The wire: each request and each reply goes out and back with its schema,
//! and a bad line is `bad-request` with no value of the line.

use serde_json::Map;

use super::*;

/// #233 of podrunner: a field that a request or a reply does not take is
/// refused.
#[test]
fn an_unknown_field_is_refused() {
    assert!(parse_request(br#"{"schema":1,"op":"next","capacity":1,"wait_s":1}"#).is_ok());
    assert!(parse_request(br#"{"schema":1,"op":"next","capacity":1,"wait_s":1,"bogus":1}"#).is_err());
    assert!(parse_reply(br#"{"schema":1,"reply":"done"}"#).is_ok());
    assert!(parse_reply(br#"{"schema":1,"reply":"done","bogus":1}"#).is_err());
}

#[test]
fn a_request_round_trips_with_its_schema() {
    let job = JobId::new("j-1").unwrap();
    let reqs = [
        Request::Hello { client: "podrunner".into(), version: "0.1.0".into(), host: None },
        Request::Next { capacity: 2, wait_s: 30 },
        Request::Ack { job_id: job.clone() },
        Request::Claim { job_id: job.clone(), runner_name: "r".into() },
        Request::Mint { job_id: job.clone(), runner_name: "r".into() },
        Request::Report { job_id: job.clone(), report: Map::new() },
        Request::Audit { line: Map::new() },
        Request::Withdraw { job_id: job.clone(), reason: "wait-deadline".into() },
        Request::Release { job_id: job.clone() },
        Request::Assigned {
            job_id: job,
            real: RealJob {
                run_id: 7,
                job_name: "build".into(),
                event: "push".into(),
                repository: "o/r".into(),
                head_repository: None,
                head_is_fork: None,
                actor: "a".into(),
                job_id: None,
                git_ref: Some("refs/heads/main".into()),
                repo_id: Some(1_296_269),
            },
        },
    ];
    assert_eq!(reqs.iter().map(Request::op).collect::<Vec<_>>(), OPS);
    for r in reqs {
        let line = request_line(&r);
        assert!(line.starts_with("{\"schema\":1,\"op\":") && line.ends_with('\n'), "{line}");
        assert_eq!(parse_request(line.as_bytes()).unwrap(), r);
    }
}

#[test]
fn a_reply_round_trips_with_its_schema() {
    let job = JobId::new("j-1").unwrap();
    let file = MintedFile { name: ".runner".into(), content: Hidden::new("secret-value-41") };
    let replies = [
        Reply::Hello { source: "s".into(), operator_controlled: true },
        Reply::Events { events: vec![Event::Cancel { job_id: job.clone() }] },
        Reply::Credential { credential: Minted { files: vec![file] } },
        Reply::End { code: "tier-mismatch".into(), detail: "a fork".into() },
        Reply::Done {},
        Reply::Error { code: SourceCode::UnknownJob, detail: "no such job".into() },
    ];
    for r in replies {
        let line = reply_line(&r);
        assert!(line.starts_with("{\"schema\":1,\"reply\":") && line.ends_with('\n'), "{line}");
        assert_eq!(parse_reply(line.as_bytes()).unwrap(), r);
    }
}

#[test]
fn bad_lines_are_bad_requests_and_name_no_value() {
    let cases: [&[u8]; 6] = [
        b"not json",
        br#"{"schema":1,"op":"steal"}"#,
        br#"{"schema":1,"op":"ack","job_id":"../x"}"#,
        br#"{"schema":3,"op":"release","job_id":"j"}"#,
        br#"{"schema":1,"op":"ack","job_id":"j","extra":"secret-value-41"}"#,
        br#"{"schema":1,"op":"next","capacity":"secret-value-41","wait_s":1}"#,
    ];
    for line in cases {
        let e = parse_request(line).unwrap_err();
        assert_eq!(e.code, SourceCode::BadRequest, "{}", String::from_utf8_lossy(line));
        assert!(!e.detail.contains("secret-value-41") && !e.detail.contains("../x"), "{}", e.detail);
    }
    let mut long = vec![b' '; LINE_MAX];
    long.extend_from_slice(br#"{"schema":1,"op":"release","job_id":"j"}"#);
    assert_eq!(parse_request(&long).unwrap_err().code, SourceCode::BadRequest);
}

fn real(more: &str) -> String {
    format!(
        r#"{{"schema":1,"op":"assigned","job_id":"j","real":{{"run_id":7,"job_name":"b","event":"push","repository":"o/r","actor":"a"{more}}}}}"#
    )
}

fn real_of(line: &str) -> Result<RealJob, SourceError> {
    match parse_request(line.as_bytes())? {
        Request::Assigned { real, .. } => Ok(real),
        other => panic!("{other:?}"),
    }
}

/// #352 of podrunner: `ref` and `repo_id` are optional; a real job with
/// no one of them stays valid and writes no one of them.
#[test]
fn ref_and_repo_id_are_optional() {
    let r = real_of(&real("")).unwrap();
    assert_eq!((r.git_ref.as_deref(), r.repo_id), (None, None));
    let line = request_line(&Request::Assigned { job_id: JobId::new("j").unwrap(), real: r });
    assert!(!line.contains("\"ref\"") && !line.contains("repo_id"), "{line}");
    let r = real_of(&real(r#","ref":"refs/pull/12/merge","repo_id":1296269"#)).unwrap();
    assert_eq!((r.git_ref.as_deref(), r.repo_id), (Some("refs/pull/12/merge"), Some(1_296_269)));
    let line = request_line(&Request::Assigned { job_id: JobId::new("j").unwrap(), real: r.clone() });
    assert!(line.contains(r#""ref":"refs/pull/12/merge","repo_id":1296269"#), "{line}");
    assert_eq!(real_of(&line).unwrap(), r);
    assert_eq!(real_of(&real(r#","ref":null"#)).unwrap().git_ref, None);
}

/// A `ref` has at most 256 bytes and no control character; the fault names
/// no byte of the ref.
#[test]
fn a_bad_ref_is_a_bad_request() {
    let max = "r".repeat(REF_MAX);
    assert_eq!(real_of(&real(&format!(r#","ref":"{max}""#))).unwrap().git_ref.as_deref(), Some(max.as_str()));
    let long = format!("refs/heads/{}", "x".repeat(REF_MAX));
    for bad in [format!(r#","ref":"{long}""#), r#","ref":"refs/heads/a\nb""#.into(), r#","ref":"a\u009bb""#.into()] {
        let e = real_of(&real(&bad)).unwrap_err();
        assert_eq!(e.code, SourceCode::BadRequest, "{bad}");
        assert!(e.detail.contains("a ref has") && !e.detail.contains("refs/heads"), "{}", e.detail);
    }
    assert!(real_of(&real(r#","repo_id":-1"#)).is_err());
    assert!(real_of(&real(r##","repo_id":"1""##)).is_err());
}
