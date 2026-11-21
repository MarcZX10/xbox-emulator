//! A cursor over a byte slice with fully bounds-checked little-endian reads.
//! Every accessor returns `Result` instead of panicking, so malformed input
//! produces a diagnostic rather than a crash.

use crate::error::{ParseError, Result};

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Return a sub-slice [off, off+len) with a bounds check.
    pub fn slice_at(&self, off: usize, len: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = off.checked_add(len).ok_or(ParseError::OutOfBounds {
            what,
            offset: off,
            need: len,
            len: self.data.len(),
        })?;
        self.data.get(off..end).ok_or(ParseError::OutOfBounds {
            what,
            offset: off,
            need: len,
            len: self.data.len(),
        })
    }

    pub fn u8_at(&self, off: usize, what: &'static str) -> Result<u8> {
        Ok(self.slice_at(off, 1, what)?[0])
    }

    pub fn u16_at(&self, off: usize, what: &'static str) -> Result<u16> {
        let b = self.slice_at(off, 2, what)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32_at(&self, off: usize, what: &'static str) -> Result<u32> {
        let b = self.slice_at(off, 4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64_at(&self, off: usize, what: &'static str) -> Result<u64> {
        let b = self.slice_at(off, 8, what)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    // Sequential variants that advance the cursor.
    pub fn u16(&mut self, what: &'static str) -> Result<u16> {
        let v = self.u16_at(self.pos, what)?;
        self.pos += 2;
        Ok(v)
    }

    pub fn u32(&mut self, what: &'static str) -> Result<u32> {
        let v = self.u32_at(self.pos, what)?;
        self.pos += 4;
        Ok(v)
    }

    /// Read a NUL-terminated ASCII string beginning at `off`. `max` caps the
    /// scan so a missing terminator in corrupt data cannot run away. The
    /// returned string is lossy-decoded for display; raw bytes are preserved
    /// by callers that need them.
    pub fn cstr_at(&self, off: usize, max: usize, what: &'static str) -> Result<String> {
        // Ensure the start is in bounds; scan up to `max` or EOF.
        if off > self.data.len() {
            return Err(ParseError::OutOfBounds {
                what,
                offset: off,
                need: 1,
                len: self.data.len(),
            });
        }
        let end_cap = off.saturating_add(max).min(self.data.len());
        let mut end = off;
        while end < end_cap && self.data[end] != 0 {
            end += 1;
        }
        Ok(String::from_utf8_lossy(&self.data[off..end]).into_owned())
    }
}
