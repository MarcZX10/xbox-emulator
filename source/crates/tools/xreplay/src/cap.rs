use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub fn unhex(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut v = Vec::with_capacity(b.len() / 2);
    let d = |c: u8| -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        }
    };
    let mut i = 0;
    while i + 1 < b.len() {
        v.push(d(b[i]) << 4 | d(b[i + 1]));
        i += 2;
    }
    v
}

pub fn num(s: &str) -> u64 {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x") {
        u64::from_str_radix(h, 16).unwrap_or(0)
    } else {
        s.parse::<i64>().map(|v| v as u64).unwrap_or(0)
    }
}

pub fn kv<'a>(words: &[&'a str], key: &str) -> Option<&'a str> {
    let p = format!("{key}=");
    words.iter().find_map(|w| w.strip_prefix(p.as_str()))
}

#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct Execute {
    pub n: usize,
    pub queue: u64,
    pub list: u64,
    pub complete: bool,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, Default)]
pub struct PsoRec {
    pub compute: bool,
    pub root: u64,
    pub stages: Vec<(String, Option<u64>)>,
    pub registered: bool,
}

#[derive(Clone, Debug)]
pub struct SigRec {
    pub stride: u32,
    pub root: u64,
    pub args: Vec<[u32; 4]>,
}

