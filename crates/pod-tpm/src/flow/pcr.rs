//! The PCRs and the capabilities of a TPM: the values of a selection, the
//! statement of a PolicyPCR of the PCRs as they are now, an extend, and the
//! facts of a doctor line.

use pod_core::Class;

use super::{fail, step};
use crate::alg::HashAlg;
use crate::cmd::capability::{PT_HR_TRANSIENT_AVAIL, PT_MANUFACTURER};
use crate::cmd::{self, CAP_ALGS, CAP_TPM_PROPERTIES, Capability, CapabilityData, PcrValues};
use crate::error::Error;
use crate::policy::{Statement, pcr_values_digest};
use crate::selection::PcrSelection;
use crate::transport::{Transport, transact};

/// The most reads of a selection whose counter of updates moved between
/// two reads.
const TRIES: usize = 3;

/// The values of the PCRs of `selection`, in its order (the banks in order,
/// in each bank the lowest PCR first). A TPM reads 8 at most at once, so a
/// larger selection takes more reads; a reading whose counter of updates
/// moved between two reads starts again, three times at most.
pub fn pcr_read(transport: &mut dyn Transport, selection: &PcrSelection) -> Result<Vec<Vec<u8>>, Error> {
    let mut wanted = Vec::new();
    for bank in selection.banks() {
        let alg = bank.hash().ok_or_else(|| Error::Unsupported(format!("a read of the bank {:#06x}", bank.alg())))?;
        wanted.extend(bank.pcrs().into_iter().map(|pcr| (alg, pcr)));
    }
    'again: for _ in 0..TRIES {
        let mut values: Vec<((HashAlg, u32), Vec<u8>)> = Vec::with_capacity(wanted.len());
        let mut counter = None;
        while values.len() < wanted.len() {
            let mut left = PcrSelection::new();
            for (alg, pcr) in wanted.iter().filter(|w| !values.iter().any(|(v, _)| v == *w)) {
                left.add(*alg, *pcr)?;
            }
            let reply = transact(transport, &cmd::pcr_read(&left)?).map_err(step("the TPM did not read its PCRs"))?;
            let read = PcrValues::read(&reply)?;
            if counter.is_some_and(|c| c != read.counter) {
                continue 'again;
            }
            counter = Some(read.counter);
            if read.values.is_empty() {
                return Err(fail(
                    Class::Unsupported,
                    format!("the TPM reads none of the PCRs {left}: the bank is not allocated here"),
                ));
            }
            let mut each = read.values.into_iter();
            for bank in read.selection.banks() {
                for pcr in bank.pcrs() {
                    let key = (bank.hash(), pcr);
                    let value = each.next().unwrap_or_default();
                    match key {
                        (Some(alg), pcr)
                            if wanted.contains(&(alg, pcr)) && !values.iter().any(|(v, _)| *v == (alg, pcr)) =>
                        {
                            values.push(((alg, pcr), value));
                        }
                        _ => return Err(Error::Malformed(format!("the TPM read other PCRs than those of {left}"))),
                    }
                }
            }
        }
        let mut ordered = Vec::with_capacity(wanted.len());
        for w in &wanted {
            if let Some((_, value)) = values.iter().find(|(v, _)| v == w) {
                ordered.push(value.clone());
            }
        }
        return Ok(ordered);
    }
    Err(fail(Class::TempFail, format!("the PCRs {selection} changed three times while the crate read them")))
}

/// The statement of a `TPM2_PolicyPCR` of `selection` with the values that
/// the PCRs hold now, with SHA-256: the boot state of now, for a new key or
/// for a signed policy.
pub fn policy_pcr_now(transport: &mut dyn Transport, selection: &PcrSelection) -> Result<Statement, Error> {
    selection.check_policy()?;
    let values = pcr_read(transport, selection)?;
    let values: Vec<&[u8]> = values.iter().map(Vec::as_slice).collect();
    Ok(Statement::pcr(selection, &pcr_values_digest(HashAlg::Sha256, &values)?))
}

/// `TPM2_PCR_Extend` of PCR `index` of the SHA-256 bank with `digest`.
pub fn pcr_extend(transport: &mut dyn Transport, index: u32, digest: &[u8; 32]) -> Result<(), Error> {
    transact(transport, &cmd::pcr_extend(index, digest)?)
        .map_err(step(format!("the TPM did not extend PCR {index}")))?
        .empty()
}

/// The facts of a TPM for a doctor line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// The manufacturer: `IFX`, `STM`, `MSFT`, or its number.
    pub manufacturer: Option<String>,
    /// The algorithm ids that the TPM has.
    pub algorithms: Vec<u16>,
    /// The count of transient objects that the TPM can load now, at the
    /// least (`TPM_PT_HR_TRANSIENT_AVAIL`).
    pub transient_available: Option<u32>,
}

/// The manufacturer, the algorithms and the free transient slots: three
/// `TPM2_GetCapability`, and more for a list of algorithms that has more.
pub fn capabilities(transport: &mut dyn Transport) -> Result<Capabilities, Error> {
    let mut ask = |capability, property, count| -> Result<Capability, Error> {
        let reply = transact(transport, &cmd::get_capability(capability, property, count)?)
            .map_err(step("the TPM did not give its capabilities"))?;
        Capability::read(&reply)
    };
    let manufacturer = ask(CAP_TPM_PROPERTIES, PT_MANUFACTURER, 1)?.manufacturer();
    let transient_available = ask(CAP_TPM_PROPERTIES, PT_HR_TRANSIENT_AVAIL, 1)?.property(PT_HR_TRANSIENT_AVAIL);
    let mut algorithms: Vec<u16> = Vec::new();
    let mut from = 0u32;
    for _ in 0..8 {
        let answer = ask(CAP_ALGS, from, 128)?;
        let CapabilityData::Algorithms(list) = &answer.data else {
            return Err(Error::Malformed("the answer of the algorithms holds another capability".into()));
        };
        algorithms.extend(list.iter().map(|(id, _)| *id));
        match list.last() {
            Some((last, _)) if answer.more => from = u32::from(*last) + 1,
            _ => break,
        }
    }
    Ok(Capabilities { manufacturer, algorithms, transient_available })
}
