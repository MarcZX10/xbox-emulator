use crate::celt::{CeltDecoder, OPUS_BAD_ARG, OPUS_BUFFER_TOO_SMALL, OPUS_INTERNAL_ERROR, OPUS_INVALID_PACKET, OPUS_OK};
use crate::entdec::Decoder as RangeDecoder;
use crate::silk::{DecControl, SilkDecoder};

pub const MODE_SILK_ONLY: i32 = 1000;
pub const MODE_HYBRID: i32 = 1001;
pub const MODE_CELT_ONLY: i32 = 1002;

pub const BANDWIDTH_NARROWBAND: i32 = 1101;
pub const BANDWIDTH_MEDIUMBAND: i32 = 1102;
pub const BANDWIDTH_WIDEBAND: i32 = 1103;
pub const BANDWIDTH_SUPERWIDEBAND: i32 = 1104;
pub const BANDWIDTH_FULLBAND: i32 = 1105;

pub fn packet_mode(toc: u8) -> i32 {
    if toc & 0x80 != 0 {
        MODE_CELT_ONLY
    } else if toc & 0x60 == 0x60 {
        MODE_HYBRID
    } else {
        MODE_SILK_ONLY
    }
}

pub fn packet_bandwidth(toc: u8) -> i32 {
    if toc & 0x80 != 0 {
        let b = BANDWIDTH_MEDIUMBAND + ((toc >> 5) & 3) as i32;
        if b == BANDWIDTH_MEDIUMBAND {
            BANDWIDTH_NARROWBAND
        } else {
            b
        }
    } else if toc & 0x60 == 0x60 {
        if toc & 0x10 != 0 {
            BANDWIDTH_FULLBAND
        } else {
            BANDWIDTH_SUPERWIDEBAND
        }
    } else {
        BANDWIDTH_NARROWBAND + ((toc >> 5) & 3) as i32
    }
}

pub fn packet_channels(toc: u8) -> usize {
    if toc & 4 != 0 {
        2
    } else {
        1
    }
}

pub fn samples_per_frame(toc: u8, fs: i32) -> i32 {
    if toc & 0x80 != 0 {
        let a = ((toc >> 3) & 3) as i32;
        (fs << a) / 400
    } else if toc & 0x60 == 0x60 {
        if toc & 8 != 0 {
            fs / 50
        } else {
            fs / 100
        }
    } else {
        let a = ((toc >> 3) & 3) as i32;
        if a == 3 {
            fs * 60 / 1000
        } else {
            (fs << a) / 100
        }
    }
}

fn parse_size(data: &[u8]) -> (i32, i32) {
    if data.is_empty() {
        (-1, -1)
    } else if data[0] < 252 {
        (1, data[0] as i32)
    } else if data.len() < 2 {
        (-1, -1)
    } else {
        (2, 4 * data[1] as i32 + data[0] as i32)
    }
}

pub struct ParsedPacket {
    pub toc: u8,
    pub sizes: Vec<usize>,
    pub payload_offset: usize,
    pub packet_offset: usize,
}

