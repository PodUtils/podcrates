use super::*;

const NAME: &str = "a0b1.t.example";

/// The answer to the question of `kind` for [`NAME`] with `flags`: each
/// record of `answers` is `(type, data)` with its owner a pointer to the
/// question; `opt` is the OPT record `(size, extended RCODE)`.
fn answer(id: u16, flags: u16, kind: u16, answers: &[(u16, Vec<u8>)], opt: Option<(u16, u8)>) -> Vec<u8> {
    let mut out = id.to_be_bytes().to_vec();
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&[0, 1, 0, answers.len() as u8, 0, 0, 0, u8::from(opt.is_some())]);
    for label in NAME.split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&[0, 1]);
    for (rtype, data) in answers {
        out.extend_from_slice(&[0xc0, 12]);
        out.extend_from_slice(&rtype.to_be_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
    }
    if let Some((size, high)) = opt {
        out.push(0);
        out.extend_from_slice(&OPT.to_be_bytes());
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&[high, 0, 0, 0, 0, 0]);
    }
    out
}

/// The data of a TXT record of `strings`.
fn txt(strings: &[&[u8]]) -> Vec<u8> {
    strings.iter().flat_map(|s| std::iter::once(s.len() as u8).chain(s.iter().copied())).collect()
}

/// The data of a CNAME record whose target is `name`, with no pointer.
fn cname(name: &str) -> Vec<u8> {
    let mut out: Vec<u8> = name.split('.').flat_map(|l| std::iter::once(l.len() as u8).chain(l.bytes())).collect();
    out.push(0);
    out
}