#[derive(Clone, Debug)]
pub enum Slot {
    Cbv { va: u64, size: u32 },
    Srv { res: u64, desc: Option<Vec<u8>> },
    Uav { res: u64, desc: Option<Vec<u8>> },
    Sampler { desc: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct HeapRec {
    pub cpu: u64,
    pub gpu: u64,
    pub count: u64,
    pub inc: u64,
    pub slots: BTreeMap<u64, Slot>,
}

#[derive(Clone, Debug)]
pub struct ViewRec {
    pub kind: char,
    pub res: u64,
    pub desc: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct BufRec {
    pub res: u64,
    pub va: u64,
    pub size: u64,
    pub heap: u32,
    pub state: Option<u32>,
    pub data: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ResRec {
    pub res: u64,
    pub desc: [u8; 0x38],
    pub state: Option<u32>,
    pub data: Option<PathBuf>,
}

#[derive(Default)]
pub struct Capture {
    pub dir: PathBuf,
    pub executes: Vec<Execute>,
    pub psos: HashMap<u64, PsoRec>,
    pub sigs: HashMap<u64, SigRec>,
    pub roots: HashMap<u64, Vec<u8>>,
    pub views: HashMap<u64, ViewRec>,
    pub heaps: Vec<HeapRec>,
    pub buffers: BTreeMap<u64, BufRec>,
    pub resources: HashMap<u64, ResRec>,
    pub notes: Vec<String>,
}

fn opt_state(s: Option<&str>) -> Option<u32> {
    match s {
        Some("unknown") | None => None,
        Some(v) => Some(num(v) as u32),
    }
}

fn data_path(dir: &Path, s: Option<&str>) -> Option<PathBuf> {
    match s {
        Some("none") | None => None,
        Some(f) => Some(dir.join(f)),
    }
}

impl Capture {
    pub fn load(dir: &Path) -> Result<Capture, String> {
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).map_err(|e| format!("{}: {e}", dir.join(n).display()));
        let mut c = Capture { dir: dir.to_path_buf(), ..Default::default() };
        let mut index = 0usize;
        for l in read("commands.txt")?.lines() {
            if let Some(rest) = l.strip_prefix("execute ") {
                let w: Vec<&str> = rest.split_whitespace().collect();
                c.executes.push(Execute {
                    n: w.first().map(|s| num(s.trim_start_matches('#')) as usize).unwrap_or(0),
                    queue: kv(&w, "queue").map(num).unwrap_or(0),
                    list: kv(&w, "list").map(num).unwrap_or(0),
                    complete: kv(&w, "complete") == Some("true"),
                    lines: Vec::new(),
                });
            } else if let Some(t) = l.strip_prefix("  ") {
                if let Some(e) = c.executes.last_mut() {
                    e.lines.push(Line { text: t.to_string(), index });
                    index += 1;
                }
            }
        }
        for l in read("objects.txt").unwrap_or_default().lines() {
            let w: Vec<&str> = l.split_whitespace().collect();
            if w.len() < 2 {
                continue;
            }
            let id = num(w[1]);
            match w[0] {
                "pso" => {
                    let mut p = PsoRec { registered: w.get(2) != Some(&"UNREGISTERED"), compute: w.get(2) == Some(&"compute"), ..Default::default() };
                    p.root = kv(&w, "root").map(num).unwrap_or(0);
                    for st in ["vs", "ps", "ds", "hs", "gs", "cs"] {
                        if let Some(v) = kv(&w, st) {
                            p.stages.push((st.to_string(), if v == "unknown" { None } else { u64::from_str_radix(v, 16).ok() }));
                        }
                    }
                    c.psos.insert(id, p);
                }
                "signature" if w.get(2) != Some(&"UNREGISTERED") => {
                    let args = kv(&w, "args")
                        .unwrap_or("")
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|a| {
                            let mut x = [0u32; 4];
                            for (i, p) in a.split(':').take(4).enumerate() {
                                x[i] = p.parse().unwrap_or(0);
                            }
                            x
                        })
                        .collect();
                    c.sigs.insert(id, SigRec { stride: kv(&w, "stride").map(num).unwrap_or(0) as u32, root: kv(&w, "root").map(num).unwrap_or(0), args });
                }
                "rootsig" if w.get(2) != Some(&"UNREGISTERED") => {
                    if let Ok(b) = std::fs::read(dir.join("rootsig").join(format!("{id:x}.bin"))) {
                        c.roots.insert(id, b);
                    }
                }
                _ => {}
            }
        }
        for l in read("descriptors.txt").unwrap_or_default().lines() {
            let w: Vec<&str> = l.split_whitespace().collect();
            if w.is_empty() {
                continue;
            }
            if (w[0] == "rv" || w[0] == "dv") && w.len() >= 2 {
                let desc = kv(&w, "desc").filter(|d| *d != "null").map(unhex);
                c.views.insert(num(w[1]), ViewRec { kind: w[0].as_bytes()[0] as char, res: kv(&w, "res").map(num).unwrap_or(0), desc });
            } else if w[0] == "heap" {
                c.heaps.push(HeapRec {
                    cpu: kv(&w, "cpu").map(num).unwrap_or(0),
                    gpu: kv(&w, "gpu").map(num).unwrap_or(0),
                    count: kv(&w, "count").map(num).unwrap_or(0),
                    inc: kv(&w, "inc").map(num).unwrap_or(0),
                    slots: BTreeMap::new(),
                });
            } else if l.starts_with("  ") && w.len() >= 2 {
                let slot = num(w[0]);
                let d = |k: &str| kv(&w, k).filter(|d| *d != "null").map(unhex);
                let s = match w[1] {
                    "cbv" => Slot::Cbv { va: kv(&w, "va").map(num).unwrap_or(0), size: kv(&w, "size").map(num).unwrap_or(0) as u32 },
                    "srv" => Slot::Srv { res: kv(&w, "res").map(num).unwrap_or(0), desc: d("desc") },
                    "uav" => Slot::Uav { res: kv(&w, "res").map(num).unwrap_or(0), desc: d("desc") },
                    "sampler" => Slot::Sampler { desc: d("desc").unwrap_or_default() },
                    _ => continue,
                };
                if let Some(h) = c.heaps.last_mut() {
                    h.slots.insert(slot, s);
                }
            }
        }
        for l in read("resources.txt").unwrap_or_default().lines() {
            let w: Vec<&str> = l.split_whitespace().collect();
            match w.first() {
                Some(&"buffer") => {
                    let b = BufRec {
                        res: kv(&w, "res").map(num).unwrap_or(0),
                        va: kv(&w, "va").map(num).unwrap_or(0),
                        size: kv(&w, "size").map(num).unwrap_or(0),
                        heap: kv(&w, "heap").map(num).unwrap_or(1) as u32,
                        state: opt_state(kv(&w, "state")),
                        data: data_path(dir, kv(&w, "data")),
                    };
                    c.buffers.insert(b.va, b);
                }
                Some(&"resource") => {
                    let mut desc = [0u8; 0x38];
                    let d = unhex(kv(&w, "desc").unwrap_or(""));
                    let n = d.len().min(0x38);
                    desc[..n].copy_from_slice(&d[..n]);
                    let r = ResRec { res: kv(&w, "res").map(num).unwrap_or(0), desc, state: opt_state(kv(&w, "state")), data: data_path(dir, kv(&w, "data")) };
                    c.resources.insert(r.res, r);
                }
                Some(_) => c.notes.push(l.to_string()),
                None => {}
            }
        }
        Ok(c)
    }

    pub fn buffer_of(&self, va: u64) -> Option<&BufRec> {
        self.buffers.range(..=va).next_back().map(|e| e.1).filter(|b| va < b.va + b.size.max(1))
    }

    pub fn buffer_by_res(&self, res: u64) -> Option<&BufRec> {
        self.buffers.values().find(|b| b.res == res)
    }

    pub fn heap_of_gpu(&self, gpu: u64) -> Option<(usize, u64)> {
        self.heaps.iter().enumerate().find_map(|(i, h)| {
            if h.inc != 0 && gpu >= h.gpu && gpu < h.gpu + h.count * h.inc {
                Some((i, (gpu - h.gpu) / h.inc))
            } else {
                None
            }
        })
    }

    pub fn heap_of_cpu(&self, cpu: u64) -> Option<(usize, u64)> {
        self.heaps.iter().enumerate().find_map(|(i, h)| {
            if h.inc != 0 && cpu >= h.cpu && cpu < h.cpu + h.count * h.inc {
                Some((i, (cpu - h.cpu) / h.inc))
            } else {
                None
            }
        })
    }

    pub fn is_sampler_heap(&self, i: usize) -> bool {
        self.heaps[i].slots.values().any(|s| matches!(s, Slot::Sampler { .. }))
    }
}

pub fn table_sizes(blob: &[u8]) -> BTreeMap<u32, (u32, usize)> {
    let mut out = BTreeMap::new();
    let rd = |b: &[u8], o: usize| -> Option<u32> { b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())) };
    let mut rts = blob;
    if blob.len() >= 32 && &blob[0..4] == b"DXBC" {
        let n = rd(blob, 28).unwrap_or(0) as usize;
        let mut found = None;
        for i in 0..n.min(64) {
            let Some(off) = rd(blob, 32 + i * 4) else { break };
            let off = off as usize;
            if blob.get(off..off + 4) == Some(b"RTS0") {
                let sz = rd(blob, off + 4).unwrap_or(0) as usize;
                found = blob.get(off + 8..(off + 8 + sz).min(blob.len()));
            }
        }
        match found {
            Some(f) => rts = f,
            None => return out,
        }
    }
    let (Some(ver), Some(np), Some(po)) = (rd(rts, 0), rd(rts, 4), rd(rts, 8)) else { return out };
    let rsize = if ver >= 2 { 24 } else { 20 };
    for p in 0..np.min(64) as usize {
        let base = po as usize + p * 12;
        let (Some(ty), Some(payload)) = (rd(rts, base), rd(rts, base + 8)) else { break };
        if ty != 0 {
            out.insert(p as u32, (ty, 0));
            continue;
        }
        let (Some(nr), Some(ro)) = (rd(rts, payload as usize), rd(rts, payload as usize + 4)) else { continue };
        let mut end = 0usize;
        let mut run = 0usize;
        for r in 0..nr.min(64) as usize {
            let rb = ro as usize + r * rsize;
            let Some(count) = rd(rts, rb + 4) else { break };
            let Some(offset) = rd(rts, rb + rsize - 4) else { break };
            let count = if count == u32::MAX { 64 } else { (count as usize).min(4096) };
            let start = if offset == u32::MAX { run } else { offset as usize };
            run = start + count;
            end = end.max(run);
        }
        out.insert(p as u32, (0, end));
    }
    out
}
