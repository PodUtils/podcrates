//! The help and the manual of a command line from one model.
//!
//! A program fills a [`Manual`]: its commands with their arguments, flags
//! (with a kind and a status against another tool) and rules; its topics;
//! its facts (the variables in groups, the exit codes, the files, the keys
//! of a configuration file, its own tables) and its examples. [`clap::read`]
//! (feature `clap`) fills the commands of a clap program. Each writer walks
//! the same model:
//!
//! * [`text`]: plain text that needs nothing of the host;
//! * [`roff`]: one man(7) page that groff and mandoc render;
//! * [`json`]: one JSON object, and its reader;
//! * [`llms`]: `llms.txt` and `llms-full.txt` from the JSON form;
//! * [`help`]: the help of each command.
//!
//! [`select`] serves `CLI man [SECTION] [--no-pager | --roff | --json]`
//! through the [`pager`]. [`check`] holds the mechanical checks that a
//! program calls from its own tests: the text rules ([`rules`]), a
//! generated file against the model, the variables and the exit codes
//! against the code, and the flags against the parser. [`error::fault`]
//! gives the form of an error: `OPTION: what is wrong; fix: what to do`.
//!
//! The crate prints nothing and does not exit. Only the pager reads the
//! environment (`PAGER`, `TERM`, `PATH`, `LINES`, `LESS`), when the program
//! asks it to page; [`pager::variables`] gives these as facts.

pub mod check;
#[cfg(feature = "clap")]
pub mod clap;
pub mod error;
pub mod facts;
pub mod help;
pub mod json;
pub mod llms;
pub mod model;
pub mod pager;
pub mod program;
pub mod roff;
pub mod rules;
pub mod scan;
pub mod sections;
pub mod select;
pub mod text;

pub use facts::{Code, ConfigFile, ConfigKey, Exit, Facts, FileFact, Table, Use, Var, VarGroup};
pub use model::{
    Arg, Availability, Block, Command, Example, Flag, FlagKind, Group, Manual, Parity, Rule, Span, Status, Topic,
};
pub use select::{Request, body, run};

#[cfg(test)]
mod sample;
