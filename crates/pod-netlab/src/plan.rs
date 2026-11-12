//! The commands that build and remove a lab inside a guest that is root.
//! Each command is an argument list, never a shell string; the consumer
//! runs the steps in order and stops at the first failed step that must
//! pass. The guest needs iproute2 (`ip`, `tc`), nftables, sysctl, iputils
//! `ping`, and tayga for `--nat64`.

mod extras;
mod links;
pub mod ruleset;

use crate::lab::{Lab, LabError, Settings};
use crate::layout::Role;

/// One step of a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Run `argv[0]` (found in `PATH`) with the rest as its arguments, and
    /// give it `stdin`. With `must`, a nonzero exit fails the plan.
    Run { argv: Vec<String>, stdin: Option<String>, must: bool },
    /// Write a file of mode 0644, and make its parent directories.
    Write { path: String, contents: String },
    /// Remove a file or an empty directory; a missing one is no error.
    Remove { path: String },
    /// Send SIGTERM to the process whose id the file holds; a missing file
    /// or process is no error.
    Stop { pidfile: String },
}

impl Step {
    /// A command that must pass.
    pub fn run<S: AsRef<str>>(argv: &[S]) -> Step {
        Step::Run { argv: argv.iter().map(|a| a.as_ref().to_string()).collect(), stdin: None, must: true }
    }

    /// A command whose failure the plan ignores.
    pub fn try_run<S: AsRef<str>>(argv: &[S]) -> Step {
        match Step::run(argv) {
            Step::Run { argv, stdin, .. } => Step::Run { argv, stdin, must: false },
            other => other,
        }
    }

    fn with_stdin(self, text: String) -> Step {
        match self {
            Step::Run { argv, must, .. } => Step::Run { argv, stdin: Some(text), must },
            other => other,
        }
    }

    /// The step as one line for a log: the arguments joined by spaces, or
    /// the file operation.
    pub fn line(&self) -> String {
        match self {
            Step::Run { argv, stdin, must } => {
                let tail = if stdin.is_some() { " < (text)" } else { "" };
                let head = if *must { "" } else { "(may fail) " };
                format!("{head}{}{tail}", argv.join(" "))
            }
            Step::Write { path, .. } => format!("write {path}"),
            Step::Remove { path } => format!("remove {path}"),
            Step::Stop { pidfile } => format!("stop the process of {pidfile}"),
        }
    }
}

/// A command in the namespace of `role`.
pub(crate) fn exec(lab: &Lab, role: Role, args: &[&str]) -> Step {
    let ns = lab.namespace(role);
    let mut argv = vec!["ip", "netns", "exec", ns.as_str()];
    argv.extend_from_slice(args);
    Step::run(&argv)
}

/// `ip -n NS ARGS`.
pub(crate) fn ip(lab: &Lab, role: Role, args: &[&str]) -> Step {
    let ns = lab.namespace(role);
    let mut argv = vec!["ip", "-n", ns.as_str()];
    argv.extend_from_slice(args);
    Step::run(&argv)
}

/// The files of a lab in the guest.
pub struct Files {
    pub tayga_conf: String,
    pub tayga_pid: String,
    /// The guest's resolver, which `ip netns exec` mounts on
    /// `/etc/resolv.conf`.
    pub resolv_conf: String,
    pub resolv_dir: String,
}

impl Lab {
    pub fn files(&self, settings: &Settings) -> Files {
        let dir = settings.state_dir.trim_end_matches('/');
        let resolv_dir = format!("/etc/netns/{}", self.namespace(Role::Guest));
        Files {
            tayga_conf: format!("{dir}/{}-tayga.conf", self.stem()),
            tayga_pid: format!("{dir}/{}-tayga.pid", self.stem()),
            resolv_conf: format!("{resolv_dir}/resolv.conf"),
            resolv_dir,
        }
    }

    /// The steps that build the lab: the namespaces, the sysctls, the
    /// links, the addresses and the routes, tayga, the nftables rules, the
    /// guest's resolver, one packet along each hop (L3), and last the netem
    /// queues, so that the packet of each hop has no loss.
    pub fn up(&self, settings: &Settings) -> Result<Vec<Step>, LabError> {
        settings.check()?;
        let mut steps = Vec::new();
        links::namespaces(self, settings, &mut steps);
        links::routes(self, &mut steps);
        if self.knobs().nat64 {
            extras::tayga(self, &self.files(settings), &mut steps);
        }
        ruleset::steps(self, &mut steps);
        extras::resolver(self, &self.files(settings), &mut steps);
        links::hops(self, &mut steps);
        extras::netem(self, &mut steps);
        Ok(steps)
    }

    /// The steps that remove the lab. Each step may fail, so that a second
    /// run, or a run after a failed `up`, removes what is left.
    pub fn down(&self, settings: &Settings) -> Result<Vec<Step>, LabError> {
        settings.check()?;
        let files = self.files(settings);
        let mut steps = Vec::new();
        if self.knobs().nat64 {
            steps.push(Step::Stop { pidfile: files.tayga_pid.clone() });
        }
        for role in self.layout().roles.iter().rev() {
            steps.push(Step::try_run(&["ip", "netns", "del", &self.namespace(*role)]));
        }
        if self.knobs().nat64 {
            steps.push(Step::Remove { path: files.tayga_conf });
            steps.push(Step::Remove { path: files.tayga_pid });
        }
        steps.push(Step::Remove { path: files.resolv_conf });
        steps.push(Step::Remove { path: files.resolv_dir });
        Ok(steps)
    }
}
