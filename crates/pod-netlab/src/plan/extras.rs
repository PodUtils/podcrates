//! tayga for `--nat64`, the guest's resolver, and the netem queues of the
//! router.

use super::{Files, Step, exec, ip};
use crate::knobs::{Family, loss_text};
use crate::lab::Lab;
use crate::layout::{
    Layout, NAT64_DEVICE, NAT64_POOL, NAT64_POOL_LEN, NAT64_PREFIX, NAT64_PREFIX_LEN, NAT64_TAYGA_V4, Role,
};

/// tayga's configuration. No `data-dir`: the lab keeps no mapping past its
/// life.
pub fn tayga_conf() -> String {
    format!(
        "tun-device {NAT64_DEVICE}\nipv4-addr {NAT64_TAYGA_V4}\nprefix {NAT64_PREFIX}/{NAT64_PREFIX_LEN}\n\
         dynamic-pool {NAT64_POOL}/{NAT64_POOL_LEN}\n"
    )
}

/// tayga in the router, as its documents set it up: the tun device, the
/// router's addresses on it, the routes of the pool and of the prefix, and
/// the daemon with its pid file. The lab starts no service of a package
/// (F19).
pub(super) fn tayga(lab: &Lab, files: &Files, steps: &mut Vec<Step>) {
    steps.push(Step::Write { path: files.tayga_conf.clone(), contents: tayga_conf() });
    steps.push(exec(lab, Role::Router, &["tayga", "--mktun", "--config", &files.tayga_conf]));
    steps.push(ip(lab, Role::Router, &["link", "set", NAT64_DEVICE, "up"]));
    let out4 = format!("{}/32", lab.layout().router_out(Family::V4));
    steps.push(ip(lab, Role::Router, &["addr", "add", &out4, "dev", NAT64_DEVICE]));
    let in6 = format!("{}/128", Layout::router_in(Family::V6));
    steps.push(ip(lab, Role::Router, &["addr", "add", &in6, "dev", NAT64_DEVICE, "nodad"]));
    let pool = format!("{NAT64_POOL}/{NAT64_POOL_LEN}");
    steps.push(ip(lab, Role::Router, &["-4", "route", "add", &pool, "dev", NAT64_DEVICE]));
    let prefix = format!("{NAT64_PREFIX}/{NAT64_PREFIX_LEN}");
    steps.push(ip(lab, Role::Router, &["-6", "route", "add", &prefix, "dev", NAT64_DEVICE]));
    steps.push(exec(lab, Role::Router, &["tayga", "--config", &files.tayga_conf, "--pidfile", &files.tayga_pid]));
}

/// The guest's resolver: the world's DNS on each family of the stack, IPv4
/// first.
pub fn resolv_conf(lab: &Lab) -> String {
    let mut text = String::new();
    for family in Family::BOTH.into_iter().filter(|f| lab.knobs().stack.has(*f)) {
        text.push_str(&format!("nameserver {}\n", Layout::world(family)));
    }
    text.push_str("options timeout:2 attempts:2\n");
    text
}

pub(super) fn resolver(lab: &Lab, files: &Files, steps: &mut Vec<Step>) {
    steps.push(Step::Write { path: files.resolv_conf.clone(), contents: resolv_conf(lab) });
}

/// The arguments of netem after `netem`: the delay, the loss and the rate
/// of each direction.
pub fn netem_args(lab: &Lab) -> Vec<String> {
    let knobs = lab.knobs();
    let mut args = Vec::new();
    if let Some(ms) = knobs.delay_ms {
        args.extend(["delay".to_string(), format!("{ms}ms")]);
    }
    if let Some(bp) = knobs.loss_bp {
        args.extend(["loss".to_string(), loss_text(bp)]);
    }
    if let Some(kbit) = knobs.rate_kbit {
        args.extend(["rate".to_string(), format!("{kbit}kbit")]);
    }
    args
}

/// netem on the router's two links, so that each direction has the delay,
/// the loss and the rate.
pub(super) fn netem(lab: &Lab, steps: &mut Vec<Step>) {
    if !lab.knobs().impaired() {
        return;
    }
    let ns = lab.namespace(Role::Router);
    let args = netem_args(lab);
    for dev in ["rg", lab.layout().outside] {
        let mut argv: Vec<&str> = vec!["tc", "-n", &ns, "qdisc", "add", "dev", dev, "root", "netem"];
        argv.extend(args.iter().map(String::as_str));
        steps.push(Step::run(&argv));
    }
}
