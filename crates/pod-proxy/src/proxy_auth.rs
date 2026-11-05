//! What a proxy's `407` asks for. RFC 9110, section 11.7.1: the answer
//! carries `Proxy-Authenticate` with each scheme that the proxy takes. The
//! dial sends Basic from the user and password of the proxy URL, and answers
//! a Digest challenge ([`crate::digest`]) on a new connection; an NTLM or
//! Negotiate proxy refuses both, and the message must say why rather than
//! only that it refused. [`schemes`] is public for a caller that speaks to
//! a proxy of its own (podssh's second proxy over TLS) and says its `407`
//! in its own words.

/// What the dial sent with the `CONNECT` that got the `407`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sent {
    /// No credentials: the proxy URL has none.
    Nothing,
    /// Basic, with the user and password of the proxy URL.
    Basic,
    /// A Digest answer to the proxy's challenge.
    Digest,
}

/// The schemes of each `Proxy-Authenticate` of a response head, in order,
/// each once. A header can hold more than one challenge, apart at commas,
/// with parameters after a scheme (`Basic realm="corp", NTLM`): a part is a
/// scheme when its first word is a token with no `=` after it.
pub fn schemes(head: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in head.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        if !name.trim().eq_ignore_ascii_case("proxy-authenticate") {
            continue;
        }
        for part in outside_quotes(value) {
            let part = part.trim_start();
            let word = part.split([' ', '=']).next().unwrap_or("");
            let is_token =
                !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+.^_`|~-".contains(c));
            let is_param = part[word.len()..].trim_start().starts_with('=');
            if is_token && !is_param && !found.iter().any(|s| s.eq_ignore_ascii_case(word)) && found.len() < 8 {
                found.push(word.to_string());
            }
        }
    }
    found
}

/// The parts of a header's value apart at commas that are not inside a
/// quoted string: `realm="a, b"` stays one part.
fn outside_quotes(value: &str) -> Vec<&str> {
    let (mut parts, mut start, mut quoted, mut escaped) = (Vec::new(), 0, false, false);
    for (i, c) in value.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

/// The reason of a `407`, with what the proxy asks for, what the program
/// `app` sent, and what can be done: a Digest answer that the proxy refused
/// names the user and the password; a Digest challenge that `app` did not
/// answer names the algorithms that it answers; a scheme other than Basic
/// and Digest names a local proxy that speaks it.
pub(crate) fn explain(reason: &str, head: &str, sent: Sent, app: &str) -> String {
    let asks = schemes(head);
    if asks.is_empty() {
        return reason.to_string();
    }
    let what = match sent {
        Sent::Digest => format!(
            "{app} sent a Digest answer with the user and the password of the proxy URL, and the proxy refused it: \
             check the user and the password"
        ),
        Sent::Basic => format!("{app} sent Basic with the user and the password of the proxy URL"),
        Sent::Nothing => format!("{app} sent no credentials (put user:password@ in the proxy URL)"),
    };
    let has = |name: &str| asks.iter().any(|s| s.eq_ignore_ascii_case(name));
    let advice = if has("basic") {
        String::new()
    } else if has("digest") {
        match sent {
            Sent::Basic => format!(
                "; the Digest challenge has an algorithm or a qop that {app} does not answer ({app} answers MD5, \
                 MD5-sess, SHA-256 and SHA-256-sess with qop auth)"
            ),
            _ => String::new(),
        }
    } else {
        format!(
            "; {app} supports Basic and Digest only: ask for such an account, or use a local proxy that speaks the \
             scheme (for example cntlm for NTLM)"
        )
    };
    format!("{reason}; it asks for {}; {what}{advice}", asks.join(" or "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet's words of a `407`, as the dial makes them for its program.
    fn explain_407(head: &[u8], sent_basic: bool, digest_tried: bool) -> String {
        let sent = match (sent_basic, digest_tried) {
            (_, true) => Sent::Digest,
            (true, false) => Sent::Basic,
            (false, false) => Sent::Nothing,
        };
        explain("Proxy Authentication Required", &String::from_utf8_lossy(head), sent, "podnet")
    }

    #[test]
    fn a_407_names_the_schemes() {
        let head = b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: NTLM, Negotiate\r\nproxy-authenticate: Basic realm=\"a, b\", Digest realm=\"x\", qop=\"auth\"\r\n\r\n";
        let text = explain_407(head, true, false);
        assert!(text.contains("it asks for NTLM or Negotiate or Basic or Digest"), "{text}");
        assert!(text.contains("podnet sent Basic"), "{text}");
        let text = explain_407(b"HTTP/1.1 407 x\r\nProxy-Authenticate: NTLM, Negotiate\r\n\r\n", false, false);
        assert!(text.contains("NTLM or Negotiate") && text.contains("podnet supports Basic and Digest only"), "{text}");
        assert!(text.contains("sent no credentials"), "{text}");
    }

    // A Digest 407 does not get the advice of Basic only.
    #[test]
    fn a_407_with_digest_does_not_say_basic_only() {
        let head = b"HTTP/1.1 407 x\r\nProxy-Authenticate: Digest realm=\"r\", nonce=\"n\", algorithm=MD5, qop=\"auth\"\r\n\r\n";
        let text = explain_407(head, true, true);
        assert!(!text.contains("Basic and Digest only"), "{text}");
        assert!(text.contains("check the user and the password"), "{text}");
        let text = explain_407(head, false, false);
        assert!(!text.contains("only"), "{text}");
        assert!(text.contains("sent no credentials"), "{text}");
        let odd =
            b"HTTP/1.1 407 x\r\nProxy-Authenticate: Digest realm=\"r\", nonce=\"n\", algorithm=SHA-512-256\r\n\r\n";
        let text = explain_407(odd, true, false);
        assert!(text.contains("does not answer"), "{text}");
    }
}
