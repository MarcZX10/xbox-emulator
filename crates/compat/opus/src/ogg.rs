pub struct OpusFile {
    pub channels: usize,
    pub pre_skip: usize,
    pub gain_q8: i16,
    pub mapping_family: u8,
    pub stream_count: u8,
    pub coupled_count: u8,
    pub mapping: Vec<u8>,
    pub packets: Vec<Vec<u8>>,
}

pub fn ogg_packets(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut p = 0usize;
    while p + 27 <= data.len() {
        if &data[p..p + 4] != b"OggS" {
            break;
        }
        let nseg = data[p + 26] as usize;
        if p + 27 + nseg > data.len() {
            break;
        }
        let segs = &data[p + 27..p + 27 + nseg];
        let mut q = p + 27 + nseg;
        for &l in segs {
            let l = l as usize;
            if q + l > data.len() {
                return out;
            }
            cur.extend_from_slice(&data[q..q + l]);
            q += l;
            if l < 255 {
                out.push(std::mem::take(&mut cur));
            }
        }
        p = q;
    }
    out
}

pub fn parse_opus_file(data: &[u8]) -> Option<OpusFile> {
    let pk = ogg_packets(data);
    let head = pk.first()?;
    if head.len() < 19 || &head[..8] != b"OpusHead" {
        return None;
    }
    let channels = head[9] as usize;
    let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;
    let gain_q8 = i16::from_le_bytes([head[16], head[17]]);
    let family = head[18];
    let (sc, cc, map) = if family != 0 && head.len() >= 21 + channels {
        (head[19], head[20], head[21..21 + channels].to_vec())
    } else {
        (1, (channels == 2) as u8, (0..channels as u8).collect())
    };
    Some(OpusFile {
        channels,
        pre_skip,
        gain_q8,
        mapping_family: family,
        stream_count: sc,
        coupled_count: cc,
        mapping: map,
        packets: pk.into_iter().skip(2).collect(),
    })
}
