//! `TPM2_GetCapability` (part 3, clause 30.2) and its data
//! (`TPMS_CAPABILITY_DATA`, part 2, clause 10.10.2): the algorithms, the
//! handles and the properties of the TPM.

use super::CC_GET_CAPABILITY;
use crate::error::Error;
use crate::transport::Reply;
use crate::wire::{MAX_RESPONSE, Out, command};

/// `TPM_CAP_ALGS`: the algorithms of the TPM.
pub const CAP_ALGS: u32 = 0x0000_0000;
/// `TPM_CAP_HANDLES`: the handles of a range (the first octet of the
/// property names it: 0x80 for the transient objects).
pub const CAP_HANDLES: u32 = 0x0000_0001;
/// `TPM_CAP_TPM_PROPERTIES`: the properties of the TPM.
pub const CAP_TPM_PROPERTIES: u32 = 0x0000_0006;
/// `TPM_PT_MANUFACTURER`: the vendor of the TPM, four letters.
pub const PT_MANUFACTURER: u32 = 0x0000_0105;
/// `TPM_PT_HR_TRANSIENT_AVAIL`: the count of transient objects that the TPM
/// can load now, at the least.
pub const PT_HR_TRANSIENT_AVAIL: u32 = 0x0000_0207;

/// `TPM2_GetCapability` of `count` items of `capability` from `property` on.
pub fn get_capability(capability: u32, property: u32, count: u32) -> Result<Vec<u8>, Error> {
    let mut params = Out::default();
    params.u32(capability).u32(property).u32(count);
    command(CC_GET_CAPABILITY, &[], &[], &params.finish()?)
}

/// `TPM2_GetCapability` of the manufacturer: a question that each TPM 2.0
/// answers.
pub fn get_manufacturer() -> Result<Vec<u8>, Error> {
    get_capability(CAP_TPM_PROPERTIES, PT_MANUFACTURER, 1)
}

/// The data of a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityData {
    /// Each algorithm and its attributes (`TPMS_ALG_PROPERTY`).
    Algorithms(Vec<(u16, u32)>),
    Handles(Vec<u32>),
    /// Each property and its value (`TPMS_TAGGED_PROPERTY`).
    Properties(Vec<(u32, u32)>),
    /// A capability that the crate does not read: its code and its bytes.
    Other(u32, Vec<u8>),
}

/// What `TPM2_GetCapability` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// More items of the capability come after these.
    pub more: bool,
    pub data: CapabilityData,
}

impl Capability {
    pub fn read(reply: &Reply) -> Result<Capability, Error> {
        let mut p = reply.params();
        let more = match p.u8()? {
            0 => false,
            1 => true,
            other => return Err(Error::Malformed(format!("moreData is {other}, where it is 0 or 1"))),
        };
        let capability = p.u32()?;
        let data = match capability {
            CAP_ALGS => {
                let n = p.count(6, MAX_RESPONSE / 6, "algorithms")?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push((p.u16()?, p.u32()?));
                }
                CapabilityData::Algorithms(list)
            }
            CAP_HANDLES => {
                let n = p.count(4, MAX_RESPONSE / 4, "handles")?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(p.u32()?);
                }
                CapabilityData::Handles(list)
            }
            CAP_TPM_PROPERTIES => {
                let n = p.count(8, MAX_RESPONSE / 8, "properties")?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push((p.u32()?, p.u32()?));
                }
                CapabilityData::Properties(list)
            }
            other => {
                let rest = p.take(p.remaining())?.to_vec();
                CapabilityData::Other(other, rest)
            }
        };
        p.finish("the response of TPM2_GetCapability")?;
        Ok(Capability { more, data })
    }

    /// The value of the TPM property `property`, if the data holds it.
    pub fn property(&self, property: u32) -> Option<u32> {
        match &self.data {
            CapabilityData::Properties(list) => list.iter().find(|(p, _)| *p == property).map(|(_, v)| *v),
            _ => None,
        }
    }

    /// The manufacturer: four letters, as `IFX`, `STM` or `MSFT`, or its
    /// number when they are not letters.
    pub fn manufacturer(&self) -> Option<String> {
        let value = self.property(PT_MANUFACTURER)?.to_be_bytes();
        let text: String = value.iter().filter(|b| **b != 0).map(|b| char::from(*b)).collect();
        if !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
            return Some(text.trim().to_string());
        }
        Some(format!("{:#010x}", u32::from_be_bytes(value)))
    }
}
