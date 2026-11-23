//! The exit codes of the harness. The table is `pod_core`'s: this crate
//! re-exports the class and adds no second table of codes.
//!
//! The six codes of podnet#501 are named here so that a test reads them in
//! the one table, and so that a binary prints the same table in its help.

pub use pod_core::Class;

/// One code of the contract, with its class and its meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    pub code: u8,
    pub class: Class,
    pub meaning: &'static str,
}

/// The six codes of the issue: 64 unknown or malformed name, carrier or
/// query key; 66 a missing key or identity file; 69 a carrier or listener
/// disabled by policy; 71 the permissions of an identity file; 77 a failed
/// authentication or a wrong peer; 78 a protocol or suite mismatch.
pub const CONTRACT: [Code; 6] = [
    Code { code: 64, class: Class::Usage, meaning: "unknown or malformed name, carrier or query key" },
    Code { code: 66, class: Class::NotFound, meaning: "missing key or identity file" },
    Code { code: 69, class: Class::Unavailable, meaning: "carrier or listener disabled by policy" },
    Code { code: 71, class: Class::Permission, meaning: "identity file permissions are wrong" },
    Code { code: 77, class: Class::Refused, meaning: "authentication failed or wrong peer" },
    Code { code: 78, class: Class::Suite, meaning: "protocol or suite mismatch" },
];

/// The class of a code from `Class::ALL`, if the table holds it. A code
/// that two classes share (78) gives the first class of the table.
pub fn class_of(code: u8) -> Option<Class> {
    Class::ALL.into_iter().find(|class| class.exit_code() == code)
}

/// The code of a class, from the table of `pod_core`.
pub fn code_of(class: Class) -> u8 {
    class.exit_code()
}

/// The table of the six codes, one line for each, for the help of a binary.
pub fn table() -> String {
    let mut out = String::from("code class       meaning\n");
    for entry in CONTRACT {
        out.push_str(&format!("{:<4} {:<11} {}\n", entry.code, entry.class.name(), entry.meaning));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_codes_are_in_the_one_table_and_are_distinct() {
        let mut codes = std::collections::HashSet::new();
        for entry in CONTRACT {
            assert!(Class::ALL.contains(&entry.class), "{:?}", entry.class);
            assert_eq!(entry.class.exit_code(), entry.code, "{:?}", entry.class);
            assert_eq!(class_of(entry.code).map(|c| c.exit_code()), Some(entry.code));
            assert!(codes.insert(entry.code), "the code {} is twice", entry.code);
        }
        assert_eq!(codes.len(), 6);
        assert_eq!(CONTRACT.iter().map(|c| c.code).collect::<Vec<_>>(), vec![64, 66, 69, 71, 77, 78]);
    }

    #[test]
    fn the_help_table_names_each_code() {
        let text = table();
        for entry in CONTRACT {
            assert!(text.contains(&entry.code.to_string()), "{text}");
            assert!(text.contains(entry.class.name()), "{text}");
            assert!(text.contains(entry.meaning), "{text}");
        }
    }

    #[test]
    fn the_codes_stay_distinct() {
        assert_ne!(CONTRACT[0].code, CONTRACT[1].code);
        assert_ne!(CONTRACT[4].class, CONTRACT[5].class);
        assert_eq!(code_of(Class::Suite), 78);
        assert_eq!(class_of(65), None);
    }
}