pub fn parse_packet(packet: &[u8], self_delimited: bool) -> Result<ParsedPacket, i32> {
    if packet.is_empty() {
        return Err(OPUS_INVALID_PACKET);
    }
    let framesize = samples_per_frame(packet[0], 48000);
    let toc = packet[0];
    let mut p = 1usize;
    let mut len = packet.len() as i32 - 1;
    let mut last_size = len;
    let mut size = [0i32; 48];
    let mut cbr = false;
    let count: usize;
    let mut pad = 0i32;
    match toc & 3 {
        0 => count = 1,
        1 => {
            count = 2;
            cbr = true;
            if !self_delimited {
                if len & 1 != 0 {
                    return Err(OPUS_INVALID_PACKET);
                }
                last_size = len / 2;
                size[0] = last_size;
            }
        }
        2 => {
            count = 2;
            let (bytes, s) = parse_size(&packet[p..]);
            len -= bytes;
            if s < 0 || s > len {
                return Err(OPUS_INVALID_PACKET);
            }
            size[0] = s;
            p += bytes as usize;
            last_size = len - s;
        }
        _ => {
            if len < 1 {
                return Err(OPUS_INVALID_PACKET);
            }
            let ch = packet[p];
            p += 1;
            count = (ch & 0x3f) as usize;
            if count == 0 || framesize * count as i32 > 5760 {
                return Err(OPUS_INVALID_PACKET);
            }
            len -= 1;
            if ch & 0x40 != 0 {
                loop {
                    if len <= 0 {
                        return Err(OPUS_INVALID_PACKET);
                    }
                    let v = packet[p] as i32;
                    p += 1;
                    len -= 1;
                    let tmp = if v == 255 { 254 } else { v };
                    len -= tmp;
                    pad += tmp;
                    if v != 255 {
                        break;
                    }
                }
            }
            if len < 0 {
                return Err(OPUS_INVALID_PACKET);
            }
            cbr = ch & 0x80 == 0;
            if !cbr {
                last_size = len;
                for s in size.iter_mut().take(count - 1) {
                    let (bytes, v) = parse_size(&packet[p.min(packet.len())..]);
                    len -= bytes;
                    if v < 0 || v > len {
                        return Err(OPUS_INVALID_PACKET);
                    }
                    *s = v;
                    p += bytes as usize;
                    last_size -= bytes + v;
                }
                if last_size < 0 {
                    return Err(OPUS_INVALID_PACKET);
                }
            } else if !self_delimited {
                last_size = len / count as i32;
                if last_size * count as i32 != len {
                    return Err(OPUS_INVALID_PACKET);
                }
                for s in size.iter_mut().take(count - 1) {
                    *s = last_size;
                }
            }
        }
    }
    if self_delimited {
        let (bytes, v) = parse_size(&packet[p.min(packet.len())..]);
        len -= bytes;
        if v < 0 || v > len {
            return Err(OPUS_INVALID_PACKET);
        }
        size[count - 1] = v;
        p += bytes as usize;
        if cbr {
            if v * count as i32 > len {
                return Err(OPUS_INVALID_PACKET);
            }
            for s in size.iter_mut().take(count - 1) {
                *s = v;
            }
        } else if bytes + v > last_size {
            return Err(OPUS_INVALID_PACKET);
        }
    } else {
        if last_size > 1275 {
            return Err(OPUS_INVALID_PACKET);
        }
        size[count - 1] = last_size;
    }
    let payload_offset = p;
    let mut end = p;
    for s in size.iter().take(count) {
        end += *s as usize;
    }
    if end > packet.len() {
        return Err(OPUS_INVALID_PACKET);
    }
    let packet_offset = if self_delimited { end } else { end + pad as usize };
    Ok(ParsedPacket { toc, sizes: size[..count].iter().map(|&s| s as usize).collect(), payload_offset, packet_offset: packet_offset.min(packet.len()) })
}

fn smooth_fade(in1: &[f32], in2: &[f32], out: &mut [f32], overlap: usize, channels: usize, window: &[f32], fs: i32) {
    let inc = (48000 / fs) as usize;
    for c in 0..channels {
        for i in 0..overlap {
            let k = i * channels + c;
            if k >= out.len() || k >= in1.len() || k >= in2.len() {
                break;
            }
            let mut w = window.get(i * inc).copied().unwrap_or(1.0);
            w *= w;
            out[k] = w * in2[k] + (1.0 - w) * in1[k];
        }
    }
}

pub struct OpusDecoder {
    celt: CeltDecoder,
    silk: SilkDecoder,
    channels: usize,
    fs: i32,
    dec_control: DecControl,
    decode_gain: i32,
    stream_channels: usize,
    bandwidth: i32,
    mode: i32,
    prev_mode: i32,
    frame_size: i32,
    prev_redundancy: bool,
    last_packet_duration: i32,
    softclip_mem: [f32; 2],
    range_final: u32,
}

