//! The flags of `sand net up`, ported unchanged: a consumer's command line
//! gives them to `Knobs::parse_flags`, and `Knobs::flags` writes a knob
//! set back as flags (a scenario as its command line).

use super::check::Contradiction;
use super::{Broken, Icmp, KnobError, Knobs, Nat, NatFlags, Ping, Policy, Stack, loss_text};

fn bad(flag: &str, why: &'static str) -> KnobError {
    KnobError::BadFlag { flag: flag.to_string(), why }
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, KnobError> {
    value.parse().map_err(|_| bad(flag, "the value is not a whole number"))
}

/// A percent with at most two decimals (`5`, `0.25`) in hundredths.
fn percent(flag: &str, value: &str) -> Result<u16, KnobError> {
    let value = value.strip_suffix('%').unwrap_or(value);
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    let digits = |s: &str| !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || frac.len() > 2 || !(frac.is_empty() || digits(frac)) {
        return Err(bad(flag, "the value is not a percent with at most two decimals"));
    }
    let whole: u32 = number(flag, whole)?;
    let frac: u32 = if frac.is_empty() { 0 } else { number::<u32>(flag, frac)? * if frac.len() == 1 { 10 } else { 1 } };
    u16::try_from(whole * 100 + frac).map_err(|_| bad(flag, "the percent is too large"))
}

fn port_list(flag: &str, value: &str) -> Result<Vec<u16>, KnobError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value.split(',').map(|p| number(flag, p)).collect()
}

fn word<T>(flag: &str, value: &str, read: fn(&str) -> Option<T>) -> Result<T, KnobError> {
    read(value).ok_or_else(|| bad(flag, "the value is not a word of this flag"))
}

impl Knobs {
    /// The knobs of sand-net's flags (`--stack v6`, `--nat both`, `--tcp
    /// 22,443`, `--no-dns`). A flag that is not given keeps its default.
    /// The result is checked (`Knobs::check`).
    pub fn parse_flags<S: AsRef<str>>(args: &[S]) -> Result<Knobs, KnobError> {
        let mut knobs = Knobs::default();
        let mut policy: Option<Policy> = None;
        let mut lists: Option<(Vec<u16>, Vec<u16>)> = None;
        let mut args = args.iter().map(AsRef::as_ref);
        while let Some(flag) = args.next() {
            let switch = match flag {
                "--no-dns" => Some(&mut knobs.dns),
                "--dnat" => Some(&mut knobs.dnat),
                "--hairpin" => Some(&mut knobs.hairpin),
                "--nat64" => Some(&mut knobs.nat64),
                "--dns64" => Some(&mut knobs.dns64),
                "--cgnat" => Some(&mut knobs.cgnat),
                _ => None,
            };
            if let Some(on) = switch {
                *on = flag != "--no-dns";
                continue;
            }
            let value = args.next().ok_or_else(|| bad(flag, "the flag needs a value"))?;
            match flag {
                "--stack" => knobs.stack = word(flag, value, Stack::from_word)?,
                "--nat" => knobs.nat = word(flag, value, Nat::from_word)?,
                "--nat-flags" => knobs.nat_flags = Some(word(flag, value, NatFlags::from_word)?),
                "--policy" => policy = Some(word(flag, value, Policy::from_word)?),
                "--tcp" => lists.get_or_insert_with(Default::default).0 = port_list(flag, value)?,
                "--udp" => lists.get_or_insert_with(Default::default).1 = port_list(flag, value)?,
                "--broken" => knobs.broken = Some(word(flag, value, Broken::from_word)?),
                "--mtu" => knobs.mtu = Some(number(flag, value)?),
                "--icmp" => knobs.icmp = word(flag, value, Icmp::from_word)?,
                "--delay" => knobs.delay_ms = Some(number(flag, value.strip_suffix("ms").unwrap_or(value))?),
                "--loss" => knobs.loss_bp = Some(percent(flag, value)?),
                "--rate" => knobs.rate_kbit = Some(number(flag, value.strip_suffix("kbit").unwrap_or(value))?),
                "--ping" => knobs.ping = word(flag, value, Ping::from_word)?,
                _ => return Err(bad(flag, "the flag is not a knob")),
            }
        }
        knobs.policy = match (policy, lists) {
            (Some(_), Some(_)) => return Err(KnobError::Contradiction(Contradiction::PolicyAndPorts)),
            (Some(p), None) => p,
            (None, Some((tcp, udp))) => Policy::Ports { tcp, udp },
            (None, None) => Policy::Open,
        };
        knobs.check()?;
        Ok(knobs)
    }

    /// The flags of this knob set; a default knob gives no flag.
    /// `Knobs::parse_flags` of the result gives the same knobs.
    pub fn flags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut pair = |flag: &str, value: String| {
            out.push(flag.to_string());
            out.push(value);
        };
        let d = Knobs::default();
        if self.stack != d.stack {
            pair("--stack", self.stack.word().into());
        }
        if self.nat != d.nat {
            pair("--nat", self.nat.word().into());
        }
        if let Some(flags) = self.nat_flags {
            pair("--nat-flags", flags.word().into());
        }
        let join = |list: &[u16]| list.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
        match &self.policy {
            Policy::Open => {}
            Policy::Ports { tcp, udp } => {
                pair("--tcp", join(tcp));
                pair("--udp", join(udp));
            }
            other => pair("--policy", other.word().unwrap_or("open").into()),
        }
        if let Some(broken) = self.broken {
            pair("--broken", broken.word().into());
        }
        if let Some(mtu) = self.mtu {
            pair("--mtu", mtu.to_string());
        }
        if self.icmp != d.icmp {
            pair("--icmp", self.icmp.word().into());
        }
        if let Some(ms) = self.delay_ms {
            pair("--delay", ms.to_string());
        }
        if let Some(bp) = self.loss_bp {
            pair("--loss", loss_text(bp).trim_end_matches('%').to_string());
        }
        if let Some(kbit) = self.rate_kbit {
            pair("--rate", kbit.to_string());
        }
        if self.ping != d.ping {
            pair("--ping", self.ping.word().into());
        }
        for (on, flag) in [
            (!self.dns, "--no-dns"),
            (self.dnat, "--dnat"),
            (self.hairpin, "--hairpin"),
            (self.nat64, "--nat64"),
            (self.dns64, "--dns64"),
            (self.cgnat, "--cgnat"),
        ] {
            if on {
                out.push(flag.to_string());
            }
        }
        out
    }
}
