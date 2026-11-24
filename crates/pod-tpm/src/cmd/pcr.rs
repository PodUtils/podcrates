//! The commands of the PCRs: `TPM2_PCR_Read` (part 3, clause 22.4) and
//! `TPM2_PCR_Extend` (22.2).

use super::{CC_PCR_EXTEND, CC_PCR_READ};
use crate::alg::ALG_SHA256;
use crate::error::Error;
use crate::selection::PcrSelection;
use crate::transport::Reply;
use crate::wire::{Auth, MAX_DIGEST, Out, command};

/// The most digests in a `TPML_DIGEST` (part 2, clause 10.9.5).
pub const MAX_VALUES: usize = 8;
/// The highest PCR index that a command takes.
pub const MAX_PCR: u32 = 31;

/// `TPM2_PCR_Read` of `selection`. A TPM reads 8 PCRs at most at once, and
/// its response names the PCRs that it read.
pub fn pcr_read(selection: &PcrSelection) -> Result<Vec<u8>, Error> {
    command(CC_PCR_READ, &[], &[], &selection.marshal())
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
/// read, and their values in the order of the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcrValues {
    pub counter: u32,
    pub selection: PcrSelection,
    pub values: Vec<Vec<u8>>,
}

impl PcrValues {
    /// Read the response; a count of values that is not the count of the
    /// PCRs that it names is refused.
    pub fn read(reply: &Reply) -> Result<PcrValues, Error> {
        let mut p = reply.params();
        let counter = p.u32()?;
        let selection = PcrSelection::read(&mut p)?;
        let count = p.count(2, MAX_VALUES, "PCR values")?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(p.sized_max(MAX_DIGEST, "a PCR value")?.to_vec());
        }
        p.finish("the response of TPM2_PCR_Read")?;
        if values.len() != selection.count() {
            return Err(Error::Malformed(format!(
                "{} values for the {} PCRs that the response names",
                values.len(),
                selection.count()
            )));
        }
        let mut each = values.iter();
        for bank in selection.banks() {
            for pcr in bank.pcrs() {
                let (Some(value), Some(alg)) = (each.next(), bank.hash()) else { continue };
                if value.len() != alg.size() {
                    return Err(Error::Malformed(format!(
                        "the value of PCR {pcr} of the bank {alg} has {} bytes",
                        value.len()
                    )));
                }
            }
        }
        Ok(PcrValues { counter, selection, values })
    }
}
