//! The reader of a struct of a message: its data words, its pointers, its
//! lists, each with a check of its bounds.

use super::*;

impl<'a> StructReader<'a> {
    pub(super) fn data_word(&self, byte: usize) -> Option<(u64, u32)> {
        if byte >= usize::from(self.data_words) * 8 {
            return None;
        }
        let segment = self.msg.segments.get(self.segment)?;
        let word = *segment.get(self.data + byte / 8)?;
        Some((word, ((byte % 8) * 8) as u32))
    }

    pub(super) fn bits(&self, byte: usize, width: u32) -> u64 {
        match self.data_word(byte) {
            Some((word, shift)) => {
                let value = word >> shift;
                if width == 64 { value } else { value & ((1u64 << width) - 1) }
            }
            // A field outside the data section has its default value: zero.
            None => 0,
        }
    }

    pub fn get_u8(&self, index: usize) -> u8 {
        self.bits(index, 8) as u8
    }

    pub fn get_u16(&self, index: usize) -> u16 {
        self.bits(index * 2, 16) as u16
    }

    pub fn get_u32(&self, index: usize) -> u32 {
        self.bits(index * 4, 32) as u32
    }

    pub fn get_u64(&self, index: usize) -> u64 {
        self.bits(index * 8, 64)
    }

    pub fn get_bool(&self, bit: usize) -> bool {
        match self.data_word(bit / 8) {
            Some((word, _)) => word & (1u64 << (bit % 64)) != 0,
            None => false,
        }
    }

    pub(super) fn pointer(&self, index: u16) -> Result<Target> {
        if index >= self.ptr_count {
            return Ok(Target::Null);
        }
        self.msg.follow(self.segment, self.ptrs + usize::from(index))
    }

    pub(super) fn child(&self, segment: usize, start: usize, data: u16, ptrs: u16) -> Result<Self> {
        if self.depth >= MAX_DEPTH {
            bail!(Stage::EdgeStream, Class::Protocol, "the message nests too deeply");
        }
        Ok(StructReader {
            msg: self.msg,
            segment,
            data: start,
            data_words: data,
            ptrs: start + usize::from(data),
            ptr_count: ptrs,
            depth: self.depth + 1,
        })
    }

    /// The struct in pointer `index`, or `None` for a null pointer.
    pub fn get_struct(&self, index: u16) -> Result<Option<StructReader<'a>>> {
        match self.pointer(index)? {
            Target::Null => Ok(None),
            Target::Struct { segment, start, data, ptrs } => self.child(segment, start, data, ptrs).map(Some),
            _ => bail!(Stage::EdgeStream, Class::Protocol, "pointer {index} is not a struct"),
        }
    }

    /// The capability index in pointer `index`, or `None`.
    pub fn get_capability(&self, index: u16) -> Result<Option<u32>> {
        match self.pointer(index)? {
            Target::Null => Ok(None),
            Target::Capability(cap) => Ok(Some(cap)),
            _ => bail!(Stage::EdgeStream, Class::Protocol, "pointer {index} is not a capability"),
        }
    }

    pub(super) fn bytes(&self, index: u16) -> Result<Option<Vec<u8>>> {
        match self.pointer(index)? {
            Target::Null => Ok(None),
            Target::List { segment, start, elem: ELEM_BYTE, count, .. } => {
                let end = start.saturating_add(count.div_ceil(8) as usize);
                let segment =
                    self.msg.segments.get(segment).or_fail(Stage::EdgeStream, Class::Protocol, "no such segment")?;
                let words =
                    segment.get(start..end).or_fail(Stage::EdgeStream, Class::Protocol, "a list past its segment")?;
                let bytes = words.iter().flat_map(|w| w.to_le_bytes());
                Ok(Some(bytes.take(count as usize).collect()))
            }
            _ => bail!(Stage::EdgeStream, Class::Protocol, "pointer {index} is not a byte list"),
        }
    }

    /// The Data in pointer `index` (empty for a null pointer).
    pub fn get_data(&self, index: u16) -> Result<Vec<u8>> {
        Ok(self.bytes(index)?.unwrap_or_default())
    }

    /// The Text in pointer `index` (empty for a null pointer).
    pub fn get_text(&self, index: u16) -> Result<String> {
        let mut bytes = self.bytes(index)?.unwrap_or_default();
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        String::from_utf8(bytes).or_fail(Stage::EdgeStream, Class::Protocol, "Text is not UTF-8")
    }

    /// The `List(Struct)` in pointer `index` (empty for a null pointer).
    pub fn get_struct_list(&self, index: u16) -> Result<Vec<StructReader<'a>>> {
        match self.pointer(index)? {
            Target::Null => Ok(Vec::new()),
            Target::List { segment, start, elem: ELEM_COMPOSITE, tag, count: list_words } => {
                if tag & 3 != 0 {
                    bail!(Stage::EdgeStream, Class::Protocol, "the tag of a struct list is not a struct pointer");
                }
                let count = ((tag as u32) >> 2) as usize;
                let data = ((tag >> 32) & 0xffff) as u16;
                let ptrs = (tag >> 48) as u16;
                let size = usize::from(data) + usize::from(ptrs);
                // The peer sets both factors: a product in usize wraps on a
                // 32-bit target. A u64 holds 2^30 * 2^17 words.
                let words = count as u64 * size as u64;
                if words > list_words {
                    bail!(Stage::EdgeStream, Class::Protocol, "the elements of a struct list overrun its pointer");
                }
                self.msg.check_range(segment, start + 1, words)?;
                // An element of 0 words reads no word. Charge one word for
                // each element, as the reference implementation does. Then a
                // short message cannot make a long list.
                if size == 0 {
                    self.msg.spend(count as u64)?;
                }
                (0..count).map(|i| self.child(segment, start + 1 + i * size, data, ptrs)).collect()
            }
            _ => bail!(Stage::EdgeStream, Class::Protocol, "pointer {index} is not a struct list"),
        }
    }

    /// The `List(Text)` in pointer `index` (empty for a null pointer).
    pub fn get_text_list(&self, index: u16) -> Result<Vec<String>> {
        match self.pointer(index)? {
            Target::Null => Ok(Vec::new()),
            Target::List { segment, start, elem: ELEM_POINTER, count, .. } => {
                if count > u64::from(u16::MAX) {
                    bail!(Stage::EdgeStream, Class::Protocol, "a text list of {count} elements");
                }
                self.msg.check_range(segment, start, count)?;
                let list = StructReader {
                    msg: self.msg,
                    segment,
                    data: start,
                    data_words: 0,
                    ptrs: start,
                    ptr_count: count as u16,
                    depth: self.depth + 1,
                };
                (0..list.ptr_count).map(|i| list.get_text(i)).collect()
            }
            _ => bail!(Stage::EdgeStream, Class::Protocol, "pointer {index} is not a pointer list"),
        }
    }
}
