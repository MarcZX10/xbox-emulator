mod common;

use common::*;
use XAudio2_9::xma_stream::{validate_format, XmaDecoder};

fn decode_all(r: &Riff) -> Vec<i16> {
    let w = r.fmt.wfx;
    let mut d = XmaDecoder::new({ w.n_samples_per_sec }, { w.n_channels } as usize).unwrap();
    let mut out = Vec::new();
    d.feed(&r.data, &mut out);
    out
}

fn rms(s: &[i16]) -> f64 {
    (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

#[test]
fn real_xma2_decodes_to_samples_encoded_on_every_layout() {
    for src in [MONO, STEREO, QUAD, SURROUND] {
        let r = load(src.0, src.1, src.2);
        assert!(validate_format(&r.fmt).is_ok(), "{}", src.0);
        let ch = { r.fmt.wfx.n_channels } as usize;
        let pcm = decode_all(&r);
        assert_eq!(pcm.len() % ch, 0);
        let frames = (pcm.len() / ch) as u32;
        assert_eq!(frames, { r.fmt.samples_encoded }, "{} {}ch", src.0, ch);
        assert!(frames >= { r.fmt.play_begin } + { r.fmt.play_length });
        for c in 0..ch {
            let lane: Vec<i16> = pcm.iter().skip(c).step_by(ch).copied().collect();
            let lfe = { r.fmt.channel_mask } & 0x8 != 0 && c == 3;
            let floor = if lfe { 5.0 } else { 30.0 };
            assert!(rms(&lane) > floor, "{} channel {c} is silent (rms {:.1})", src.0, rms(&lane));
        }
    }
}

#[test]
fn real_xma2_after_first_frame_matches_ffmpeg_semantics_exactly() {
    for src in [MONO, STEREO] {
        let r = load(src.0, src.1, src.2);
        let w = r.fmt.wfx;
        let ch = { w.n_channels } as usize;
        let ours = decode_all(&r);
        let mut ff = xma::Xma2Stream::new({ w.n_samples_per_sec }, ch).unwrap();
        let mut theirs = Vec::new();
        for p in r.data.chunks(2048) {
            ff.decode_packet(p, &mut theirs);
        }
        let shift = (512 + xma::XMA_START_SKIP) * ch;
        assert_eq!(ours.len(), theirs.len() + shift);
        assert_eq!(&ours[shift..], &theirs[..], "{}", src.0);
        assert!(rms(&ours[..512 * ch]) < rms(&ours[512 * ch..]) + 1.0);
    }
}
