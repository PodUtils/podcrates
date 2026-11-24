//! A selection of PCRs (`TPML_PCR_SELECTION`; the Library specification,
//! part 2, clauses 10.6.1, 10.6.2 and 10.9.7): banks, each a hash
//! algorithm and a bit set of PCR indices. Bit n of octet n/8 is PCR n
//! (part 1, "Selecting Multiple PCR"). The values of a selection are in its
//! order: the banks in order, and in each bank the lowest PCR first.

use std::fmt;

use crate::alg::HashAlg;
use crate::error::Error;
use crate::wire::{In, Out};

/// The PCRs of a bank on a PC Client platform: 0 to 23.
pub const PCR_COUNT: u32 = 24;
/// The octets of the bit set that the crate writes: `PCR_SELECT_MIN` of a
/// PC Client platform, (24 + 7) / 8.
pub const PCR_SELECT_MIN: usize = 3;
/// The most octets of a bit set that the crate reads: `PCR_SELECT_MAX` of
/// a TPM of 32 PCRs, as tpm2-tss takes it.
pub const PCR_SELECT_MAX: usize = 4;
/// The most banks in a selection (`HASH_COUNT`, bounded here).
pub const MAX_BANKS: usize = 16;

/// One bank of a selection: the id of its hash algorithm, the octets of
/// its bit set as it came, and its PCRs as bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bank {
    alg: u16,
    size: u8,
    bits: u32,
}

impl Bank {
    /// The id of the hash algorithm of the bank.
    pub fn alg(&self) -> u16 {
        self.alg
    }

    /// The hash algorithm, if the crate knows it.
    pub fn hash(&self) -> Option<HashAlg> {
        HashAlg::from_id(self.alg)
    }

    /// The PCRs, the lowest first.
    pub fn pcrs(&self) -> Vec<u32> {
        (0..PCR_COUNT).filter(|n| self.bits & (1 << n) != 0).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    fn name(&self) -> String {
        self.hash().map_or_else(|| format!("{:#06x}", self.alg), |h| h.name().to_string())
    }
}

/// A selection of PCRs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PcrSelection {
    banks: Vec<Bank>,
}

impl PcrSelection {
    /// The selection of no PCR.
    pub fn new() -> PcrSelection {
        PcrSelection::default()
    }

    /// The selection of `pcrs` in the bank of `alg`.
    pub fn of(alg: HashAlg, pcrs: &[u32]) -> Result<PcrSelection, Error> {
        let mut selection = PcrSelection::new();
        for pcr in pcrs {
            selection.add(alg, *pcr)?;
        }
        Ok(selection)
    }

    /// Add PCR `pcr` of the bank of `alg`; the bank comes last when it is
    /// new.
    pub fn add(&mut self, alg: HashAlg, pcr: u32) -> Result<(), Error> {
        if pcr >= PCR_COUNT {
            return Err(Error::Usage(format!("{pcr} is no PCR: the PCRs are 0 to {}", PCR_COUNT - 1)));
        }
        let index = match self.banks.iter().position(|b| b.alg == alg.id()) {
            Some(index) => index,
            None if self.banks.len() >= MAX_BANKS => {
                return Err(Error::Usage(format!("more than {MAX_BANKS} banks of PCRs")));
            }
            None => {
                self.banks.push(Bank { alg: alg.id(), size: PCR_SELECT_MIN as u8, bits: 0 });
                self.banks.len() - 1
            }
        };
        self.banks[index].bits |= 1 << pcr;
        Ok(())
    }

    /// A selection as tpm2-tools writes it: `sha256:0,7+sha1:1`. A list with
    /// no bank (`0,7,11`) is of the SHA-256 bank. Each PCR is 0 to 23, and
    /// each bank names one at least.
    pub fn parse(text: &str) -> Result<PcrSelection, Error> {
        let mut selection = PcrSelection::new();
        for part in text.split('+') {
            let (alg, list) = match part.split_once(':') {
                Some((name, list)) => {
                    let name = name.trim();
                    let alg = HashAlg::ALL.into_iter().find(|a| a.name() == name);
                    let alg = alg.ok_or_else(|| Error::Usage(format!("{name:?} is no bank of PCRs")))?;
                    (alg, list)
                }
                None => (HashAlg::Sha256, part),
            };
            for word in list.split(',') {
                let word = word.trim();
                let pcr = word.parse::<u32>().ok().filter(|n| *n < PCR_COUNT);
                let pcr = pcr
                    .ok_or_else(|| Error::Usage(format!("{word:?} is no PCR: the PCRs are 0 to {}", PCR_COUNT - 1)))?;
                selection.add(alg, pcr)?;
            }
        }
        Ok(selection)
    }