impl OpusDecoder {
    pub fn new(fs: i32, channels: usize) -> Result<OpusDecoder, i32> {
        if !matches!(fs, 48000 | 24000 | 16000 | 12000 | 8000) || !(1..=2).contains(&channels) {
            return Err(OPUS_BAD_ARG);
        }
        let mut celt = CeltDecoder::new(fs, channels).map_err(|_| OPUS_INTERNAL_ERROR)?;
        celt.set_signalling(false);
        let dec_control = DecControl { n_channels_api: channels as i32, api_sample_rate: fs, ..Default::default() };
        Ok(OpusDecoder {
            celt,
            silk: SilkDecoder::new(),
            channels,
            fs,
            dec_control,
            decode_gain: 0,
            stream_channels: channels,
            bandwidth: 0,
            mode: 0,
            prev_mode: 0,
            frame_size: fs / 400,
            prev_redundancy: false,
            last_packet_duration: 0,
            softclip_mem: [0.0; 2],
            range_final: 0,
        })
    }

    pub fn reset(&mut self) {
        self.stream_channels = self.channels;
        self.bandwidth = 0;
        self.mode = 0;
        self.prev_mode = 0;
        self.prev_redundancy = false;
        self.last_packet_duration = 0;
        self.softclip_mem = [0.0; 2];
        self.range_final = 0;
        self.celt.reset();
        self.silk.reset();
        self.frame_size = self.fs / 400;
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn sample_rate(&self) -> i32 {
        self.fs
    }

    pub fn final_range(&self) -> u32 {
        self.range_final
    }

    pub fn last_packet_duration(&self) -> i32 {
        self.last_packet_duration
    }

    pub fn set_gain(&mut self, q8: i32) -> i32 {
        if !(-32768..=32767).contains(&q8) {
            return OPUS_BAD_ARG;
        }
        self.decode_gain = q8;
        OPUS_OK
    }

    fn decode_frame(&mut self, data_in: Option<&[u8]>, pcm: &mut [f32], frame_size_in: i32, decode_fec: bool) -> i32 {
        let ch = self.channels;
        let f20 = self.fs / 50;
        let f10 = f20 >> 1;
        let f5 = f10 >> 1;
        let f2_5 = f5 >> 1;
        if frame_size_in < f2_5 {
            return OPUS_BUFFER_TOO_SMALL;
        }
        let mut frame_size = frame_size_in.min(self.fs / 25 * 3);
        let mut data = data_in;
        let mut len = data.map_or(0, |d| d.len() as i32);
        if len <= 1 {
            data = None;
            frame_size = frame_size.min(self.frame_size);
        }
        let dbuf: &[u8] = data.unwrap_or(&[]);
        let mut dec = RangeDecoder::new(dbuf);
        let mut audiosize;
        let mode;
        let bandwidth;
        if data.is_some() {
            audiosize = self.frame_size;
            mode = self.mode;
            bandwidth = self.bandwidth;
        } else {
            audiosize = frame_size;
            mode = if self.prev_redundancy { MODE_CELT_ONLY } else { self.prev_mode };
            bandwidth = 0;
            if mode == 0 {
                let n = (audiosize.max(0) as usize * ch).min(pcm.len());
                pcm[..n].fill(0.0);
                return audiosize;
            }
            if audiosize > f20 {
                let mut off = 0usize;
                let mut left = audiosize;
                while left > 0 {
                    if off > pcm.len() {
                        return OPUS_BUFFER_TOO_SMALL;
                    }
                    let r = self.decode_frame(None, &mut pcm[off..], left.min(f20), false);
                    if r < 0 {
                        return r;
                    }
                    if r == 0 {
                        return OPUS_INTERNAL_ERROR;
                    }
                    off += r as usize * ch;
                    left -= r;
                }
                return frame_size;
            } else if audiosize < f20 {
                if audiosize > f10 {
                    audiosize = f10;
                } else if mode != MODE_SILK_ONLY && audiosize > f5 && audiosize < f10 {
                    audiosize = f5;
                }
            }
        }
        let celt_accum = mode != MODE_CELT_ONLY;
        let mut transition = data.is_some()
            && self.prev_mode > 0
            && ((mode == MODE_CELT_ONLY && self.prev_mode != MODE_CELT_ONLY && !self.prev_redundancy) || (mode != MODE_CELT_ONLY && self.prev_mode == MODE_CELT_ONLY));
        let mut pcm_transition: Vec<f32> = Vec::new();
        if transition && mode == MODE_CELT_ONLY {
            pcm_transition = vec![0.0; f5 as usize * ch];
            self.decode_frame(None, &mut pcm_transition, f5.min(audiosize), false);
        }
        if audiosize > frame_size {
            return OPUS_BAD_ARG;
        }
        frame_size = audiosize;
        let fsz = frame_size as usize;
        if pcm.len() < fsz * ch {
            return OPUS_BUFFER_TOO_SMALL;
        }
        if mode != MODE_CELT_ONLY {
            let pcm_too_small = frame_size < f10;
            let mut silk_buf = if pcm_too_small { vec![0.0f32; f10 as usize * ch] } else { Vec::new() };
            if self.prev_mode == MODE_CELT_ONLY {
                self.silk.reset();
            }
            self.dec_control.payload_size_ms = 10.max(1000 * audiosize / self.fs);
            if data.is_some() {
                self.dec_control.n_channels_internal = self.stream_channels as i32;
                self.dec_control.internal_sample_rate = if mode == MODE_SILK_ONLY {
                    match bandwidth {
                        BANDWIDTH_NARROWBAND => 8000,
                        BANDWIDTH_MEDIUMBAND => 12000,
                        _ => 16000,
                    }
                } else {
                    16000
                };
            }
            let lost_flag = if data.is_none() { 1 } else if decode_fec { 2 } else { 0 };
            let mut decoded = 0i32;
            loop {
                let first = decoded == 0;
                let mut sfs = 0i32;
                let off = decoded as usize * ch;
                let target: &mut [f32] = if pcm_too_small { &mut silk_buf[off.min(f10 as usize * ch)..] } else { &mut pcm[off.min(fsz * ch)..] };
                let ret = self.silk.decode(&mut self.dec_control, lost_flag, first, &mut dec, target, &mut sfs);
                if ret != 0 {
                    if lost_flag != 0 {
                        sfs = frame_size - decoded;
                        let n = (sfs.max(0) as usize * ch).min(target.len());
                        target[..n].fill(0.0);
                    } else {
                        return OPUS_INTERNAL_ERROR;
                    }
                }
                if sfs <= 0 {
                    return OPUS_INTERNAL_ERROR;
                }
                decoded += sfs;
                if decoded >= frame_size {
                    break;
                }
            }
            if pcm_too_small {
                pcm[..fsz * ch].copy_from_slice(&silk_buf[..fsz * ch]);
            }
        }
        let mut start_band = 0;
        let mut redundancy = false;
        let mut redundancy_bytes = 0i32;
        let mut celt_to_silk = false;
        if !decode_fec && mode != MODE_CELT_ONLY && data.is_some() && dec.tell() + 17 + 20 * (mode == MODE_HYBRID) as i32 <= 8 * len {
            redundancy = if mode == MODE_HYBRID { dec.bit_logp(12) } else { true };
            if redundancy {
                celt_to_silk = dec.bit_logp(1);
                redundancy_bytes = if mode == MODE_HYBRID { dec.dec_uint(256) as i32 + 2 } else { len - ((dec.tell() + 7) >> 3) };
                len -= redundancy_bytes;
                if len * 8 < dec.tell() || len < 0 {
                    len = 0;
                    redundancy_bytes = 0;
                    redundancy = false;
                }
                dec.storage = dec.storage.saturating_sub(redundancy_bytes as u32);
            }
        }
        if mode != MODE_CELT_ONLY {
            start_band = 17;
        }
        if redundancy {
            transition = false;
        }
        if transition && mode != MODE_CELT_ONLY {
            pcm_transition = vec![0.0; f5 as usize * ch];
            self.decode_frame(None, &mut pcm_transition, f5.min(audiosize), false);
        }
        if bandwidth != 0 {
            let endband = match bandwidth {
                BANDWIDTH_NARROWBAND => 13,
                BANDWIDTH_MEDIUMBAND | BANDWIDTH_WIDEBAND => 17,
                BANDWIDTH_SUPERWIDEBAND => 19,
                _ => 21,
            };
            self.celt.set_end_band(endband);
        }
        self.celt.set_stream_channels(self.stream_channels);
        let lenu = len.max(0) as usize;
        let rb = redundancy_bytes.max(0) as usize;
        let red_slice: &[u8] = if redundancy && lenu + rb <= dbuf.len() { &dbuf[lenu..lenu + rb] } else { &[] };
        let mut redundant_audio = if redundancy { vec![0.0f32; f5 as usize * ch] } else { Vec::new() };
        let mut redundant_rng = 0u32;
        if redundancy && celt_to_silk {
            self.celt.set_start_band(0);
            self.celt.decode_with_ec(Some(red_slice), &mut redundant_audio, f5 as usize, None, false);
            redundant_rng = self.celt.final_range();
        }
        self.celt.set_start_band(start_band);
        let mut celt_ret = 0;
        if mode != MODE_SILK_ONLY {
            let celt_frame_size = f20.min(frame_size) as usize;
            if mode != self.prev_mode && self.prev_mode > 0 && !self.prev_redundancy {
                self.celt.reset();
            }
            let d = if decode_fec || data.is_none() { None } else { Some(&dbuf[..lenu.min(dbuf.len())]) };
            celt_ret = self.celt.decode_with_ec(d, pcm, celt_frame_size, Some(&mut dec), celt_accum);
            self.range_final = self.celt.final_range();
        } else {
            let silence = [0xFFu8, 0xFF];
            if !celt_accum {
                pcm[..fsz * ch].fill(0.0);
            }
            if self.prev_mode == MODE_HYBRID && !(redundancy && celt_to_silk && self.prev_redundancy) {
                self.celt.set_start_band(0);
                self.celt.decode_with_ec(Some(&silence), pcm, f2_5 as usize, None, celt_accum);
            }
            self.range_final = if data.is_some() { dec.rng } else { 0 };
        }
        let window = CeltDecoder::window();
        let f25 = f2_5 as usize;
        if redundancy && !celt_to_silk {
            self.celt.reset();
            self.celt.set_start_band(0);
            self.celt.decode_with_ec(Some(red_slice), &mut redundant_audio, f5 as usize, None, false);
            redundant_rng = self.celt.final_range();
            let o = ch * (fsz - f25);
            let in1 = pcm[o..].to_vec();
            smooth_fade(&in1, &redundant_audio[ch * f25..], &mut pcm[o..], f25, ch, window, self.fs);
        }
        if redundancy && celt_to_silk && (self.prev_mode != MODE_SILK_ONLY || self.prev_redundancy) {
            pcm[..ch * f25].copy_from_slice(&redundant_audio[..ch * f25]);
            let in2 = pcm[ch * f25..].to_vec();
            smooth_fade(&redundant_audio[ch * f25..], &in2, &mut pcm[ch * f25..], f25, ch, window, self.fs);
        }
        if transition && !pcm_transition.is_empty() {
            if audiosize >= f5 {
                pcm[..ch * f25].copy_from_slice(&pcm_transition[..ch * f25]);
                let in2 = pcm[ch * f25..].to_vec();
                smooth_fade(&pcm_transition[ch * f25..], &in2, &mut pcm[ch * f25..], f25, ch, window, self.fs);
            } else {
                let in2 = pcm.to_vec();
                smooth_fade(&pcm_transition, &in2, pcm, f25, ch, window, self.fs);
            }
        }
        if self.decode_gain != 0 {
            let gain = (6.48814081e-4f32 * self.decode_gain as f32).exp2();
            for v in pcm[..fsz * ch].iter_mut() {
                *v *= gain;
            }
        }
        if len <= 1 {
            self.range_final = 0;
        } else {
            self.range_final ^= redundant_rng;
        }
        self.prev_mode = mode;
        self.prev_redundancy = redundancy && !celt_to_silk;
        if celt_ret < 0 {
            celt_ret
        } else {
            audiosize
        }
    }

    pub fn decode_native(&mut self, data: &[u8], pcm: &mut [f32], frame_size: i32, decode_fec: bool, self_delimited: bool) -> (i32, usize) {
        let ch = self.channels;
        if frame_size <= 0 {
            return (OPUS_BAD_ARG, 0);
        }
        if (decode_fec || data.is_empty()) && frame_size % (self.fs / 400) != 0 {
            return (OPUS_BAD_ARG, 0);
        }
        if pcm.len() < frame_size as usize * ch {
            return (OPUS_BUFFER_TOO_SMALL, 0);
        }
        if data.is_empty() {
            let mut count = 0i32;
            while count < frame_size {
                let r = self.decode_frame(None, &mut pcm[count as usize * ch..], frame_size - count, false);
                if r < 0 {
                    return (r, 0);
                }
                if r == 0 {
                    return (OPUS_INTERNAL_ERROR, 0);
                }
                count += r;
            }
            self.last_packet_duration = count;
            return (count, 0);
        }
        let pkt_mode = packet_mode(data[0]);
        let pkt_bw = packet_bandwidth(data[0]);
        let pkt_fs = samples_per_frame(data[0], self.fs);
        let pkt_ch = packet_channels(data[0]);
        let parsed = match parse_packet(data, self_delimited) {
            Ok(p) => p,
            Err(e) => return (e, 0),
        };
        let consumed = parsed.packet_offset;
        let mut off = parsed.payload_offset;
        if decode_fec {
            if frame_size < pkt_fs || pkt_mode == MODE_CELT_ONLY || self.mode == MODE_CELT_ONLY {
                let r = self.decode_native(&[], pcm, frame_size, false, false).0;
                return (r, consumed);
            }
            let duration_copy = self.last_packet_duration;
            if frame_size - pkt_fs != 0 {
                let r = self.decode_native(&[], pcm, frame_size - pkt_fs, false, false).0;
                if r < 0 {
                    self.last_packet_duration = duration_copy;
                    return (r, consumed);
                }
            }
            self.mode = pkt_mode;
            self.bandwidth = pkt_bw;
            self.frame_size = pkt_fs;
            self.stream_channels = pkt_ch;
            let fdata = &data[off..off + parsed.sizes[0]];
            let r = self.decode_frame(Some(fdata), &mut pcm[ch * (frame_size - pkt_fs) as usize..], pkt_fs, true);
            if r < 0 {
                return (r, consumed);
            }
            self.last_packet_duration = frame_size;
            return (frame_size, consumed);
        }
        if parsed.sizes.len() as i32 * pkt_fs > frame_size {
            return (OPUS_BUFFER_TOO_SMALL, consumed);
        }
        self.mode = pkt_mode;
        self.bandwidth = pkt_bw;
        self.frame_size = pkt_fs;
        self.stream_channels = pkt_ch;
        let mut nb = 0i32;
        for &sz in &parsed.sizes {
            let fdata = &data[off..off + sz];
            let r = self.decode_frame(Some(fdata), &mut pcm[nb as usize * ch..], frame_size - nb, false);
            if r < 0 {
                return (r, consumed);
            }
            off += sz;
            nb += r;
        }
        self.last_packet_duration = nb;
        self.softclip_mem = [0.0; 2];
        (nb, consumed)
    }

    pub fn decode_float(&mut self, data: &[u8], pcm: &mut [f32], frame_size: i32, decode_fec: bool) -> i32 {
        self.decode_native(data, pcm, frame_size, decode_fec, false).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ogg::parse_opus_file;

    fn refs_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("TOMBOX_OPUS_REFS").unwrap_or_else(|_| r"D:\xbox-pc-port\emulator\third_party\opus-refs".into()))
    }

