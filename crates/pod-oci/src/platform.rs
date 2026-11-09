//! Which `os/arch/variant` a client asks a registry for, and how it
//! decides.
//!
//! The architecture of the host comes from the build; the platform comes at
//! run time. [`Platform::host`] is what this binary was built for, the
//! default. [`Platform::wanted`] is what the client asks for: a flag or a
//! default of the caller can change it, as a caller on an amd64 host can ask
//! for `linux/arm64` on purpose, and docker allows it.

use crate::error::{Result, usage};

/// The only OS of the client, as a value. A caller refuses an image that
/// declares `windows` or `darwin` by name.
pub const OS: &str = "linux";

/// The OCI name of the architecture of the build.
///
/// The name of Rust and the OCI name are different words, and the name of
/// the kernel too: Rust says `x86_64` for OCI's `amd64`, `aarch64` for
/// `arm64` and `x86` for `386`. A guess gives a 404 of the registry or a
/// wrong manifest.
///
/// The list maps the values of `std::env::consts::ARCH` to the names of the
/// `image-index` of the OCI image specification, which are Go's `GOARCH`
/// values. A target with no row keeps its own name: the registry then names
/// what its index offers, which is better than a wrong pull.
pub const HOST_ARCH: &str = host_arch();

const fn host_arch() -> &'static str {
    // A `match` on `std::env::consts::ARCH` cannot be `const`, so this is a
    // ladder of cfg. The compiler checks the name of each target_arch.
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
        // `ppc64le` and `ppc64` are different platforms for a registry: the
        // byte order makes the difference, not a variant.
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
/// Only `arm` and `arm64` have one. `linux/arm/v7` and `linux/arm/v6` are
/// different images; `linux/arm64/v8` and `linux/arm64` are one image,
/// written two ways. So [`Platform::matches`] takes an absent variant as a
/// match of each variant.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub variant: Option<String>,
}

impl Platform {
    /// What this binary was built for.
    pub fn host() -> Platform {
        Platform { os: OS.to_string(), arch: HOST_ARCH.to_string(), variant: HOST_VARIANT.map(str::to_string) }
    }

    /// What the client asks for: the `flag` of the caller, else the first
    /// of `defaults` with a value, else the host. Each default is a name and
    /// its value: the caller reads its variables (docker's is
    /// `DOCKER_DEFAULT_PLATFORM`) and gives them in its order. An empty value
    /// is no value. The order is docker's: the flag, then the environment,
    /// then the host.
    pub fn wanted(flag: Option<&str>, defaults: &[(&str, Option<&str>)]) -> Result<Platform> {
        if let Some(f) = flag {
            return Platform::parse(f);
        }
        for (name, value) in defaults {
            if let Some(v) = value
                && !v.trim().is_empty()
            {
                return Platform::parse(v.trim()).map_err(|e| usage(format!("${name} is not a platform: {e}")));
            }
        }
        Ok(Platform::host())
    }

    /// `os/arch`, `os/arch/variant`, or a bare `arch`.
    ///
    /// A bare word is an architecture, not an OS, as for docker:
    /// `--platform arm64` means `linux/arm64`. As an OS, it would ask for
    /// `arm64/amd64`.
    pub fn parse(s: &str) -> Result<Platform> {
        let s = s.trim();
        if s.is_empty() {
            return Err(usage("an empty --platform names nothing. Write os/arch, for example linux/arm64"));
        }
        let parts: Vec<&str> = s.split('/').collect();
        let p = match parts.as_slice() {
            [arch] => Platform { os: OS.to_string(), arch: normalise_arch(arch), variant: implied_variant(arch) },
            [os, arch] => {
                Platform { os: (*os).to_string(), arch: normalise_arch(arch), variant: implied_variant(arch) }
            }
            [os, arch, variant] => {
                Platform { os: (*os).to_string(), arch: normalise_arch(arch), variant: Some((*variant).to_string()) }
            }
            _ => {
                return Err(usage(format!(
                    "{s:?} has {} fields. A platform is os/arch or \
                     os/arch/variant, for example linux/arm64/v8",
                    parts.len()
                )));
            }
        };
        if p.arch.is_empty() || p.os.is_empty() {
            return Err(usage(format!(
                "{s:?} has an empty field. A platform is os/arch, for example \
                 linux/arm64"
            )));
        }
        Ok(p)
    }

    /// Whether an index entry answers for this platform.
    ///
    /// An absent variant on one side matches; two different variants do not.
    /// `linux/arm64` and `linux/arm64/v8` are one platform, written two ways;
    /// `linux/arm/v6` and `linux/arm/v7` are two platforms, and one run as the
    /// other is an illegal instruction, not an error message.
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
    /// False is not a refusal: `binfmt_misc` with a registered interpreter
    /// runs a foreign payload. The caller says so and checks before it runs
    /// the payload.
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
/// docker accepts each of these, and the client too: `--platform
/// linux/x86_64` says what the caller means, and a 404 that names `amd64` is
/// a worse answer.
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

/// The one case where the word of the architecture holds a variant: `armv7`
/// and `armv6` both become `arm`, and the version is the difference. With no
/// variant, an armv7 image would come to an armv6 machine.
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
        // docker's reading: `--platform arm64` means linux/arm64. As an OS,
        // the word would ask a registry for `arm64/amd64`.
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
        // Both become `arm`, and the variant is the difference. With no
        // variant, a v7 image comes to a v6 machine: an illegal instruction.
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
        // `linux/arm64` and `linux/arm64/v8` are one platform, written two
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
        // Rust says x86_64 and a registry says amd64.
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
        let defaults = [("FIRST_PLATFORM", None), ("SECOND_PLATFORM", Some("linux/s390x"))];
        assert_eq!(Platform::wanted(Some("linux/riscv64"), &defaults).unwrap().arch, "riscv64");
        assert_eq!(Platform::wanted(None, &defaults).unwrap().arch, "s390x");
        let blank = [("FIRST_PLATFORM", Some("  "))];
        assert_eq!(Platform::wanted(None, &blank).unwrap(), Platform::host());
        assert_eq!(Platform::wanted(None, &[]).unwrap(), Platform::host());
        let bad = Platform::wanted(None, &[("FIRST_PLATFORM", Some("a/b/c/d"))]).unwrap_err();
        assert!(bad.to_string().starts_with("$FIRST_PLATFORM is not a platform"), "{bad}");
        assert_eq!(bad.class, pod_core::Class::Usage);
    }
}
