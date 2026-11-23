//! The command line of the harness: load a fixture directory, run the
//! classifiers, and print the table. It prints no pass count.

use pod_harness::date;
use pod_harness::exit::{Class, code_of, table};
use pod_harness::fixture::Fixtures;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: pod-harness --data DIR [--json]\n       pod-harness --archive DIR [--json]\n";

fn main() -> ExitCode {
    let mut dir: Option<PathBuf> = None;
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" | "--archive" => match args.next() {
                Some(value) => dir = Some(PathBuf::from(value)),
                None => {
                    eprint!("{USAGE}");
                    return ExitCode::from(code_of(Class::Usage));
                }
            },
            "--json" => json = true,
            "--help" | "-h" => {
                print!("{USAGE}\nThe exit codes of this program:\n{}", table());
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("pod-harness: unknown argument {other:?}\n{USAGE}");
                return ExitCode::from(code_of(Class::Usage));
            }
        }
    }
    let Some(dir) = dir else {
        eprint!("{USAGE}");
        return ExitCode::from(code_of(Class::Usage));
    };
    let fixtures = match Fixtures::load(&dir) {
        Ok(fixtures) => fixtures,
        Err(e) => {
            eprintln!("pod-harness: {dir}: {e}", dir = dir.display());
            return ExitCode::from(code_of(Class::Config));
        }
    };
    let date = date::today();
    let run = match fixtures.run(&date) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("pod-harness: {dir}: {e}", dir = dir.display());
            return ExitCode::from(code_of(Class::Config));
        }
    };
    if json {
        println!("{}", run.table.json());
    } else {
        print!("{}", run.table.render());
    }
    let seconds = run.elapsed.as_secs_f64();
    eprintln!("pod-harness: {seconds:.3} s of a budget of {} s", pod_harness::BUDGET_SECS);
    if seconds > pod_harness::BUDGET_SECS as f64 {
        eprintln!("pod-harness: the run is over its budget");
        return ExitCode::from(code_of(Class::Unavailable));
    }
    ExitCode::SUCCESS
}
