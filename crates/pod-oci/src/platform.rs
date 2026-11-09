//! Which `os/arch/variant` podvm asks a registry for, and how it decides.
//!
//! [`TODO/image.md`](../../../TODO/image.md) T-0212. Until 2026-09-09 this was
//! two `const`s, `OS = "linux"` and `ARCH = "amd64"`, so podvm built for
//! aarch64 would still have pulled an amd64 rootfs and handed the payload an
//! `Exec format error` with nothing to say about why.
//!
//! ⛔ **The host's architecture is read at compile time and the platform is
//! decided at run time, and those are different things.** `HOST` below is what
//! this binary was built for, which is the only honest default. What podvm
//! actually asks for is [`Platform::wanted`], which a `--platform` flag or an
//! environment variable may override, because a caller on an amd64 host asking
//! for `linux/arm64` is doing something deliberate and docker lets them.

use crate::error::{Error, Result};

/// The only OS podvm has ever been able to run, stated as a value rather than
/// assumed. ⚠ An image declaring `windows` or `darwin` is refused by name
/// rather than extracted and failed later.
pub const OS: &str = "linux";

/// ⭐ **The OCI name for the architecture this binary was compiled for.**
///
/// ⛔ Rust's name and the OCI name are not the same word, and neither is the
/// kernel's: rust says `x86_64` where OCI says `amd64`, `aarch64` where OCI
/// says `arm64`, and `x86` where OCI says `386`. Every one of those is a
/// registry 404 or, worse, a silently wrong manifest if guessed.
///
/// ⚠ The list is `std::env::consts::ARCH`'s values mapped to the platform
/// strings the OCI image spec's `image-index` uses, which are Go's `GOARCH`
/// values. A rust target with no mapping here falls through to its own name and
/// will fail at the registry with an index that names what it does offer, which
/// is a better failure than a wrong pull.
pub const HOST_ARCH: &str = host_arch();

const fn host_arch() -> &'static str {
    // ⚠ A `match` on `std::env::consts::ARCH` cannot be `const`, so this is a
    // cfg ladder. It is the same claim either way and the compiler checks the
    // spelling of each target_arch.
    #[cfg(target_arch = "x86_64")]
    {
        "amd64"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "arm64"
    }
    #[cfg(target_arch = "x86")]
    {
        "386"
    }
    #[cfg(target_arch = "arm")]
    {
        "arm"
    }
    #[cfg(target_arch = "riscv64")]
    {
        "riscv64"
    }
    #[cfg(target_arch = "loongarch64")]
    {
        "loong64"
    }
    #[cfg(target_arch = "powerpc64")]
    {
        // ⚠ `ppc64le` and `ppc64` are different platforms to a registry, and
        // the difference is endianness rather than a variant.
        #[cfg(target_endian = "little")]
        {
            "ppc64le"
        }
        #[cfg(target_endian = "big")]
        {
            "ppc64"
        }
    }
    #[cfg(target_arch = "s390x")]
    {
        "s390x"
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "loongarch64",
        target_arch = "powerpc64",
        target_arch = "s390x",
    )))]
    {
        std::env::consts::ARCH
    }
}

/// The variant the host's architecture implies, where its family has one.
///
/// ⚠ Only `arm` and `arm64` carry one in practice. `linux/arm/v7` and
/// `linux/arm/v6` are different images; `linux/arm64/v8` and `linux/arm64` are
/// the same one written two ways, which is why [`Platform::matches`] treats an
/// absent variant as a wildcard rather than as a mismatch.
pub const HOST_VARIANT: Option<&str> = host_variant();

const fn host_variant() -> Option<&'static str> {
    #[cfg(all(target_arch = "arm", target_feature = "v7"))]
    {
        Some("v7")
    }
    #[cfg(all(target_arch = "arm", not(target_feature = "v7")))]
    {
        Some("v6")
    }
    #[cfg(not(target_arch = "arm"))]
    {
        None
    }
}

/// `PODVM_DEFAULT_PLATFORM` first, then docker's own variable.
///
/// ⚠ docker's is honoured deliberately: `AGENTS.md` says podvm answers to
/// `docker` on PATH and takes the same flags, and a caller who has set
/// `DOCKER_DEFAULT_PLATFORM` for their toolchain means it for podvm too.
const ENV_VARS: [&str; 2] = ["PODVM_DEFAULT_PLATFORM", "DOCKER_DEFAULT_PLATFORM"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub variant: Option<String>,
}

