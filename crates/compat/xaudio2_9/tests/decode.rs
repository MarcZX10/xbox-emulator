mod common;

use common::*;
use XAudio2_9::xma_stream::XmaDecoder;

#[test]
fn synthesized_mono_packets_decode_to_expected_frames() {
    let mut d = XmaDecoder::new(48000, 1).unwrap();
    let data = silent_stream(1, 2, 8);
    let mut out = Vec::new();
    let n = d.feed(&data, &mut out);
    assert_eq!(n as u32, expected_frames(2, 8));
    assert_eq!(out.len(), n);
    assert!(out.iter().all(|s| *s == 0));
}

#[test]
fn synthesized_stereo_packets_decode_to_expected_frames() {
    let mut d = XmaDecoder::new(44100, 2).unwrap();
    let data = silent_stream(2, 3, 5);
    let mut out = Vec::new();
    let n = d.feed(&data, &mut out);
    assert_eq!(n as u32, expected_frames(3, 5));
    assert_eq!(out.len(), n * 2);
}

#[test]
fn synthesized_four_channel_multistream_interleaves_both_streams() {
    let mut d = XmaDecoder::new(48000, 4).unwrap();
    let data = silent_stream(4, 2, 6);
    let mut owners = Vec::new();
    let mut out = Vec::new();
    let mut total = 0;
    for pkt in data.chunks(PACKET) {
        owners.push(d.owner_of_next_packet());
        total += d.feed(pkt, &mut out);
    }
    assert_eq!(owners, vec![0, 1, 0, 1]);
    assert_eq!(total as u32, expected_frames(2, 6));
    assert_eq!(out.len(), total * 4);
    assert_eq!(d.packets_decoded(), 4);
}

#[test]
fn three_channel_layout_uses_stereo_then_mono_stream() {
    let mut d = XmaDecoder::new(48000, 3).unwrap();
    let data = silent_stream(3, 2, 4);
    let mut out = Vec::new();
    let n = d.feed(&data, &mut out);
    assert_eq!(n as u32, expected_frames(2, 4));
    assert_eq!(out.len(), n * 3);
}