/// RFC 6891, section 6.1.2: the OPT record is the root name, the type 41,
/// the size in the class, and a TTL of 0 for the extended RCODE, the
/// version and the flags.
#[test]
fn a_query_has_the_id_and_the_opt_record_with_its_size() {
    let q = query(0x1234, NAME, TXT, Some(EDNS_SIZE)).unwrap();
    let mut plain = wire::query(NAME, TXT).unwrap();
    plain[..2].copy_from_slice(&[0x12, 0x34]);
    plain[11] = 1;
    plain.extend_from_slice(&[0, 0, 41, 0x04, 0xd0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(q, plain);
    let q = query(7, NAME, CNAME, None).unwrap();
    assert_eq!(&q[..2], &[0, 7]);
    assert_eq!(&q[10..12], &[0, 0], "no OPT record");
    assert_eq!(&q[q.len() - 4..], &[0, 5, 0, 1]);
    assert!(query(1, NAME, TXT, Some(511)).unwrap_err().contains("512"));
    assert!(query(1, NAME, TXT, Some(PLAIN_SIZE)).is_ok());
    assert!(query(1, "a b.example", TXT, None).is_err());
}

/// The strings of each TXT record, in order, and each CNAME target; the
/// size and the extended RCODE of the server's OPT record.
#[test]
fn an_answer_gives_its_txt_strings_its_cnames_and_its_opt_record() {
    let big = vec![b'x'; 255];
    let records = vec![
        (CNAME, cname("Next.T.Example")),
        (TXT, txt(&[b"abc", b"", &big])),
        (wire::A, vec![192, 0, 2, 1]),
        (TXT, txt(&[b"\x00\xff"])),
    ];
    let msg = answer(9, 0x8180, TXT, &records, Some((1232, 0)));
    let found = read(&msg, 9, "A0B1.t.example.", TXT).unwrap();
    assert!(!found.truncated);
    assert_eq!((found.rcode, found.edns), (0, Some(1232)));
    assert_eq!(found.records.len(), 3, "the A record is left out");
    assert_eq!(found.records[1], Record::Txt(vec![b"abc".to_vec(), Vec::new(), big.clone()]));
    assert_eq!(found.cnames(), vec!["next.t.example"]);
    let mut joined = b"abc".to_vec();
    joined.extend_from_slice(&big);
    assert_eq!(found.txt(), vec![joined, vec![0, 0xff]]);
    // An extended RCODE: 1 in the OPT record and 0 in the header is 16.
    let msg = answer(9, 0x8180, TXT, &[], Some((4096, 1)));
    assert_eq!(read(&msg, 9, NAME, TXT).unwrap().rcode, 16);
    // NXDOMAIN and SERVFAIL are answers: the caller backs off on its RCODE.
    let msg = answer(9, 0x8183, TXT, &[], None);
    assert_eq!(
        read(&msg, 9, NAME, TXT).unwrap(),
        Answer { id: 9, truncated: false, rcode: 3, edns: None, records: Vec::new() }
    );
    assert_eq!(read(&answer(9, 0x8182, TXT, &[], None), 9, NAME, TXT).unwrap().rcode, 2);
}

/// The TC bit: a cut answer gives the records before the cut; with no TC
/// bit, each cut is refused, never a panic.
#[test]
fn a_cut_answer_needs_the_tc_bit() {
    let records = vec![(TXT, txt(&[b"first"])), (TXT, txt(&[b"second"]))];
    let whole = answer(3, 0x8180, TXT, &records, None);
    let first_end = whole.len() - (2 + 10 + 7);
    let mut cut = answer(3, 0x8380, TXT, &records, None);
    for at in first_end..cut.len() {
        cut.truncate(at);
        let found = read(&cut, 3, NAME, TXT).unwrap();
        assert!(found.truncated);
        assert_eq!(found.txt(), vec![b"first".to_vec()], "cut at {at}");
    }
    for at in 0..whole.len() {
        assert!(read(&whole[..at], 3, NAME, TXT).is_err(), "cut at {at}");
    }
    assert_eq!(read(&whole, 3, NAME, TXT).unwrap().records.len(), 2);
    // A server may cut the records and set the counts to what it sends.
    let msg = answer(3, 0x8380, TXT, &records[..1], None);
    assert!(read(&msg, 3, NAME, TXT).unwrap().truncated);
}

/// A query, another ID, another question, a TXT string past its record and
/// a CNAME that is not one name are refused.
#[test]
fn foreign_and_bad_answers_are_refused() {
    let ok = answer(5, 0x8180, TXT, &[(TXT, txt(&[b"a"]))], None);
    assert!(read(&ok, 5, NAME, TXT).is_ok());
    assert!(read(&ok, 6, NAME, TXT).unwrap_err().contains("ID 5"));
    assert!(read(&ok, 5, "other.example", TXT).unwrap_err().contains("another question"));
    assert!(read(&ok, 5, NAME, CNAME).unwrap_err().contains("another question"));
    let q = query(5, NAME, TXT, Some(EDNS_SIZE)).unwrap();
    assert!(read(&q, 5, NAME, TXT).unwrap_err().contains("query"));
    let past = answer(5, 0x8180, TXT, &[(TXT, vec![4, b'a', b'b'])], None);
    assert!(read(&past, 5, NAME, TXT).unwrap_err().contains("past"));
    let mut long = cname("a.example");
    long.push(0);
    let long = answer(5, 0x8180, CNAME, &[(CNAME, long)], None);
    assert!(read(&long, 5, NAME, CNAME).unwrap_err().contains("CNAME"));
    let root = answer(5, 0x8180, CNAME, &[(CNAME, vec![0])], None);
    assert!(read(&root, 5, NAME, CNAME).unwrap_err().contains("CNAME"));
    // A CNAME target that is a pointer to the question's name.
    let pointer = answer(5, 0x8180, CNAME, &[(CNAME, vec![0xc0, 12])], None);
    assert_eq!(read(&pointer, 5, NAME, CNAME).unwrap().cnames(), vec![NAME]);
}

/// The server helpers: the question of a query, and an answer whose TXT
/// records the reader gives back whole, for a carrier of bytes over DNS.
#[test]
fn a_server_reads_a_question_and_builds_an_answer() {
    let q = query(0x4321, "A1B2.T.Example", TXT, Some(EDNS_SIZE)).unwrap();
    assert_eq!(super::question(&q).unwrap(), (0x4321, "a1b2.t.example".to_string(), TXT));
    let big = vec![7u8; 600];
    let records: &[&[u8]] = &[b"up", &[], &big];
    let msg = super::answer(0x4321, "a1b2.t.example", TXT, records, Some(EDNS_SIZE)).unwrap();
    let got = read(&msg, 0x4321, "a1b2.t.example", TXT).unwrap();
    assert_eq!(got.rcode, 0);
    assert_eq!(got.edns, Some(EDNS_SIZE));
    assert_eq!(got.txt(), vec![b"up".to_vec(), Vec::new(), big]);
    // A query with no question is refused.
    assert!(super::question(&q[..4]).is_err());
}

/// The EDNS0 size of a query: its OPT record, at least 512; none with no
/// OPT record, and none for a message cut in its question or its records.
#[test]
fn a_server_reads_the_size_that_a_query_takes() {
    let q = query(1, "a.t.example", TXT, Some(EDNS_SIZE)).unwrap();
    assert_eq!(asked_size(&q), Some(EDNS_SIZE));
    assert_eq!(asked_size(&query(1, "a.t.example", TXT, Some(4096)).unwrap()), Some(4096));
    assert_eq!(asked_size(&query(1, "a.t.example", TXT, None).unwrap()), None);
    // An OPT record under 512 counts as 512.
    let mut small = q.clone();
    let at = small.len() - 8;
    small[at..at + 2].copy_from_slice(&100u16.to_be_bytes());
    assert_eq!(asked_size(&small), Some(PLAIN_SIZE));
    // A message cut in its OPT record, or in its question.
    assert_eq!(asked_size(&q[..q.len() - 3]), None);
    assert_eq!(asked_size(&q[..14]), None);
    assert_eq!(asked_size(&[]), None);
}