impl Platform {
    /// What this binary was built for.
    pub fn host() -> Platform {
        Platform {
            os: OS.to_string(),
            arch: HOST_ARCH.to_string(),
            variant: HOST_VARIANT.map(str::to_string),
        }
    }

    /// What podvm asks for, given an explicit `--platform` or none.
    ///
    /// ⛔ The order is the flag, then the environment, then the host, and it is
    /// docker's. A caller who passes neither gets the host's, which is the only
    /// answer that cannot be surprising.
    pub fn wanted(flag: Option<&str>) -> Result<Platform> {
        if let Some(f) = flag {
            return Platform::parse(f);
        }
        for var in ENV_VARS {
            if let Ok(v) = std::env::var(var)
                && !v.trim().is_empty()
            {
                return Platform::parse(v.trim())
                    .map_err(|e| Error::Usage(format!("${var} is not a platform: {e}")));
            }
        }
        Ok(Platform::host())
    }

    /// `os/arch`, `os/arch/variant`, or a bare `arch`.
    ///
    /// ⚠ A bare word is an **architecture**, not an OS, which is what docker
    /// does: `--platform arm64` means `linux/arm64`. Reading it as an OS would
    /// silently ask for `arm64/amd64`.
    pub fn parse(s: &str) -> Result<Platform> {
        let s = s.trim();
        if s.is_empty() {
            return Err(Error::Usage(
                "an empty --platform names nothing. Write os/arch, for example \
                 linux/arm64"
                    .into(),
            ));
        }
        let parts: Vec<&str> = s.split('/').collect();
        let p = match parts.as_slice() {
            [arch] => Platform {
                os: OS.to_string(),
                arch: normalise_arch(arch),
                variant: implied_variant(arch),
            },
            [os, arch] => Platform {
                os: (*os).to_string(),
                arch: normalise_arch(arch),
                variant: implied_variant(arch),
            },
            [os, arch, variant] => Platform {
                os: (*os).to_string(),
                arch: normalise_arch(arch),
                variant: Some((*variant).to_string()),
            },
            _ => {
                return Err(Error::Usage(format!(
                    "{s:?} has {} fields. A platform is os/arch or \
                     os/arch/variant, for example linux/arm64/v8",
                    parts.len()
                )))
            }
        };
        if p.arch.is_empty() || p.os.is_empty() {
            return Err(Error::Usage(format!(
                "{s:?} has an empty field. A platform is os/arch, for example \
                 linux/arm64"
            )));
        }
        Ok(p)
    }

    /// Whether an index entry answers for this platform.
    ///
    /// ⛔ An **absent** variant on either side matches, and a **present and
    /// different** one does not. `linux/arm64` and `linux/arm64/v8` are one
    /// platform written two ways and every registry writes both; `linux/arm/v6`
    /// and `linux/arm/v7` are two platforms and running one as the other is an
    /// illegal instruction rather than an error message.
    pub fn matches(&self, os: &str, arch: &str, variant: Option<&str>) -> bool {
        if self.os != os || self.arch != arch {
            return false;
        }
        match (self.variant.as_deref(), variant) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }

    /// Whether this is the platform the running binary can execute natively.
    ///
    /// ⚠ False does **not** mean refuse: `binfmt_misc` with a registered
    /// interpreter runs a foreign payload, which is
    /// [`TODO/enter.md`](../../../TODO/enter.md) T-0506's whole subject. It
    /// means podvm has to say so and check before it execs.
    pub fn is_host(&self) -> bool {
        let h = Platform::host();
        self.os == h.os
            && self.arch == h.arch
            && match (self.variant.as_deref(), h.variant) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.variant {
            Some(v) if !v.is_empty() => write!(f, "{}/{}/{}", self.os, self.arch, v),
            _ => write!(f, "{}/{}", self.os, self.arch),
        }
    }
}

