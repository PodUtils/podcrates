//! A contact of an ACME account (RFC 8555, section 7.3): a `mailto:` URI
//! (RFC 6068) of one address, where the CA sends the warnings of expiry and
//! of revocation. The check here comes before each request: a contact that
//! the CA would refuse, or that would name more than one address, is refused
//! by name before the account step.

use std::fmt;

/// The scheme of each contact.
const MAILTO: &str = "mailto:";
/// The longest address: the path of RFC 5321, section 4.5.3.1.3.
pub const MAX_ADDRESS: usize = 254;
/// The longest local part (RFC 5321, section 4.5.3.1.1).
const MAX_LOCAL: usize = 64;
/// The characters of a local part past letters and digits: the atext of
/// RFC 5322, section 3.2.3, and the dot. No `%`, `?` or `,`: each needs
/// the escapes of RFC 6068, and a CA refuses them.
const LOCAL_MARKS: &str = "!#$&'*+-/=^_`{|}~.";

/// A `mailto:` URI of one address, checked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Contact(String);

impl Contact {
    /// The contact of a `mailto:` URI, as RFC 8555 sends it. The scheme can
    /// be in any case; it is kept in lower case.
    pub fn parse(text: &str) -> Result<Contact, String> {
        let refuse = |why: &str| format!("the contact {text:?}: {why}");
        let scheme = text.get(..MAILTO.len()).filter(|s| s.eq_ignore_ascii_case(MAILTO));
        if scheme.is_none() {
            return Err(refuse("give a mailto: URI of one e-mail address, for example mailto:ops@example.org"));
        }
        let address = &text[MAILTO.len()..];
        check_address(address).map_err(|why| refuse(&why))?;
        Ok(Contact(format!("{MAILTO}{address}")))
    }

    /// The contact of one e-mail address, as a user gives it.
    pub fn mailto(address: &str) -> Result<Contact, String> {
        check_address(address).map_err(|why| format!("the e-mail address {address:?}: {why}"))?;
        Ok(Contact(format!("{MAILTO}{address}")))
    }

    /// The URI, as the CA gets it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The address, with no scheme.
    pub fn address(&self) -> &str {
        &self.0[MAILTO.len()..]
    }
}

impl fmt::Display for Contact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One address: `LOCAL@DOMAIN`, with no header, no second address and no
/// escape.
fn check_address(address: &str) -> Result<(), String> {
    if address.is_empty() {
        return Err("no address".into());
    }
    if address.len() > MAX_ADDRESS {
        return Err(format!("an address has at most {MAX_ADDRESS} characters"));
    }
    if address.contains(',') {
        return Err("give one address alone: a contact has no ','".into());
    }
    if address.contains('?') {
        return Err("a contact has no header: remove '?' and the text after it".into());
    }
    let Some((local, domain)) = address.split_once('@') else {
        return Err("an address is LOCAL@DOMAIN: there is no '@'".into());
    };
    if local.is_empty() || local.len() > MAX_LOCAL {
        return Err(format!("the part before '@' has 1 to {MAX_LOCAL} characters"));
    }
    if let Some(c) = local.chars().find(|c| !c.is_ascii_alphanumeric() && !LOCAL_MARKS.contains(*c)) {
        return Err(format!("the character {c:?} is not allowed before '@'"));
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return Err("a '.' before '@' is not at an end and not next to another '.'".into());
    }
    check_domain(domain)
}

/// The domain of an address: DNS labels, two or more, the last not a number.
fn check_domain(domain: &str) -> Result<(), String> {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return Err(format!("the domain {domain:?} has no '.': give a name such as example.org"));
    }
    for label in &labels {
        let ok = (1..=63).contains(&label.len())
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !ok {
            return Err(format!(
                "{label:?} of the domain is not a DNS label: use letters, digits and '-' (1 to 63, no '-' at an end)"
            ));
        }
    }
    if labels.last().is_some_and(|top| top.bytes().all(|b| b.is_ascii_digit())) {
        return Err(format!("the domain {domain:?} is an address: give a DNS name"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mailto_uri_of_one_address_is_a_contact() {
        let contact = Contact::parse("mailto:ops@example.org").unwrap();
        assert_eq!(contact.as_str(), "mailto:ops@example.org");
        assert_eq!(contact.address(), "ops@example.org");
        assert_eq!(
            Contact::parse("MAILTO:a.b+tag@mail.example.org").unwrap().as_str(),
            "mailto:a.b+tag@mail.example.org"
        );
        assert_eq!(Contact::mailto("ops@example.org").unwrap(), contact);
        assert_eq!(contact.to_string(), "mailto:ops@example.org");
    }

    #[test]
    fn a_contact_that_a_ca_refuses_is_refused_by_name_before_the_request() {
        let refused = [
            ("tel:+12025550100", "give a mailto: URI"),
            ("ops@example.org", "give a mailto: URI"),
            ("mailto:", "no address"),
            ("mailto:a@example.org,b@example.org", "one address alone"),
            ("mailto:ops@example.org?subject=hi", "no header"),
            ("mailto:ops.example.org", "no '@'"),
            ("mailto:a@b@example.org", "not a DNS label"),
            ("mailto:o%70s@example.org", "'%'"),
            ("mailto:o ps@example.org", "' '"),
            ("mailto:.ops@example.org", "'.' before '@'"),
            ("mailto:ops@localhost", "has no '.'"),
            ("mailto:ops@-a.example.org", "not a DNS label"),
            ("mailto:ops@192.0.2.1", "is an address"),
            ("mailto:ops@exa\u{1b}mple.org", "not a DNS label"),
        ];
        for (text, why) in refused {
            let err = Contact::parse(text).unwrap_err();
            assert!(err.starts_with(&format!("the contact {text:?}: ")), "{err}");
            assert!(err.contains(why), "{text}: {err}");
            assert!(!err.contains('\u{1b}'), "a control character is escaped: {err:?}");
        }
        let long = format!("mailto:{}@example.org", "a".repeat(65));
        assert!(Contact::parse(&long).unwrap_err().contains("1 to 64"));
        let longest =
            format!("mailto:{}@{}.org", "a".repeat(64), ["b".repeat(63), "c".repeat(63), "d".repeat(57)].join("."));
        assert_eq!(longest.len() - MAILTO.len(), MAX_ADDRESS);
        assert!(Contact::parse(&longest).is_ok());
        let over = longest.replace("@b", "@bb");
        assert!(Contact::parse(&over).unwrap_err().contains("at most 254"));
        assert!(
            Contact::mailto("a,b@example.org").unwrap_err().starts_with("the e-mail address \"a,b@example.org\": ")
        );
    }
}