    fn decode_file(name: &str) -> (Vec<f32>, Vec<f32>) {
        let dir = refs_dir();
        let f = parse_opus_file(&std::fs::read(dir.join(format!("{name}.opus"))).expect("opus ref")).unwrap();
        let reference: Vec<f32> = std::fs::read(dir.join(format!("{name}.f32"))).expect("f32 ref").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let mut d = OpusDecoder::new(48000, f.channels).unwrap();
        let mut out = Vec::new();
        let mut pcm = vec![0f32; 5760 * f.channels];
        for p in &f.packets {
            let n = d.decode_float(p, &mut pcm, 5760, false);
            assert!(n > 0, "{name}: decode error {n}");
            out.extend_from_slice(&pcm[..n as usize * f.channels]);
        }
        (out[f.pre_skip * f.channels..].to_vec(), reference)
    }

    fn to_s16(v: f32) -> i32 {
        (v * 32768.0).round().clamp(-32768.0, 32767.0) as i32
    }

    #[test]
    fn opus_matches_libopus_all_modes() {
        for name in ["celt", "celtmono", "silk", "silkst", "hybrid"] {
            let (ours, reference) = decode_file(name);
            let n = ours.len().min(reference.len());
            assert!(n * 100 >= reference.len() * 99, "{name}: {} vs {}", ours.len(), reference.len());
            let mut max_lsb = 0;
            let mut diffs = 0;
            for i in 0..n {
                let d = (to_s16(ours[i]) - to_s16(reference[i])).abs();
                max_lsb = max_lsb.max(d);
                if d != 0 {
                    diffs += 1;
                }
            }
            eprintln!("{name}: compared {n} samples, max diff {max_lsb} LSB, {diffs} differing");
            assert!(max_lsb <= 1, "{name}: max diff {max_lsb} LSB");
        }
    }

