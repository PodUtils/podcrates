//! The quoting of a word in each syntax of a service: `ExecStart=` and
//! `Environment=` of systemd, a POSIX shell (a line of the crontab), a
//! Windows command line, and XML. The base is podssh's
//! `node_install/text.rs`; podrunner's `print-unit` and podnet's
//! `autostart.rs` quote `ExecStart=` the same way.

/// The bytes that need no quote in each syntax here.
fn plain(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-=:,@+".contains(&b))
}

/// A word as `ExecStart=` reads it: `%` and `$` doubled, as systemd expands
/// them, and in double quotes when it is not plain.
pub fn systemd_word(word: &str) -> String {
    let escaped = word.replace('%', "%%").replace('$', "$$");
    if plain(word) {
        return escaped;
    }
    format!("\"{}\"", escaped.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One assignment of `Environment=`: in double quotes, with `%` doubled, as
/// systemd expands a specifier there. systemd expands no `$` in this key.
pub fn systemd_env(key: &str, value: &str) -> String {
    let text = format!("{key}={value}").replace('%', "%%");
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One assignment of `Environment=` whose value holds a specifier of
/// systemd (`%d`, the directory of the credentials), which stays as it is.
pub(crate) fn systemd_env_spec(key: &str, value_with_spec: &str) -> String {
    format!("\"{key}={}\"", value_with_spec.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A word for a POSIX shell: as it is when plain, else in single quotes.
pub fn sh_word(word: &str) -> String {
    if plain(word) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// A word of a Windows command line, as `CommandLineToArgvW` reads it: in
/// double quotes when it holds a space, a tab or a quote, with the
/// backslashes before a quote doubled.
pub fn windows_word(word: &str) -> String {
    if !word.is_empty() && !word.contains([' ', '\t', '"']) {
        return word.to_string();
    }
    let mut quoted = String::from('"');
    let mut slashes = 0;
    for c in word.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(slashes * 2 + 1));
                quoted.push('"');
                slashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(slashes));
                quoted.push(c);
                slashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(slashes * 2));
    quoted.push('"');
    quoted
}

/// `text` for an element or an attribute of XML.
pub fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A path of a list of systemd (`ReadWritePaths=`, `BindPaths=`), which
/// splits at a space and reads `:` in a bind: letters, digits and
/// `/._-+@` alone, absolute.
pub(crate) fn list_path_ok(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with('/') && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-+@".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_syntax_quotes_what_it_must() {
        assert_eq!(sh_word("node"), "node");
        assert_eq!(sh_word("a b"), "'a b'");
        assert_eq!(sh_word("it's"), "'it'\\''s'");
        assert_eq!(sh_word(""), "''");
        assert_eq!(systemd_word("/usr/bin/podssh"), "/usr/bin/podssh");
        assert_eq!(systemd_word("a b"), "\"a b\"");
        assert_eq!(systemd_word("100%"), "\"100%%\"");
        assert_eq!(systemd_word("$HOME"), "\"$$HOME\"");
        assert_eq!(systemd_word("say \"hi\"\\"), "\"say \\\"hi\\\"\\\\\"");
        assert_eq!(windows_word("C:\\podssh.exe"), "C:\\podssh.exe");
        assert_eq!(windows_word("C:\\Program Files\\x\\"), "\"C:\\Program Files\\x\\\\\"");
        assert_eq!(windows_word("a\"b"), "\"a\\\"b\"");
        assert_eq!(windows_word(""), "\"\"");
        assert_eq!(xml("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
    }

    #[test]
    fn an_assignment_of_the_environment_doubles_a_specifier_and_keeps_a_dollar() {
        assert_eq!(systemd_env("A", "x y"), "\"A=x y\"");
        assert_eq!(systemd_env("P", "100%$HOME\"\\"), "\"P=100%%$HOME\\\"\\\\\"");
        assert_eq!(systemd_env_spec("T_FILE", "%d/t"), "\"T_FILE=%d/t\"");
    }

    #[test]
    fn a_path_of_a_list_has_no_space_and_no_colon() {
        assert!(list_path_ok(std::path::Path::new("/var/lib/pod-runner_1/a@b+c")));
        for bad in ["/a b", "/a:b", "relative", "/a\\b"] {
            assert!(!list_path_ok(std::path::Path::new(bad)), "{bad}");
        }
    }
}
