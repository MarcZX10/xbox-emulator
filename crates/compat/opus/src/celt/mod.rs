pub mod bands;
pub mod cwrs;
pub mod decoder;
pub mod energy;
pub mod lpc;
pub mod mathops;
pub mod mdct;
pub mod rate;
pub mod tables;
pub mod vq;

#[cfg(test)]
mod tests;

pub const BITRES: i32 = 3;
pub const NB_EBANDS: usize = 21;
pub const EFF_EBANDS: usize = 21;
pub const SHORT_MDCT_SIZE: usize = 120;

pub use decoder::{CeltDecoder, OPUS_BAD_ARG, OPUS_BUFFER_TOO_SMALL, OPUS_INTERNAL_ERROR, OPUS_INVALID_PACKET, OPUS_OK};
