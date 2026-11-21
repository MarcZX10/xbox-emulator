use crate::ffi::*;
use std::collections::VecDeque;
use xma::bits::BitWriter;

pub const XMA_SAMPLES_PER_FRAME: u32 = 512;

fn silent_frame(w: &mut BitWriter, channels: usize, more: bool) {
    let len = if channels == 1 { 31 } else { 36 };
    w.put(15, len);
    w.put(1, 1);
    w.put(1, 0);
    if channels == 2 {
        w.put(1, 0);
    }
    w.put(8, 0);
    w.put(1, 0);
    w.put(1, 0);
    w.put(1, 0);
    if channels == 2 {
        w.put(1, 0);
        w.put(1, 1);
        w.put(1, 0);
    }
    for _ in 0..channels {
        w.put(1, 0);
    }
    w.put(1, 0);
    w.put(1, more as u32);
}

pub fn silent_packet(channels: usize, frames: u32, skip: u32) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(6, frames);
    w.put(15, 0);
    w.put(3, 1);
    w.put(8, skip);
    for i in 0..frames {
        silent_frame(&mut w, channels, i + 1 < frames);
    }
    let mut b = w.buf;
    b.resize(XMA_BYTES_PER_PACKET as usize, 0);
    b
}

fn primed_stream(rate: u32, channels: usize) -> Result<xma::Xma2Stream, xma::InitError> {
    let mut dec = xma::Xma2Stream::new(rate, channels)?;
    let out = dec.decode_packet_planar(&silent_packet(channels, 1, 0));
    debug_assert!(out.is_empty());
    Ok(dec)
}

pub fn validate_format(f: &XMA2WAVEFORMATEX) -> Result<(), &'static str> {
    let w = f.wfx;
    let channels = w.n_channels as u32;
    if channels == 0 || w.n_samples_per_sec == 0 {
        return Err("nChannels and nSamplesPerSec must be nonzero");
    }
    if w.n_block_align as u32 != channels * 2 || w.w_bits_per_sample != 16 {
        return Err("XMA2 nBlockAlign must be nChannels*2 with wBitsPerSample 16");
    }
    if w.cb_size != 34 {
        return Err("XMA2 cbSize must be 34");
    }
    if f.encoder_version < 3 {
        return Err("XMA2 EncoderVersion must be at least 3");
    }
    if f.block_count == 0 {
        return Err("XMA2 BlockCount must be nonzero");
    }
    let bpb = f.bytes_per_block;
    if bpb == 0 || bpb > XMA_READBUFFER_MAX_BYTES {
        return Err("XMA2 BytesPerBlock must be in 1..=XMA_READBUFFER_MAX_BYTES");
    }
    let mask = f.channel_mask;
    if mask != 0 && mask.count_ones() != channels {
        return Err("XMA2 ChannelMask bit count must equal nChannels");
    }
    if f.num_streams as u32 != (channels + 1) >> 1 {
        return Err("XMA2 NumStreams must be (nChannels+1)/2");
    }
    let enc = f.samples_encoded;
    if f.play_begin.wrapping_add(f.play_length) > enc {
        return Err("XMA2 PlayBegin+PlayLength exceeds SamplesEncoded");
    }
    if f.loop_begin.wrapping_add(f.loop_length) > enc {
        return Err("XMA2 LoopBegin+LoopLength exceeds SamplesEncoded");
    }
    Ok(())
}

pub fn first_packet_metadata(packet: &[u8]) -> u32 {
    packet.get(2).map(|b| (*b & 7) as u32).unwrap_or(0)
}

pub fn validate_buffer(b: &XAUDIO2_BUFFER, data: &[u8], apu_ok: bool) -> Result<(), &'static str> {
    if !apu_ok {
        return Err("XMA buffer start address must be ApuAlloc memory aligned to XMA_BYTES_PER_PACKET");
    }
    if first_packet_metadata(data) != 1 {
        return Err("first XMA packet metadata must be 1 for XMA2 data");
    }
    let bytes = b.audio_bytes;
    if bytes % XMA_BYTES_PER_PACKET != 0 {
        return Err("XMA buffer size must be a multiple of XMA_BYTES_PER_PACKET");
    }
    if bytes > XMA_READBUFFER_MAX_BYTES {
        return Err("XMA buffer size exceeds XMA_READBUFFER_MAX_BYTES");
    }
    let pb = b.play_begin;
    if pb % XMA_SAMPLES_PER_SUBFRAME != 0 {
        return Err("XMA PlayBegin must fall on a 128-sample boundary");
    }
    let pl = b.play_length;
    if pl % XMA_SAMPLES_PER_SUBFRAME != 0 {
        return Err("XMA PlayLength must be a multiple of 128 samples");
    }
    Ok(())
}

