//! A stub of the flatten: the tests run against it first.
use std::io::Write;

use pod_core::Report;

use crate::Layer;
use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub report: Report,
}

#[derive(Debug, Clone, Default)]
pub struct Flattened {
    pub layers: usize,
    pub entries: u64,
    pub written: u64,
    pub hidden: u64,
    pub whiteouts: u64,
    pub skipped: u64,
    pub skipped_kinds: Vec<String>,
    pub bytes: u64,
}

pub fn flatten<W: Write>(_layers: &[Layer], _out: &mut W, _options: &Options) -> Result<Flattened> {
    Err(crate::error::unsupported("the stub writes no stream"))
}
