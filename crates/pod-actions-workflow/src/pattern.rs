//! The patterns of the filters of `on:` (GitHub's "Filter pattern cheat
//! sheet"): `*` is any text with no `/`, `**` is any text, `**/` is zero or
//! more directories, `?` is zero or one of the character or set before it,
//! `+` is one or more of it, `[...]` is one character of a set with ranges,
//! `\` makes the next character literal, and a `!` at the start negates the
//! pattern. Each other character is literal, with case.

use crate::error::{Error, ErrorKind};

/// One pattern of a filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// The text as written, with its `!`.
    pub text: String,
    /// The pattern starts with `!`: a match removes the name.
    pub negated: bool,
    tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
enum Atom {
    Char(char),
    /// The ranges of a set; one character is a range of one.
    Set(Vec<(char, char)>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Count {
    One,
    Maybe,
    Many,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Atom(Atom, Count),
    /// `*`: any text with no `/`.
    Star,
    /// `**`: any text.
    Any,
    /// `**/` at the start or after `/`: empty, or any text that ends with
    /// `/`, so that `**/README.md` matches `README.md`.
    Dirs,
}

fn bad(text: &str, why: &'static str) -> Error {
    Error::new("", None, ErrorKind::BadPattern { pattern: text.to_string(), why })
}

/// A range must stay in one class, as GitHub allows `a-z`, `A-Z` and `0-9`.
fn same_class(a: char, b: char) -> bool {
    let class = |c: char| {
        if c.is_ascii_lowercase() {
            1
        } else if c.is_ascii_uppercase() {
            2
        } else if c.is_ascii_digit() {
            3
        } else {
            0
        }
    };
    class(a) != 0 && class(a) == class(b) && a <= b
}

impl Pattern {
    /// Read a pattern. An empty pattern, a `?` or `+` with nothing before
    /// it, a set that is not closed or empty, a bad range and a `\` at the
    /// end are errors.
    pub fn parse(text: &str) -> Result<Pattern, Error> {
        let (negated, body) = match text.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if body.is_empty() {
            return Err(bad(text, "the pattern is empty"));
        }
        let chars: Vec<char> = body.chars().collect();
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            match c {
                '*' if chars.get(i) == Some(&'*') => {
                    i += 1;
                    let at_start = tokens.is_empty() || matches!(tokens.last(), Some(Token::Atom(Atom::Char('/'), _)));
                    if at_start && chars.get(i) == Some(&'/') {
                        i += 1;
                        tokens.push(Token::Dirs);
                    } else {
                        tokens.push(Token::Any);
                    }
                }
                '*' => tokens.push(Token::Star),
                '?' | '+' => {
                    let Some(Token::Atom(_, count @ Count::One)) = tokens.last_mut() else {
                        return Err(bad(text, "`?` and `+` must follow a character or a set"));
                    };
                    *count = if c == '?' { Count::Maybe } else { Count::Many };
                }
                '[' => {
                    let (set, next) = set(&chars, i).ok_or_else(|| bad(text, "a set `[...]` is not closed"))?;
                    if set.is_empty() {
                        return Err(bad(text, "a set `[...]` is empty"));
                    }
                    if set.iter().any(|&(a, b)| a != b && !same_class(a, b)) {
                        return Err(bad(text, "a range must be in a-z, A-Z or 0-9, in order"));
                    }
                    tokens.push(Token::Atom(Atom::Set(set), Count::One));
                    i = next;
                }
                '\\' => {
                    let Some(&next) = chars.get(i) else {
                        return Err(bad(text, "a `\\` at the end escapes nothing"));
                    };
                    i += 1;
                    tokens.push(Token::Atom(Atom::Char(next), Count::One));
                }
                _ => tokens.push(Token::Atom(Atom::Char(c), Count::One)),
            }
        }
        Ok(Pattern { text: text.to_string(), negated, tokens })
    }

    /// The pattern (with no regard to its `!`) matches all of `name`.
    pub fn matches(&self, name: &str) -> bool {
        let text: Vec<char> = name.chars().collect();
        let n = text.len();
        // `next[i]`: the tokens after the current one match `text[i..]`.
        // One pass from the end for each token keeps the cost at
        // tokens * length, with no backtracking.
        let mut next = vec![false; n + 1];
        next[n] = true;
        for token in self.tokens.iter().rev() {
            let mut cur = vec![false; n + 1];
            let mut dir = false;
            for i in (0..=n).rev() {
                let c = text.get(i).copied();
                cur[i] = match token {
                    Token::Atom(atom, count) => {
                        let hit = c.is_some_and(|c| atom.matches(c));
                        match count {
                            Count::One => hit && next[i + 1],
                            Count::Maybe => next[i] || (hit && next[i + 1]),
                            Count::Many => hit && (next[i + 1] || cur[i + 1]),
                        }
                    }
                    Token::Star => next[i] || (c.is_some_and(|c| c != '/') && cur[i + 1]),
                    Token::Any => next[i] || (c.is_some() && cur[i + 1]),
                    Token::Dirs => {
                        dir = dir || (c == Some('/') && next[i + 1]);
                        next[i] || dir
                    }
                };
            }
            next = cur;
        }
        next[0]
    }
}

impl Atom {
    fn matches(&self, c: char) -> bool {
        match self {
            Atom::Char(want) => c == *want,
            Atom::Set(ranges) => ranges.iter().any(|&(a, b)| a <= c && c <= b),
        }
    }
}

/// The ranges of a set that starts at `chars[i]` (after its `[`), and the
/// place after its `]`.
fn set(chars: &[char], mut i: usize) -> Option<(Vec<(char, char)>, usize)> {
    let mut out = Vec::new();
    loop {
        let c = *chars.get(i)?;
        i += 1;
        match c {
            ']' => return Some((out, i)),
            _ if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|&e| e != ']') => {
                out.push((c, chars[i + 1]));
                i += 2;
            }
            _ => out.push((c, c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn hit(pattern: &str, name: &str) -> bool {
        Pattern::parse(pattern).unwrap().matches(name)
    }

    #[test]
    fn a_star_stops_at_a_slash_and_two_stars_do_not() {
        assert!(hit("feature/*", "feature/my-branch"));
        assert!(!hit("feature/*", "feature/beta-a/my-branch"));
        assert!(hit("feature/**", "feature/mona/the/octocat"));
        assert!(hit("*", "main") && !hit("*", "releases/v1"));
        assert!(hit("**", "all/the/branches"));
        assert!(hit("*feature", "feature") && hit("*feature", "ver-10-feature"));
    }

    #[test]
    fn a_question_mark_and_a_plus_count_the_atom_before_them() {
        assert!(hit("*.jsx?", "page.js") && hit("*.jsx?", "page.jsx"));
        assert!(!hit("*.jsx?", "page.jsxx"));
        assert!(hit("v[12].[0-9]+.[0-9]+", "v1.10.1") && hit("v[12].[0-9]+.[0-9]+", "v2.0.0"));
        assert!(!hit("v[12].[0-9]+.[0-9]+", "v3.0.0") && !hit("v[12].[0-9]+.[0-9]+", "v1..0"));
        assert!(hit("[CB]at", "Cat") && hit("[1-2]00", "200") && !hit("[CB]at", "cat"));
        assert!(hit(r"\*x", "*x") && !hit(r"\*x", "ax"));
    }

    #[test]
    fn a_bad_pattern_is_refused() {
        for text in ["", "!", "?a", "a*+", "[abc", "[]", "[z-a]", "[a-Z]", "a\\"] {
            assert!(Pattern::parse(text).is_err(), "{text}");
        }
        assert!(Pattern::parse("!a").unwrap().negated);
        assert!(!Pattern::parse("a!").unwrap().negated);
    }
}
