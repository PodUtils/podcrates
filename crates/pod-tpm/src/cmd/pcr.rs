//! The commands of the PCRs: `TPM2_PCR_Read` (part 3, clause 22.4) and
//! `TPM2_PCR_Extend` (22.2).

use super::{CC_PCR_EXTEND, CC_PCR_READ};
use crate::alg::ALG_SHA256;
use crate::error::Error;
use crate::transport::Reply;
use crate::wire::{Auth, In, MAX_DIGEST, Out, command};

/// The most banks in a `TPML_PCR_SELECTION` (`HASH_COUNT`, bounded here as
/// part 2 bounds the list).
pub const MAX_BANKS: usize = 16;
/// The most digests in a `TPML_DIGEST` (part 2, clause 10.9.3).
pub const MAX_VALUES: usize = 8;
/// The highest PCR index that a command takes.
pub const MAX_PCR: u32 = 31;

/// `TPM2_PCR_Read` of `selection`, a marshalled `TPML_PCR_SELECTION`.
pub fn pcr_read(selection: &[u8]) -> Result<Vec<u8>, Error> {
    command(CC_PCR_READ, &[], &[], selection)
}

/// `TPM2_PCR_Extend` of PCR `index` in the SHA-256 bank with `digest`,
/// under the empty password of the PCR: a test changes the boot state with
/// it.
pub fn pcr_extend(index: u32, digest: &[u8; 32]) -> Result<Vec<u8>, Error> {
    if index > MAX_PCR {
        return Err(Error::Usage(format!("PCR {index}: the PCRs are 0 to {MAX_PCR}")));
    }
    let mut params = Out::default();
    params.u32(1).u16(ALG_SHA256).raw(digest);
    command(CC_PCR_EXTEND, &[index], &[Auth::EMPTY_PASSWORD], &params.finish()?)
}

/// What `TPM2_PCR_Read` gives: the counter of updates, the PCRs that it
/// read (a marshalled `TPML_PCR_SELECTION`), and their values in the order
/// of the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcrValues {
    pub counter: u32,
    pub selection: Vec<u8>,
    pub values: Vec<Vec<u8>>,
}

impl PcrValues {
    pub fn read(reply: &Reply) -> Result<PcrValues, Error> {
        let mut p = reply.params();
        let counter = p.u32()?;
        let selection = read_selection_bytes(&mut p)?.to_vec();
        let count = p.count(2, MAX_VALUES, "PCR values")?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(p.sized_max(MAX_DIGEST, "a PCR value")?.to_vec());
        }
        p.finish("the response of TPM2_PCR_Read")?;
        Ok(PcrValues { counter, selection, values })
    }
}

/// The bytes of a `TPML_PCR_SELECTION`, with its count of banks bounded.
fn read_selection_bytes<'a>(input: &mut In<'a>) -> Result<&'a [u8], Error> {
    let start = input.rest();
    let count = input.count(3, MAX_BANKS, "PCR banks")?;
    for _ in 0..count {
        input.u16()?;
        let size = input.u8()?;
        input.take(usize::from(size))?;
    }
    Ok(&start[..start.len() - input.remaining()])
}
