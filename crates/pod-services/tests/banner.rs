//! The reader of the banner of an SSH forward relay (`forward::Banner`,
//! T-224): a URL that two chunks cut, the rule of match and skip, a line
//! with no URL, the bound on the held bytes, and the marks around a URL.

use pod_services::forward::{Banner, UrlRule};

fn rule(matches: &[&str], skips: &[&str]) -> UrlRule {
    let texts = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
    UrlRule { matches: texts(matches), skips: texts(skips) }
}

/// The URL of `text` read in two chunks, cut at `at`.
fn in_two(rule: &UrlRule, text: &[u8], at: usize) -> Option<String> {
    let mut banner = Banner::new(rule.clone());
    let first = banner.push(&text[..at]).map(str::to_string);
    first.or_else(|| banner.push(&text[at..]).map(str::to_string)).or_else(|| banner.finish().map(str::to_string))
}

#[test]
fn a_url_that_two_chunks_cut_is_read_whole() {
    let tunnel = rule(&[".tunnel.example"], &[]);
    let mut banner = Banner::new(tunnel.clone());
    assert_eq!(banner.push(b"ready: https://ab12.tun"), None);
    assert_eq!(banner.held(), "https://ab12.tun".len(), "the cut word is held");
    assert_eq!(banner.push(b"nel.example\r\n"), Some("https://ab12.tunnel.example"));
    // A cut at each byte of the line.
    let text = b"Your tunnel: https://ab12.tunnel.example\r\n";
    for at in 0..=text.len() {
        assert_eq!(in_two(&tunnel, text, at).as_deref(), Some("https://ab12.tunnel.example"), "cut at {at}");
    }
}

#[test]
fn the_url_of_the_dashboard_is_skipped() {
    let text = "Upgrade at https://dashboard.relay.example\n  https://ab12.relay.example\n";
    let skip = rule(&[], &["dashboard.relay.example"]);
    assert_eq!(skip.pick(text).as_deref(), Some("https://ab12.relay.example"));
    assert_eq!(UrlRule::default().pick(text).as_deref(), Some("https://dashboard.relay.example"), "no rule");
    assert_eq!(skip.pick("Upgrade at https://dashboard.relay.example\n"), None, "the dashboard alone");
}

#[test]
fn a_url_holds_each_text_of_match_and_no_text_of_skip() {
    let both = rule(&["https://", ".tunnel.example"], &["admin.", "/docs"]);
    assert!(both.accepts("https://ab12.tunnel.example"));
    assert!(!both.accepts("http://ab12.tunnel.example"), "one text of match is missing");
    assert!(!both.accepts("https://ab12.other.example"), "the other text of match is missing");
    assert!(!both.accepts("https://admin.tunnel.example"), "a text of skip");
    assert!(!both.accepts("https://ab12.tunnel.example/docs"), "the other text of skip");
    assert!(UrlRule::default().accepts("http://any.example"), "an empty rule takes each URL");
}

#[test]
fn a_line_with_no_url_gives_none() {
    let mut banner = Banner::new(UrlRule::default());
    for chunk in [
        &b"  You are not authenticated.\r\n"[..],
        b"  Your tunnel will expire in 60 minutes.\r\n",
        b"http:// https:/// ftp://x.example mailto:help@x.example\n",
        b"no end of line",
    ] {
        assert_eq!(banner.push(chunk), None, "{}", String::from_utf8_lossy(chunk));
    }
    assert_eq!(banner.finish(), None);
    assert_eq!(banner.held(), 0);
}

#[test]
fn a_banner_with_no_end_of_line_holds_at_most_the_bound() {
    let mut banner = Banner::new(UrlRule::default());
    // 1 MB with no space and no end of line.
    for _ in 0..1024 {
        assert_eq!(banner.push(&[b'a'; 1000]), None);
        assert!(banner.held() <= Banner::MOST_HELD, "held {} bytes", banner.held());
    }
    // The rest of the long word is passed over, its URL too; the next word is read.
    assert_eq!(banner.push(b"https://glued.example\nhttps://ab12.example\n"), Some("https://ab12.example"));
}

#[test]
fn a_url_of_the_bound_is_read_and_a_longer_one_is_not() {
    let fill = "a".repeat(Banner::MOST_HELD - "https://.example".len());
    let most = format!("https://{fill}.example");
    assert_eq!(most.len(), Banner::MOST_HELD);
    let longer = format!("https://a{fill}.example");
    for size in [1, 100, Banner::MOST_HELD + 10] {
        let read = |text: &str| {
            let mut banner = Banner::new(UrlRule::default());
            let pushed = text.as_bytes().chunks(size).find_map(|chunk| banner.push(chunk).map(str::to_string));
            pushed.or_else(|| banner.push(b"\n").map(str::to_string))
        };
        assert_eq!(read(&most).as_deref(), Some(most.as_str()), "chunks of {size}");
        assert_eq!(read(&longer), None, "chunks of {size}");
    }
}

#[test]
fn escapes_and_marks_around_a_url_are_not_part_of_it() {
    let tunnel = rule(&[".tunnel.example"], &[]);
    for text in [
        "\x1b[1;32mStarting HTTPS\x1b[0m on \x1b[4mhttps://ab12.tunnel.example\x1b[0m for localhost\r\n",
        "(see https://ab12.tunnel.example).\n",
        "<https://ab12.tunnel.example>\n",
        "\"https://ab12.tunnel.example\",\n",
        "[https://ab12.tunnel.example]\n",
        "url=https://ab12.tunnel.example\n",
        "\thttps://ab12.tunnel.example",
    ] {
        assert_eq!(tunnel.pick(text).as_deref(), Some("https://ab12.tunnel.example"), "{text:?}");
    }
}

#[test]
fn the_end_of_the_banner_ends_the_held_word() {
    let mut banner = Banner::new(UrlRule::default());
    assert_eq!(banner.push(b"https://ab12.tunnel.example"), None);
    assert_eq!(banner.finish(), Some("https://ab12.tunnel.example"));
    assert_eq!(banner.url(), Some("https://ab12.tunnel.example"));
}

#[test]
fn the_first_url_stays_and_the_reader_reads_no_more() {
    let mut banner = Banner::new(UrlRule::default());
    assert_eq!(banner.push(b"https://one.example https://two.example\n"), Some("https://one.example"));
    assert_eq!(banner.push(b"https://three.example\n"), Some("https://one.example"));
    assert_eq!(banner.finish(), Some("https://one.example"));
    assert_eq!(banner.held(), 0);
}

#[test]
fn a_url_that_is_not_utf8_is_passed_over() {
    let mut banner = Banner::new(UrlRule::default());
    assert_eq!(banner.push(b"https://\xffbad.example\n"), None);
    // A character of more than one byte that two chunks cut, before the URL.
    assert_eq!(banner.push("\u{2192}".as_bytes().split_at(1).0), None);
    let rest = [&"\u{2192}".as_bytes()[1..], b"https://ab12.example\n"].concat();
    assert_eq!(banner.push(&rest), Some("https://ab12.example"));
}
