//! The exit codes of `podlab` and the environment variables that it reads
//! or sets. [`facts`] (feature `manual`) is the one table of them: the help
//! and the manual of the binary read it, and the tests check it against the
//! code.

/// `podlab check`: each part holds.
pub const EXIT_HOLDS: i32 = 0;
/// `podlab check`: a part does not hold.
pub const EXIT_FAILS: i32 = 1;
/// A usage error of the command line.
pub const EXIT_USAGE: i32 = 2;
/// `podlab check`: no part fails, and a part cannot be observed here.
pub const EXIT_UNSEEN: i32 = 3;
/// `podlab run`: the command ended in a way that `--expect-exit` or
/// `--expect-stderr` does not allow.
pub const EXIT_UNEXPECTED: i32 = 4;
/// `podlab run`: the lab does not hold; the command did not run.
pub const EXIT_REFUSED: i32 = 125;
/// `podlab run`: the command was found and did not start.
pub const EXIT_CANNOT_EXEC: i32 = 126;
/// `podlab run`: the command was not found.
pub const EXIT_NOT_FOUND: i32 = 127;

/// The exit codes and the variables of `podlab`, as facts of its manual.
#[cfg(feature = "manual")]
pub fn facts() -> pod_manual::Facts {
    use pod_manual::{Code, Exit, Var, VarGroup};
    let exits = vec![
        Exit::new(
            EXIT_HOLDS,
            "holds",
            "check: each part holds. run: the command exited 0, or it ended as --expect-exit and --expect-stderr say.",
        ),
        Exit::new(EXIT_FAILS, "fails", "check: a part does not hold."),
        Exit::new(EXIT_USAGE, "usage", "A usage error of the command line."),
        Exit::new(EXIT_UNSEEN, "unseen", "check: no part fails, and a part cannot be observed here."),
        Exit::new(
            EXIT_UNEXPECTED,
            "unexpected",
            "run: the command ended in a way that --expect-exit or --expect-stderr does not allow.",
        ),
        Exit::new(EXIT_REFUSED, "refused", "run: a part of the lab does not hold. The command did not run."),
        Exit::new(EXIT_CANNOT_EXEC, "cannot-exec", "run: the command did not start."),
        Exit::new(EXIT_NOT_FOUND, "not-found", "run: the command was not found."),
        Exit { code: Code::Signal, name: "signal".into(), meaning: "run: the command ended on signal N.".into() },
        Exit {
            code: Code::Payload,
            name: "command".into(),
            meaning: "run: the exit code of the command, with no --expect-exit or --expect-stderr.".into(),
        },
    ];
    let lab = VarGroup {
        name: "The lab".into(),
        vars: vec![
            Var {
                used_by: "run".into(),
                usage: pod_manual::Use::ReadsAndSets,
                ..Var::sets(
                    &["TMPDIR"],
                    "The lab writes the files of the ids there. It is /tmp for the command, where a part puts a tmpfs on /tmp.",
                )
            },
            Var { used_by: "run".into(), ..Var::reads(&["HOME"], "The directory that noexec-home binds with noexec.") },
        ],
    };
    let net = VarGroup {
        name: "A network lab".into(),
        vars: vec![
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"],
                    "The stand-in proxy of the lab, where the class has one. Else the lab removes them.",
                )
            },
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &["ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"],
                    "The lab removes them: the command reaches out only as the class lets it.",
                )
            },
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &[
                        "GLOBAL_AGENT_HTTP_PROXY",
                        "GLOBAL_AGENT_HTTPS_PROXY",
                        "npm_config_proxy",
                        "npm_config_https_proxy",
                        "NPM_CONFIG_PROXY",
                        "NPM_CONFIG_HTTPS_PROXY",
                        "YARN_HTTP_PROXY",
                        "YARN_HTTPS_PROXY",
                        "DOCKER_HTTP_PROXY",
                        "DOCKER_HTTPS_PROXY",
                    ],
                    "The proxy of a tool other than curl. The lab sets them to its stand-in proxy where the class has one. Else the lab removes them.",
                )
            },
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &["GLOBAL_AGENT_NO_PROXY", "npm_config_noproxy", "NPM_CONFIG_NOPROXY", "DOCKER_NO_PROXY"],
                    "The lab removes them.",
                )
            },
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &[
                        "CLOUDSDK_PROXY_TYPE",
                        "CLOUDSDK_PROXY_ADDRESS",
                        "CLOUDSDK_PROXY_PORT",
                        "CLOUDSDK_PROXY_USERNAME",
                        "CLOUDSDK_PROXY_PASSWORD",
                        "CLOUDSDK_PROXY_RDNS",
                    ],
                    "The proxy of the Google Cloud CLI. The lab sets the type, the address, the port and the credential of its stand-in proxy where the class has one. Else the lab removes them.",
                )
            },
            Var {
                used_by: "run".into(),
                usage: pod_manual::Use::ReadsAndSets,
                ..Var::sets(
                    &["JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS", "_JAVA_OPTIONS"],
                    "The options of a Java virtual machine. The lab removes each option of a proxy from them, and keeps the other options.",
                )
            },
            Var {
                used_by: "run".into(),
                ..Var::sets(
                    &["SSL_CERT_FILE"],
                    "The certificate file of the lab: the CA of the stand-in servers, and the CA of the proxy where the proxy terminates TLS.",
                )
            },
        ],
    };
    pod_manual::Facts {
        variables: vec![lab, net, pod_manual::pager::variables()],
        exits,
        ..pod_manual::Facts::default()
    }
}
