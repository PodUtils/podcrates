//! The numbers of GitHub's expressions: the text of a number and the number
//! of a text. The runner keeps each number as a double, writes it with the
//! .NET format "G15" in the invariant culture, and reads a text with its
//! `ParseNumber`.

/// The text of a number: 15 significant digits, with no trailing zero; the
/// form `1.5E+15` when the exponent is 15 or more, or -5 or less.
pub(crate) fn format(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        // .NET Core writes the sign of a negative zero.
        return if n.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // Rust rounds the exact binary value to 15 digits, as .NET Core does.
    let scientific = format!("{:.14e}", n.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if n < 0.0 {
        out.push('-');
    }
    if !(-4..15).contains(&exponent) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let sign = if exponent < 0 { '-' } else { '+' };
        out.push_str(&format!("E{sign}{:02}", exponent.unsigned_abs()));
    } else if exponent >= 0 {
        let point = exponent as usize + 1;
        if digits.len() <= point {
            out.push_str(digits);
            out.extend(std::iter::repeat_n('0', point - digits.len()));
        } else {
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
        out.push_str(digits);
    }
    out
}

/// The number of a text, as the runner's `ParseNumber`: the text with no
/// white space at its ends; empty gives 0; a decimal number with a sign,
/// a point and an exponent; `0x` and 1 to 8 hex digits, or `0o` and octal
/// digits, as a 32-bit signed integer; `Infinity` and `-Infinity`; each
/// other text gives NaN.
pub(crate) fn parse(text: &str) -> f64 {
    let s = text.trim();
    if s.is_empty() {
        return 0.0;
    }
    if is_decimal(s) {
        return s.parse().unwrap_or(f64::NAN);
    }
    if let Some(hex) = s.strip_prefix("0x") {
        return int32(hex, 16);
    }
    if let Some(octal) = s.strip_prefix("0o") {
        return int32(octal, 8);
    }
    // .NET Core reads the infinity symbols with no regard to case.
    let (sign, word) = match s.as_bytes()[0] {
        b'-' => (-1.0, &s[1..]),
        b'+' => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    if word.eq_ignore_ascii_case("infinity") {
        return sign * f64::INFINITY;
    }
    f64::NAN
}

/// `[+-]? digits [. digits] [e [+-] digits]`, with at least one digit before
/// the exponent: the styles `AllowLeadingSign`, `AllowDecimalPoint` and
/// `AllowExponent` of .NET.
fn is_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == b.len()
}

/// A 32-bit signed integer from its digits, as .NET reads a hex or an octal
/// text: the bits of the value, so `ffffffff` is -1. More than 32 bits, or a
/// digit of another base, gives NaN.
fn int32(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return f64::NAN;
    }
    let digits = digits.trim_start_matches('0');
    match u32::from_str_radix(if digits.is_empty() { "0" } else { digits }, radix) {
        Ok(bits) => f64::from(bits as i32),
        Err(_) => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_of_a_number_is_g15() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.333333333333333"),
            (1e14, "100000000000000"),
            (1e15, "1E+15"),
            (123456789012345678.0, "1.23456789012346E+17"),
            (0.0001, "0.0001"),
            (0.00001, "1E-05"),
            (1.25e-7, "1.25E-07"),
            (2.71, "2.71"),
            (2.5e300, "2.5E+300"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (999999999999999.0, "999999999999999"),
            (9999999999999999.0, "1E+16"),
        ];
        for (n, text) in cases {
            assert_eq!(format(*n), *text, "{n}");
        }
    }

    #[test]
    fn the_number_of_a_text() {
        let cases: &[(&str, f64)] = &[
            ("", 0.0),
            ("  ", 0.0),
            (" 12 ", 12.0),
            ("-1.5e3", -1500.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("+7", 7.0),
            ("0x1F", 31.0),
            ("0xffffffff", -1.0),
            ("0x000000001", 1.0),
            ("0o17", 15.0),
            ("0o37777777777", -1.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
        ];
        for (text, n) in cases {
            assert_eq!(parse(text), *n, "{text:?}");
        }
        for text in ["abc", "1e", "e5", ".", "0x", "0xg", "0x100000000", "0o8", "0X1F", "-0x1", "1,000", "1 2"] {
            assert!(parse(text).is_nan(), "{text:?}");
        }
    }
}
