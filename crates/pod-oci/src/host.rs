//! The grammar of a registry host: a DNS name, an IPv4 address or an IPv6
//! address in brackets, with an optional port. One grammar for the domain
//! of a reference and for a host of `--insecure-registry`, so that each
//! host that the policy takes can match a reference.

use std::net::{Ipv4Addr, Ipv6Addr};

/// The longest DNS name (RFC 1035) and the longest label.
const NAME_MOST: usize = 253;
const LABEL_MOST: usize = 63;

/// Why `h` is not `HOST[:PORT]`, or `None` when it is one. The reason is a
/// phrase that follows the host in a refusal.
pub(crate) fn defect(h: &str) -> Option<String> {
    let (host, port) = match split(h) {
        Ok(parts) => parts,
        Err(why) => return Some(why),
    };
    if let Some(p) = port
        && !is_port(p)
    {
        return Some(format!("has the port {p:?}, which is not a number from 1 to 65535"));
    }
    if let Some(inner) = host.strip_prefix('[') {
        let inner = inner.strip_suffix(']').unwrap_or(inner);
        return match inner.parse::<Ipv6Addr>() {
            Ok(_) => None,
            Err(_) => Some(format!("has {inner:?} in brackets, which is not an IPv6 address")),
        };
    }
    name_defect(host)
}

/// The host and the port. An IPv6 address must be in brackets: `::1:5000`
/// has no one reading.
fn split(h: &str) -> Result<(&str, Option<&str>), String> {
    if h.starts_with('[') {
        let Some(end) = h.find(']') else {
            return Err("has a `[` with no `]`".to_string());
        };
        let (host, rest) = h.split_at(end + 1);
        return match rest {
            "" => Ok((host, None)),
            _ => match rest.strip_prefix(':') {
                Some(p) => Ok((host, Some(p))),
                None => Err(format!("has {rest:?} after the `]`, where only `:PORT` can go")),
            },
        };
    }
    match h.matches(':').count() {
        0 => Ok((h, None)),
        1 => Ok(h.split_once(':').map(|(a, p)| (a, Some(p))).unwrap_or((h, None))),
        _ if h.parse::<Ipv6Addr>().is_ok() => Err("is an IPv6 address with no brackets: write it as [ADDRESS]".into()),
        _ => Err("has more than one `:`".to_string()),
    }
}

fn is_port(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n != 0)
}

/// A DNS name by RFC 1123 (letters, digits and `-`, no `-` at either end
/// of a label), or an IPv4 address. A name of digits only must be an IPv4
/// address: `1.2.3.999` and `5000` name no host.
fn name_defect(host: &str) -> Option<String> {
    if host.is_empty() {
        return Some("has no host before the `:`".to_string());
    }
    if host.parse::<Ipv4Addr>().is_ok() {
        return None;
    }
    if let Some(b) = host.chars().find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '.')) {
        return Some(format!("has the character {b:?}, which no host name has"));
    }
    if host.len() > NAME_MOST {
        return Some(format!("is {} bytes long, and a host name has at most {NAME_MOST}", host.len()));
    }
    for label in host.split('.') {
        if label.is_empty() {
            return Some("has an empty label (two dots, or a dot at an end)".to_string());
        }
        if label.len() > LABEL_MOST {
            return Some(format!("has a label of {} bytes, and a label has at most {LABEL_MOST}", label.len()));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Some(format!("has the label {label:?}, which starts or ends with `-`"));
        }
    }
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Some("is digits only and not an IPv4 address".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::defect;

    #[test]
    fn a_name_an_address_and_a_port_pass() {
        for h in [
            "localhost",
            "localhost:5000",
            "reg.lan",
            "my-reg.example.com:443",
            "127.0.0.1",
            "10.0.0.2:5000",
            "[::1]",
            "[::1]:5000",
            "[fe80::1]:65535",
            "xn--bcher-kva.example",
        ] {
            assert_eq!(defect(h), None, "{h}");
        }
    }

    #[test]
    fn each_other_text_is_refused_with_its_reason() {
        for (h, why) in [
            ("reg lan", "character ' '"),
            ("reg!.lan", "character '!'"),
            ("reg_x.lan", "character '_'"),
            ("reg..lan", "empty label"),
            ("reg.lan.", "empty label"),
            ("-reg.lan", "starts or ends with `-`"),
            ("reg-.lan", "starts or ends with `-`"),
            ("reg.lan:", "port \"\""),
            ("reg.lan:0", "port \"0\""),
            ("reg.lan:65536", "port \"65536\""),
            ("reg.lan:50a", "port \"50a\""),
            (":5000", "no host"),
            ("::1", "no brackets"),
            ("a:b:c", "more than one"),
            ("[::1", "no `]`"),
            ("[::1]5000", "after the `]`"),
            ("[reg.lan]:5000", "not an IPv6 address"),
            ("1.2.3.999", "digits only"),
            ("5000", "digits only"),
        ] {
            let got = defect(h).unwrap_or_else(|| panic!("{h:?} passed"));
            assert!(got.contains(why), "{h:?}: {got}");
        }
        let long = format!("{}.lan", "a".repeat(64));
        assert!(defect(&long).unwrap().contains("at most 63"));
        let longer = vec!["a".repeat(60); 5].join(".");
        assert!(defect(&longer).unwrap().contains("at most 253"));
    }
}