    pub fn banks(&self) -> &[Bank] {
        &self.banks
    }

    /// The PCRs of the bank of `alg`, the lowest first.
    pub fn pcrs(&self, alg: HashAlg) -> Vec<u32> {
        let mut out: Vec<u32> = self.banks.iter().filter(|b| b.alg == alg.id()).flat_map(|b| b.pcrs()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The PCRs of the bank of `alg`, where another bank that selects a PCR
    /// is refused: for a caller that reads one bank.
    pub fn only(&self, alg: HashAlg) -> Result<Vec<u32>, Error> {
        if let Some(other) = self.banks.iter().find(|b| b.alg != alg.id() && !b.is_empty()) {
            return Err(Error::Unsupported(format!(
                "the selection names PCRs of the bank {}, where the caller reads the bank {alg}",
                other.name()
            )));
        }
        Ok(self.pcrs(alg))
    }

    /// The count of selected PCRs: the count of values that they give.
    pub fn count(&self) -> usize {
        self.banks.iter().map(|b| b.bits.count_ones() as usize).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// That the selection can go into a policy: one bank at least, no empty
    /// bank, and a hash algorithm that the crate knows in each.
    pub fn check_policy(&self) -> Result<(), Error> {
        if self.banks.is_empty() {
            return Err(Error::Usage("a policy of PCRs selects no PCR".into()));
        }
        for bank in &self.banks {
            if bank.is_empty() {
                return Err(Error::Usage(format!("a policy of PCRs has an empty bank {}", bank.name())));
            }
            if bank.hash().is_none() {
                return Err(Error::Unsupported(format!("a policy of PCRs in the bank {}", bank.name())));
            }
        }
        Ok(())
    }

    /// The marshalled `TPML_PCR_SELECTION`. A bank that was read keeps the
    /// octets of its bit set, so that the bytes are the bytes that came.
    pub fn marshal(&self) -> Vec<u8> {
        let mut out = Out::default();
        // At most MAX_BANKS banks, so the count fits in a u32.
        out.u32(self.banks.len() as u32);
        for bank in &self.banks {
            out.u16(bank.alg).u8(bank.size).raw(&bank.bits.to_le_bytes()[..usize::from(bank.size)]);
        }
        // Out refuses only a sized buffer that is too long, and here is none.
        out.finish().unwrap_or_default()
    }

    /// Read a `TPML_PCR_SELECTION` from `input`: at most 16 banks, each bit
    /// set of at most 4 octets, and no PCR from 24 up.
    pub fn read(input: &mut In) -> Result<PcrSelection, Error> {
        let count = input.count(3, MAX_BANKS, "PCR banks")?;
        let mut banks = Vec::with_capacity(count);
        for _ in 0..count {
            let alg = input.u16()?;
            let size = input.u8()?;
            if usize::from(size) > PCR_SELECT_MAX {
                return Err(Error::Malformed(format!(
                    "a bit set of PCRs of {size} octets, where part 2 allows {PCR_SELECT_MAX}"
                )));
            }
            let mut bits = 0u32;
            for (i, byte) in input.take(usize::from(size))?.iter().enumerate() {
                bits |= u32::from(*byte) << (8 * i);
            }
            if bits >> PCR_COUNT != 0 {
                return Err(Error::Malformed(format!("PCR {PCR_COUNT} and up, which no platform has")));
            }
            banks.push(Bank { alg, size, bits });
        }
        Ok(PcrSelection { banks })
    }

    /// A whole marshalled `TPML_PCR_SELECTION`, with no byte after it.
    pub fn from_bytes(bytes: &[u8]) -> Result<PcrSelection, Error> {
        let mut input = In::new(bytes);
        let selection = PcrSelection::read(&mut input)?;
        input.finish("the selection of PCRs")?;
        Ok(selection)
    }
}

impl fmt::Display for PcrSelection {
    /// `sha256:0,7,11+sha1:1`; `none` for no bank.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.banks.is_empty() {
            return f.write_str("none");
        }
        for (i, bank) in self.banks.iter().enumerate() {
            let pcrs: Vec<String> = bank.pcrs().iter().map(u32::to_string).collect();
            write!(f, "{}{}:{}", if i == 0 { "" } else { "+" }, bank.name(), pcrs.join(","))?;
        }
        Ok(())
    }
}
