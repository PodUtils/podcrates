//! The TLS end of `pod-broker` (T-100): the head of a request inside a
//! tunnel, and the replace of its swap and of its scrub.

use std::sync::OnceLock;

use pod_broker::Compiled;
use pod_broker::replace::Replace;
use pod_broker::target::{Authority, normal_path, parse_authority};

/// The CONNECT target of the tunnel, and a policy whose rule of the host has
/// a path.
fn tunnel() -> &'static (Authority, Compiled) {
    static TUNNEL: OnceLock<(Authority, Compiled)> = OnceLock::new();
    TUNNEL.get_or_init(|| {
        let policy = r#"{"schema":1,"mode":"allow","allow":[{"host":"api.example.com","path":"/v1"}]}"#;
        let policy = pod_broker::Policy::from_json(policy).expect("a policy").compile().expect("a checked policy");
        (parse_authority("api.example.com:443", None).expect("a target"), policy)
    })
}

/// The head of a request inside a tunnel to `api.example.com:443`
/// (`pod_broker::inner::read`; new in `pod-broker`, T-100), on the bytes
/// after the first: a request that passes has a path in its normal form
/// that the rule holds, and one `Host` that names the target; a refusal is
/// 400, 403, 421, 431 or 505 with words on one line. The replace of the
/// swap and of the scrub gives the same bytes and the same count when the
/// first byte cuts the input in two pieces.
pub fn broker_inner(data: &[u8]) {
    let Some((&cut, head)) = data.split_first() else { return };
    let (authority, policy) = tunnel();
    match pod_broker::inner::read(head, authority, policy) {
        Ok((request, inner)) => {
            assert_eq!(normal_path(&inner.path).as_ref(), Ok(&inner.path), "{head:?}");
            assert!(inner.path == "/v1" || inner.path.starts_with("/v1/"), "{:?}", inner.path);
            let host = request.header("host").expect("a Host");
            assert_eq!(parse_authority(host, Some(443)).as_ref(), Ok(authority));
        }
        Err(refused) => {
            assert!([400, 403, 421, 431, 505].contains(&refused.status), "{refused:?}");
            assert!(crate::is_one_line(&pod_core::text::one_line(&refused.words)));
        }
    }
    let pairs = || {
        [("PODCRED_PLACEHOLDER_01", "made-up-value"), ("made", "M"), ("e-v", "")]
            .map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec()))
    };
    let mut whole = Replace::new(pairs());
    let all = whole.all(head);
    let mut pieces = Replace::new(pairs());
    let at = usize::from(cut).min(head.len());
    let mut out = pieces.push(&head[..at]);
    out.extend(pieces.push(&head[at..]));
    out.extend(pieces.finish());
    assert_eq!(out, all, "cut at {at}");
    assert_eq!(pieces.count(), whole.count());
}