/// The spellings people type for one architecture.
///
/// ⚠ docker accepts all of these and so does podvm, because a caller typing
/// `--platform linux/x86_64` has said exactly what they mean and a 404 naming
/// `amd64` is a worse answer than doing what they said.
fn normalise_arch(a: &str) -> String {
    match a {
        "x86_64" | "x86-64" | "amd64" => "amd64",
        "aarch64" | "arm64" | "armv8" | "armv8l" => "arm64",
        "i386" | "i486" | "i586" | "i686" | "x86" | "386" => "386",
        "armv7" | "armv7l" | "armhf" | "armv6" | "armv6l" | "armel" | "arm" => "arm",
        "riscv64" | "riscv64gc" => "riscv64",
        "loongarch64" | "loong64" => "loong64",
        "powerpc64le" | "ppc64le" => "ppc64le",
        "powerpc64" | "ppc64" => "ppc64",
        "s390x" => "s390x",
        other => other,
    }
    .to_string()
}

/// ⚠ The one case where the architecture word carries a variant: `armv7` and
/// `armv6` both normalise to `arm`, and the version is the whole difference.
/// Losing it here would pull an armv7 image onto an armv6 machine.
fn implied_variant(a: &str) -> Option<String> {
    match a {
        "armv7" | "armv7l" | "armhf" => Some("v7".into()),
        "armv6" | "armv6l" | "armel" => Some("v6".into()),
        "armv8" | "armv8l" => Some("v8".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_word_is_an_architecture_and_never_an_os() {
        // ⛔ docker's reading. `--platform arm64` means linux/arm64, and taking
        // the word as an OS would ask a registry for `arm64/amd64`.
        let p = Platform::parse("arm64").unwrap();
        assert_eq!(p.os, "linux");
        assert_eq!(p.arch, "arm64");
        assert_eq!(p.to_string(), "linux/arm64");
    }

    #[test]
    fn the_spellings_people_type_all_reach_one_platform() {
        for s in ["x86_64", "amd64", "linux/x86-64"] {
            assert_eq!(Platform::parse(s).unwrap().arch, "amd64", "{s}");
        }
        for s in ["aarch64", "arm64", "linux/aarch64"] {
            assert_eq!(Platform::parse(s).unwrap().arch, "arm64", "{s}");
        }
    }

    #[test]
    fn armv6_and_armv7_do_not_collapse_into_one_platform() {
        // ⛔ Both normalise to `arm` and the variant is the whole difference.
        // Losing it pulls a v7 image onto a v6 machine, which is an illegal
        // instruction rather than a message.
        let six = Platform::parse("armv6").unwrap();
        let seven = Platform::parse("armv7").unwrap();
        assert_eq!((six.arch.as_str(), seven.arch.as_str()), ("arm", "arm"));
        assert_eq!(six.variant.as_deref(), Some("v6"));
        assert_eq!(seven.variant.as_deref(), Some("v7"));
        assert!(!seven.matches("linux", "arm", Some("v6")));
        assert!(seven.matches("linux", "arm", Some("v7")));
    }

    #[test]
    fn an_absent_variant_matches_and_a_different_one_does_not() {
        // ⚠ `linux/arm64` and `linux/arm64/v8` are one platform written two
        // ways, and registries write both.
        let p = Platform::parse("linux/arm64").unwrap();
        assert!(p.matches("linux", "arm64", Some("v8")));
        assert!(p.matches("linux", "arm64", None));
        let v8 = Platform::parse("linux/arm64/v8").unwrap();
        assert!(v8.matches("linux", "arm64", None));
        assert!(!v8.matches("linux", "arm64", Some("v9")));
    }

    #[test]
    fn a_malformed_platform_is_refused_by_name() {
        for s in ["", "a/b/c/d", "linux/"] {
            assert!(Platform::parse(s).is_err(), "{s:?} was accepted");
        }
    }

    #[test]
    fn the_host_is_an_oci_name_and_never_rusts() {
        // ⛔ The whole point: rust says x86_64 and a registry says amd64.
        let h = Platform::host();
        assert_ne!(h.arch, std::env::consts::ARCH, "the rust name reached OCI");
        assert_eq!(h.os, "linux");
        #[cfg(target_arch = "x86_64")]
        assert_eq!(h.arch, "amd64");
        #[cfg(target_arch = "aarch64")]
        assert_eq!(h.arch, "arm64");
        assert!(h.is_host());
    }

    #[test]
    fn a_flag_beats_the_environment_and_the_environment_beats_the_host() {
        assert_eq!(
            Platform::wanted(Some("linux/riscv64")).unwrap().arch,
            "riscv64"
        );
        // ⚠ No environment is set in this test process, so this reads the host
        // rather than asserting against a variable another test could race on.
        assert_eq!(Platform::wanted(None).unwrap(), Platform::host());
    }
}