    #[test]
    fn packet_parse_codes() {
        assert_eq!(parse_packet(&[0x08, 1, 2, 3], false).unwrap().sizes, vec![3]);
        assert_eq!(parse_packet(&[0x09, 1, 2, 3, 4], false).unwrap().sizes, vec![2, 2]);
        assert!(parse_packet(&[0x09, 1, 2, 3], false).is_err());
        assert_eq!(parse_packet(&[0x0a, 1, 9, 2, 3], false).unwrap().sizes, vec![1, 2]);
        assert_eq!(parse_packet(&[0x0b, 0x03, 1, 1, 1], false).unwrap().sizes, vec![1, 1, 1]);
        assert_eq!(parse_packet(&[0x0b, 0x83, 1, 2, 7, 8, 8, 9], false).unwrap().sizes, vec![1, 2, 1]);
        assert_eq!(parse_packet(&[0x0b, 0x41, 2, 5, 0, 0], false).unwrap().sizes, vec![1]);
        assert!(parse_packet(&[], false).is_err());
        let sd = parse_packet(&[0x08, 2, 7, 7, 0x99], true).unwrap();
        assert_eq!((sd.sizes.clone(), sd.packet_offset), (vec![2], 4));
    }

    #[test]
    fn plc_and_garbage_never_panic() {
        let mut rng = 0x1234_5678u32;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        for ch in [1usize, 2] {
            for fs in [8000, 12000, 16000, 24000, 48000] {
                let mut d = OpusDecoder::new(fs, ch).unwrap();
                let mut pcm = vec![0f32; 5760 * 2];
                assert_eq!(d.decode_float(&[], &mut pcm, fs / 50, false), fs / 50);
                for k in 0..600 {
                    let len = (next() % 400) as usize;
                    let p: Vec<u8> = (0..len).map(|_| next() as u8).collect();
                    let fsz = [fs / 400, fs / 200, fs / 100, fs / 50, fs / 25, fs * 3 / 50, fs * 3 / 25][k % 7];
                    let _ = d.decode_float(&p, &mut pcm, fsz, k % 11 == 0);
                    if k % 13 == 0 {
                        let _ = d.decode_float(&[], &mut pcm, fs / 50, false);
                    }
                    if k % 97 == 0 {
                        d.reset();
                    }
                }
            }
        }
        let dir = refs_dir();
        for name in ["celt", "silk", "silkst", "hybrid"] {
            let Ok(bytes) = std::fs::read(dir.join(format!("{name}.opus"))) else { continue };
            let f = parse_opus_file(&bytes).unwrap();
            let mut d = OpusDecoder::new(48000, f.channels).unwrap();
            let mut pcm = vec![0f32; 5760 * 2];
            for (i, p) in f.packets.iter().enumerate() {
                let mut q = p.clone();
                match i % 5 {
                    0 => {
                        let b = (next() as usize) % q.len().max(1);
                        if !q.is_empty() {
                            q[b] ^= 1 << (next() % 8);
                        }
                    }
                    1 => q.truncate((next() as usize) % (q.len() + 1)),
                    2 => q.clear(),
                    _ => {}
                }
                let _ = d.decode_float(&q, &mut pcm, 5760, i % 7 == 0);
            }
        }
    }
}