pub fn console_loop_fill(b: &mut XAUDIO2_BUFFER) {
    let ll = b.loop_length;
    let pl = b.play_length;
    if ll == 0 && pl != 0 {
        b.loop_length = b.play_begin.wrapping_add(pl).wrapping_sub(b.loop_begin);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    pub play_begin: u32,
    pub play_length: u32,
    pub loop_begin: u32,
    pub loop_length: u32,
    pub loop_count: u32,
    pub clamped: bool,
}

pub fn map_regions(b: &XAUDIO2_BUFFER, decoded: u32) -> Regions {
    let total = decoded.max(1);
    let (pb0, pl0, lb0, ll0, lc) = (b.play_begin, b.play_length, b.loop_begin, b.loop_length, b.loop_count);
    let mut clamped = false;
    let pb = if pb0 >= total {
        clamped = true;
        total - 1
    } else {
        pb0
    };
    let avail = total - pb;
    let pl = if pl0 == 0 {
        0
    } else if pl0 > avail {
        clamped = true;
        avail
    } else {
        pl0
    };
    let play_end = pb + if pl == 0 { avail } else { pl };
    if lc == 0 {
        return Regions { play_begin: pb, play_length: pl, loop_begin: 0, loop_length: 0, loop_count: 0, clamped };
    }
    let lb = if lb0 >= play_end {
        clamped = true;
        play_end - 1
    } else {
        lb0
    };
    let ll = if ll0 == 0 {
        0
    } else if lb as u64 + ll0 as u64 > play_end as u64 {
        clamped = true;
        play_end - lb
    } else {
        ll0
    };
    Regions { play_begin: pb, play_length: pl, loop_begin: lb, loop_length: ll, loop_count: lc, clamped }
}

pub fn stream_layout(channels: usize) -> Vec<usize> {
    let n = (channels + 1) / 2;
    (0..n).map(|i| if i == n - 1 && channels % 2 == 1 { 1 } else { 2 }).collect()
}

struct Stream {
    dec: xma::Xma2Stream,
    fifo: VecDeque<i16>,
    channels: usize,
    skip: u32,
}

pub struct XmaDecoder {
    rate: u32,
    channels: usize,
    streams: Vec<Stream>,
    current: usize,
    packets: u64,
}

impl XmaDecoder {
    pub fn new(rate: u32, channels: usize) -> Result<XmaDecoder, xma::InitError> {
        let mut streams = Vec::new();
        for ch in stream_layout(channels) {
            streams.push(Stream { dec: primed_stream(rate, ch)?, fifo: VecDeque::new(), channels: ch, skip: 0 });
        }
        Ok(XmaDecoder { rate, channels, streams, current: 0, packets: 0 })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn packets_decoded(&self) -> u64 {
        self.packets
    }

    pub fn reset(&mut self) -> Result<(), xma::InitError> {
        *self = XmaDecoder::new(self.rate, self.channels)?;
        Ok(())
    }

    pub fn owner_of_next_packet(&self) -> usize {
        self.current
    }

    fn route(&mut self, packet: &[u8]) {
        let s = self.current;
        let st = &mut self.streams[s];
        for frame in st.dec.decode_packet_planar(packet) {
            let n = frame.first().map_or(0, |c| c.len());
            for i in 0..n {
                for c in 0..st.channels {
                    st.fifo.push_back(xma::f32_to_s16(frame[c][i]));
                }
            }
        }
        st.skip = xma::packet_skip_count(packet);
        self.packets += 1;
        if self.streams[s].skip != 0 {
            let mut best = 0;
            for i in 1..self.streams.len() {
                if self.streams[i].skip < self.streams[best].skip {
                    best = i;
                }
            }
            self.current = best;
        }
        for st in self.streams.iter_mut() {
            st.skip = st.skip.saturating_sub(1);
        }
    }

    pub fn feed(&mut self, data: &[u8], out: &mut Vec<i16>) -> usize {
        for pkt in data.chunks(XMA_BYTES_PER_PACKET as usize) {
            if pkt.len() == XMA_BYTES_PER_PACKET as usize {
                self.route(pkt);
            }
        }
        self.drain(out)
    }

    pub fn drain(&mut self, out: &mut Vec<i16>) -> usize {
        let frames = self.streams.iter().map(|s| s.fifo.len() / s.channels).min().unwrap_or(0);
        out.reserve(frames * self.channels);
        for _ in 0..frames {
            for st in self.streams.iter_mut() {
                for _ in 0..st.channels {
                    out.push(st.fifo.pop_front().unwrap_or(0));
                }
            }
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(ch: u16) -> XMA2WAVEFORMATEX {
        XMA2WAVEFORMATEX {
            wfx: WAVEFORMATEX { w_format_tag: WAVE_FORMAT_XMA2, n_channels: ch, n_samples_per_sec: 48000, n_avg_bytes_per_sec: 48000 * ch as u32 * 2, n_block_align: ch * 2, w_bits_per_sample: 16, cb_size: 34 },
            num_streams: (ch + 1) / 2,
            channel_mask: 0,
            samples_encoded: 4096,
            bytes_per_block: 2048,
            play_begin: 0,
            play_length: 0,
            loop_begin: 0,
            loop_length: 0,
            loop_count: 0,
            encoder_version: 4,
            block_count: 1,
        }
    }

    #[test]
    fn format_rules_match_console_validator() {
        assert!(validate_format(&fmt(1)).is_ok());
        assert!(validate_format(&fmt(2)).is_ok());
        assert!(validate_format(&fmt(6)).is_ok());
        let mut f = fmt(2);
        f.wfx.cb_size = 22;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.encoder_version = 2;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.block_count = 0;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.bytes_per_block = XMA_READBUFFER_MAX_BYTES + 1;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.channel_mask = 0x7;
        assert!(validate_format(&f).is_err());
        f.channel_mask = 0x3;
        assert!(validate_format(&f).is_ok());
        let mut f = fmt(3);
        f.num_streams = 1;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.play_begin = 4000;
        f.play_length = 200;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.loop_begin = 4096;
        f.loop_length = 1;
        assert!(validate_format(&f).is_err());
        let mut f = fmt(2);
        f.wfx.w_bits_per_sample = 8;
        assert!(validate_format(&f).is_err());
    }

    fn buf(bytes: u32, pb: u32, pl: u32) -> XAUDIO2_BUFFER {
        XAUDIO2_BUFFER { audio_bytes: bytes, play_begin: pb, play_length: pl, ..Default::default() }
    }

    #[test]
    fn buffer_rules_match_gcxaudioext() {
        let mut pkt = vec![0u8; 4096];
        pkt[2] = 1;
        assert!(validate_buffer(&buf(4096, 0, 0), &pkt, true).is_ok());
        assert!(validate_buffer(&buf(4096, 0, 0), &pkt, false).is_err());
        assert!(validate_buffer(&buf(4000, 0, 0), &pkt, true).is_err());
        assert!(validate_buffer(&buf(XMA_READBUFFER_MAX_BYTES + 2048, 0, 0), &pkt, true).is_err());
        assert!(validate_buffer(&buf(4096, 64, 0), &pkt, true).is_err());
        assert!(validate_buffer(&buf(4096, 128, 100), &pkt, true).is_err());
        assert!(validate_buffer(&buf(4096, 128, 256), &pkt, true).is_ok());
        pkt[2] = 0;
        assert!(validate_buffer(&buf(4096, 0, 0), &pkt, true).is_err());
    }

    #[test]
    fn loop_fill_matches_console() {
        let mut b = XAUDIO2_BUFFER { play_begin: 256, play_length: 1024, loop_begin: 512, loop_length: 0, loop_count: 2, ..Default::default() };
        console_loop_fill(&mut b);
        assert_eq!({ b.loop_length }, 768);
        let mut b = XAUDIO2_BUFFER { play_begin: 0, play_length: 0, loop_begin: 0, loop_length: 0, loop_count: 2, ..Default::default() };
        console_loop_fill(&mut b);
        assert_eq!({ b.loop_length }, 0);
    }

    #[test]
    fn region_mapping_keeps_in_range_values_and_clamps_overruns() {
        let b = XAUDIO2_BUFFER { play_begin: 128, play_length: 256, loop_begin: 128, loop_length: 128, loop_count: 3, ..Default::default() };
        let r = map_regions(&b, 1000);
        assert_eq!(r, Regions { play_begin: 128, play_length: 256, loop_begin: 128, loop_length: 128, loop_count: 3, clamped: false });
        let b = XAUDIO2_BUFFER { play_begin: 0, play_length: 2048, loop_count: 0, loop_begin: 5, loop_length: 7, ..Default::default() };
        let r = map_regions(&b, 1000);
        assert_eq!(r, Regions { play_begin: 0, play_length: 1000, loop_begin: 0, loop_length: 0, loop_count: 0, clamped: true });
        let b = XAUDIO2_BUFFER { play_begin: 0, play_length: 0, loop_begin: 900, loop_length: 500, loop_count: 255, ..Default::default() };
        let r = map_regions(&b, 1000);
        assert_eq!(r, Regions { play_begin: 0, play_length: 0, loop_begin: 900, loop_length: 100, loop_count: 255, clamped: true });
    }

    #[test]
    fn stream_layout_pairs_channels() {
        assert_eq!(stream_layout(1), vec![1]);
        assert_eq!(stream_layout(2), vec![2]);
        assert_eq!(stream_layout(3), vec![2, 1]);
        assert_eq!(stream_layout(6), vec![2, 2, 2]);
    }

    fn packet_with_skip(skip: u8) -> Vec<u8> {
        let mut p = vec![0u8; XMA_BYTES_PER_PACKET as usize];
        p[2] = 1;
        p[3] = skip;
        p
    }

    #[test]
    fn multistream_routing_follows_ffmpeg_packet_skip() {
        let mut d = XmaDecoder::new(48000, 4).unwrap();
        let mut owners = Vec::new();
        for skip in [1u8, 1, 1, 1, 3, 0, 1, 1] {
            owners.push(d.owner_of_next_packet());
            d.route(&packet_with_skip(skip));
        }
        assert_eq!(owners, vec![0, 1, 0, 1, 0, 1, 1, 0]);
        let mut d = XmaDecoder::new(48000, 2).unwrap();
        for _ in 0..4 {
            assert_eq!(d.owner_of_next_packet(), 0);
            d.route(&packet_with_skip(0));
        }
    }
}
