use compat_common::record;

const S_OK: i32 = 0;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const DXGI_FORMAT_NV12: u32 = 103;
const DXGI_FORMAT_YUY2: u32 = 107;
const MAX_DIMENSION: u16 = 0x2000;

fn byte(d: &[u8], at: usize) -> u8 {
    d.get(at).copied().unwrap_or(0)
}

fn be16(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([byte(d, at), byte(d, at + 1)])
}

fn log(msg: &str) {
    record("xg_x", "XGValidateJpegFile", msg);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Halt {
    Stalled,
    NullPayload,
}

pub fn validate(d: &[u8], out: &mut [u32]) -> i32 {
    match walk(d, out) {
        Ok(hr) => hr,
        Err(Halt::NullPayload) => {
            log("frame or scan header with no payload inside the buffer; the console dereferences a null payload pointer here");
            E_FAIL
        }
        Err(Halt::Stalled) => {
            log("marker walk makes no progress (zero step); the console loops here forever");
            loop {
                core::hint::spin_loop();
            }
        }
    }
}

pub fn walk(d: &[u8], out: &mut [u32]) -> Result<i32, Halt> {
    let end = d.len();
    let mut p = 0usize;
    let mut ok = true;
    let mut seen_soi = false;
    let mut scan_skip: u32 = 0;
    while p < end && ok {
        let marker = be16(d, p);
        let mut seg: u32 = 0;
        let mut payload: Option<usize> = None;
        if marker == 0xffd8 {
            seen_soi = true;
        } else {
            if !seen_soi {
                log("stream does not start with SOI");
                return Ok(E_FAIL);
            }
            if p + 2 < end {
                seg = be16(d, p + 2) as u32;
                if p + seg as usize >= end {
                    log(&format!("segment {marker:#06x} length {seg} runs past the end"));
                    ok = false;
                    continue;
                }
                if p + 4 < end {
                    payload = Some(p + 4);
                }
            }
        }
        match marker {
            0xffc0 => {
                if seg.wrapping_sub(2) < 6 {
                    log("SOF0 segment too short");
                    ok = false;
                } else {
                    let q = payload.ok_or(Halt::NullPayload)?;
                    let (precision, height, width, ncomp) = (byte(d, q), be16(d, q + 1), be16(d, q + 3), byte(d, q + 5));
                    out[1] = width as u32;
                    out[2] = height as u32;
                    if precision != 8 {
                        log(&format!("sample precision {precision} (only 8-bit baseline decodes)"));
                        ok = false;
                    } else if height > MAX_DIMENSION || width > MAX_DIMENSION {
                        log(&format!("{width}x{height} exceeds the 8192x8192 decoder limit"));
                        ok = false;
                    } else if ncomp != 3 {
                        log(&format!("{ncomp} components (only 3-component YCbCr decodes)"));
                        ok = false;
                    } else {
                        let mut good = true;
                        for c in 0..3usize {
                            let (id, samp) = (byte(d, q + 6 + c * 3), byte(d, q + 7 + c * 3));
                            if id == 1 {
                                match samp {
                                    0x22 => out[0] = DXGI_FORMAT_NV12,
                                    0x21 => out[0] = DXGI_FORMAT_YUY2,
                                    0x11 => {
                                        log("4:4:4 luma sampling is not decodable");
                                        good = false;
                                    }
                                    _ => {
                                        log(&format!("luma sampling {samp:#x} is not decodable"));
                                        good = false;
                                    }
                                }
                            } else if samp != 0x11 {
                                log(&format!("chroma component {id} sampling {samp:#x} is not 1x1"));
                                good = false;
                            }
                        }
                        ok &= good;
                    }
                }
            }
            0xffd9 => scan_skip = 0,
            0xffda => {
                let good = if seg.wrapping_sub(2) < 7 {
                    false
                } else {
                    byte(d, payload.ok_or(Halt::NullPayload)?) == 3
                };
                if !good {
                    log("scan header is not a 3-component scan");
                }
                ok &= good;
                scan_skip = (end as u32).wrapping_sub(seg).wrapping_sub(p as u32).wrapping_sub(4);
            }
            0xffe1..=0xffef => out[3] = out[3].wrapping_add(seg + 2),
            _ => {}
        }
        let step = seg.wrapping_add(scan_skip).wrapping_add(2);
        if step == 0 && ok {
            return Err(Halt::Stalled);
        }
        p = p.saturating_add(step as usize);
    }
    Ok(if ok { S_OK } else { E_FAIL })
}