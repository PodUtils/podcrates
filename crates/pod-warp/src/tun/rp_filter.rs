//! The reverse path filter of a TUN (PodUtils/podnet@T-072).
//!
//! On a host with systemd, each new interface can get `rp_filter` 2 (loose)
//! from systemd-sysctl. When the only egress is a loopback proxy and the
//! WAN routes are in their own tables, the filter drops the replies that
//! come in on the TUN. [`super::run`] writes 0 for its interface at each
//! session, and again 2 s later (udev can run after the create). The kernel
//! uses the larger value of `conf.all` and `conf.<interface>`: an event
//! names a value that stays above 0. With [`RpFilter::keep`] (podnet's
//! `--keep-rp-filter`), the value of the host stays.

use std::path::{Path, PathBuf};

use pod_core::{Event, Report};

/// The directory of the IPv4 settings of each interface.
pub const SYSTEM_ROOT: &str = "/proc/sys/net/ipv4/conf";

/// Where the filter is, and whether to keep the value of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpFilter {
    /// The directory of `all/rp_filter` and `INTERFACE/rp_filter`.
    pub root: PathBuf,
    /// Write nothing; read and tell only.
    pub keep: bool,
}

impl RpFilter {
    /// The filter of this host ([`SYSTEM_ROOT`]).
    pub fn system(keep: bool) -> RpFilter {
        RpFilter { root: PathBuf::from(SYSTEM_ROOT), keep }
    }

    /// Write 0 for `interface` (unless [`RpFilter::keep`]), and give the
    /// value that the kernel uses, with the event `warp.tun.rp-filter` when
    /// it is not 0.
    pub fn apply(&self, interface: &str, report: &Report) -> Option<u32> {
        apply_in(&self.root, interface, self.keep, report)
    }
}

/// The value that the kernel uses: the larger of `conf.all` and
/// `conf.<interface>`.
pub fn effective(all: u32, interface: u32) -> u32 {
    all.max(interface)
}

fn file(root: &Path, conf: &str) -> PathBuf {
    root.join(conf).join("rp_filter")
}

fn read(root: &Path, conf: &str) -> Option<u32> {
    std::fs::read_to_string(file(root, conf)).ok()?.trim().parse().ok()
}

fn apply_in(root: &Path, interface: &str, keep: bool, report: &Report) -> Option<u32> {
    if !keep
        && read(root, interface) != Some(0)
        && let Err(err) = std::fs::write(file(root, interface), "0\n")
    {
        let text = format!("rp_filter of {interface} cannot be set to 0: {err}");
        report.event(Event::warn("warp.tun.rp-filter", text));
    }
    let all = read(root, "all")?;
    let own = read(root, interface)?;
    let value = effective(all, own);
    if value != 0 {
        let kept = if keep { " (the value of the interface is kept)" } else { "" };
        let text = format!(
            "the reverse path filter of {interface} is on (all {all}, the interface {own}): the kernel can drop the replies; write 0 to net.ipv4.conf.all.rp_filter{kept}"
        );
        report.event(Event::warn("warp.tun.rp-filter", text));
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_effective_rp_filter_is_the_larger_value() {
        assert_eq!(effective(0, 0), 0);
        assert_eq!(effective(0, 2), 2);
        assert_eq!(effective(1, 0), 1);
        assert_eq!(effective(2, 1), 2);
    }

    #[test]
    fn warp_tun_writes_0_unless_it_keeps_the_value() {
        let root = crate::test_dir("rp-filter");
        let report = Report::default();
        for (conf, value) in [("all", "0"), ("podnet0", "2")] {
            std::fs::create_dir_all(root.join(conf)).unwrap();
            std::fs::write(file(&root, conf), value).unwrap();
        }
        assert_eq!(apply_in(&root, "podnet0", true, &report), Some(2));
        assert_eq!(read(&root, "podnet0"), Some(2));
        assert_eq!(RpFilter { root: root.clone(), keep: false }.apply("podnet0", &report), Some(0));
        assert_eq!(read(&root, "podnet0"), Some(0));
        // conf.all wins over the interface.
        std::fs::write(file(&root, "all"), "1").unwrap();
        assert_eq!(apply_in(&root, "podnet0", false, &report), Some(1));
        // No file: no value.
        assert_eq!(apply_in(&root, "none", false, &report), None);
    }
}
