//! Error type for PE parsing. Every failure is explicit and actionable; the
//! parser never panics on malformed input (that invariant is fuzz-tested).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A read would go past the end of the buffer.
    OutOfBounds {
        what: &'static str,
        offset: usize,
        need: usize,
        len: usize,
    },
    /// A required magic/signature did not match.
    BadMagic {
        what: &'static str,
        found: u64,
        expected: u64,
    },
    /// A structural invariant was violated (value nonsensical / self-inconsistent).
    Invariant { what: &'static str, detail: String },
    /// An RVA could not be mapped to a file offset using the section table.
    UnmappableRva { rva: u32 },
    /// A feature is recognised but not yet supported by this parser. We fail
    /// loudly rather than silently mishandling it (CLAUDE.md principle).
    Unsupported { what: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::OutOfBounds { what, offset, need, len } => write!(
                f,
                "out-of-bounds reading {what}: need {need} byte(s) at offset {offset:#x}, buffer is {len:#x} byte(s)"
            ),
            ParseError::BadMagic { what, found, expected } => write!(
                f,
                "bad magic for {what}: found {found:#x}, expected {expected:#x}"
            ),
            ParseError::Invariant { what, detail } => {
                write!(f, "invariant violated ({what}): {detail}")
            }
            ParseError::UnmappableRva { rva } => {
                write!(f, "RVA {rva:#x} is not covered by any section")
            }
            ParseError::Unsupported { what } => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for ParseError {}

pub type Result<T> = std::result::Result<T, ParseError>;
