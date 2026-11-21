use super::super::{Request, Response};
use serde_json::json;

pub const INIT_MP4: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/backend_data/fortnite/video/init.mp4"));
pub const SEG0_M4S: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/backend_data/fortnite/video/seg0.m4s"));

pub const AUDIO_INIT_MP4: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/backend_data/fortnite/video/audio_init.mp4"));
pub const AUDIO_SEG0_M4S: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/backend_data/fortnite/video/audio_seg0.m4s"));

const MASTER_M3U8: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=320000,AVERAGE-BANDWIDTH=290000,CODECS=\"avc1.4d401e,mp4a.40.2\",RESOLUTION=640x360,FRAME-RATE=30.000,AUDIO=\"aud\"\nstream.m3u8\n";
const AUDIO_M3U8: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"audio_init.mp4\"\n#EXTINF:1.021333,\naudio_seg0.m4s\n#EXT-X-ENDLIST\n";
const STREAM_M3U8: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.000000,\nseg0.m4s\n#EXT-X-ENDLIST\n";
const HLS: &str = "application/vnd.apple.mpegurl";

fn stream_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_blurl_name(f: &str) -> bool {
    f.starts_with("master") && (f.ends_with(".blurl") || f.ends_with(".json"))
}

fn clean_host(h: &str) -> Option<String> {
    let h = h.trim();
    (!h.is_empty() && h.len() <= 255 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b':' || b == b'[' || b == b']'))
        .then(|| h.to_ascii_lowercase())
}

fn is_loopback_host(h: &str) -> bool {
    let name = if h.starts_with('[') { h.split(']').next().unwrap_or("").trim_start_matches('[') } else { h.split(':').next().unwrap_or("") };
    matches!(name, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
}

pub fn master_url(req: &Request, id: &str) -> String {
    if let Some(h) = req.header("x-forwarded-host").and_then(clean_host).filter(|h| !is_loopback_host(h)) {
        return format!("https://{h}/{id}/master.m3u8");
    }
    if let Some(h) = req.header("host").and_then(clean_host) {
        if !is_loopback_host(&h) {
            return format!("https://{h}/{id}/master.m3u8");
        }
        if h.contains(':') {
            return format!("http://{h}/{id}/master.m3u8");
        }
    }
    match super::super::port() {
        Some(p) => format!("http://127.0.0.1:{p}/{id}/master.m3u8"),
        None => format!("http://127.0.0.1/{id}/master.m3u8"),
    }
}

pub fn blurl_body(master: &str) -> serde_json::Value {
    json!({
        "playlists": [
            {"type": "master", "language": "en", "url": master}
        ],
        "audioonly": false,
    })
}

fn trace(req: &Request, what: &str) {
    let range = req.header("range").unwrap_or("-");
    crate::record_raw("online", "video", &format!("{} {} -> {what} (range {range})", req.method, req.path));
}

pub fn route(req: &Request, m: &str, seg: &[&str]) -> Option<Response> {
    if m != "GET" && m != "HEAD" {
        return None;
    }
    let (id, file, nested) = match seg {
        [id, file] => (*id, *file, false),
        [id, mid, file] if is_blurl_name(mid) => (*id, *file, true),
        _ => return None,
    };
    if !stream_id(id) {
        return None;
    }
    let resp = if is_blurl_name(file) && !nested {
        let master = master_url(req, id);
        trace(req, &format!("blurl playlists (in-process 1s blue-gradient HLS + silent AAC) master url {master}"));
        Response::json(&blurl_body(&master))
    } else {
        match file {
            "master.m3u8" => Response::bytes(200, HLS, MASTER_M3U8.as_bytes().to_vec()),
            "stream.m3u8" => Response::bytes(200, HLS, STREAM_M3U8.as_bytes().to_vec()),
            "init.mp4" => Response::bytes(200, "video/mp4", INIT_MP4.to_vec()),
            "seg0.m4s" => Response::bytes(200, "video/iso.segment", SEG0_M4S.to_vec()),
            "audio.m3u8" => Response::bytes(200, HLS, AUDIO_M3U8.as_bytes().to_vec()),
            "audio_init.mp4" => Response::bytes(200, "audio/mp4", AUDIO_INIT_MP4.to_vec()),
            "audio_seg0.m4s" => Response::bytes(200, "audio/iso.segment", AUDIO_SEG0_M4S.to_vec()),
            _ => return None,
        }
    };
    if nested || !is_blurl_name(file) {
        trace(req, file);
    }
    Some(resp)
}
