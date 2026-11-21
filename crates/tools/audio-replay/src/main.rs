use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn wav(path: &Path, channels: u16, rate: u32, float: bool, data: &[u8]) -> std::io::Result<()> {
    let bits: u16 = if float { 32 } else { 16 };
    let align = channels * bits / 8;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data.len() as u32).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&(if float { 3u16 } else { 1u16 }).to_le_bytes())?;
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * align as u32).to_le_bytes())?;
    f.write_all(&align.to_le_bytes())?;
    f.write_all(&bits.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&(data.len() as u32).to_le_bytes())?;
    f.write_all(data)?;
    f.flush()
}

fn run(input: &Path, out_dir: &Path) -> Result<String, String> {
    let mut magic = [0u8; 8];
    std::fs::File::open(input).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| format!("{}: {e}", input.display()))?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let (report, csv) = if &magic == acphal::audiorec::MAGIC {
        let r = acphal::audiorec::replay(input)?;
        for o in &r.outputs {
            wav(&out_dir.join(format!("{}.wav", o.name)), o.channels, o.rate, o.float, &o.data).map_err(|e| e.to_string())?;
        }
        (r.report, r.csv)
    } else if &magic == xapu::rec::MAGIC {
        let r = xapu::rec::replay(input)?;
        for o in &r.outputs {
            let bytes: Vec<u8> = o.data.iter().flat_map(|v| v.to_le_bytes()).collect();
            wav(&out_dir.join(format!("{}.wav", o.name)), o.channels, o.rate, true, &bytes).map_err(|e| e.to_string())?;
        }
        (r.report, r.csv)
    } else {
        return Err(format!("{}: unknown recording magic {:?}", input.display(), String::from_utf8_lossy(&magic)));
    };
    std::fs::write(out_dir.join("report.txt"), &report).map_err(|e| e.to_string())?;
    std::fs::write(out_dir.join("depth.csv"), &csv).map_err(|e| e.to_string())?;
    Ok(report)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: audio-replay <recording.tbrec> [out_dir]");
        std::process::exit(2);
    }
    let input = PathBuf::from(&args[1]);
    let out = args.get(2).map(PathBuf::from).unwrap_or_else(|| input.with_extension("replay"));
    match run(&input, &out) {
        Ok(report) => {
            print!("{report}");
            println!("\nwrote {}", out.display());
        }
        Err(e) => {
            eprintln!("audio-replay: {e}");
            std::process::exit(1);
        }
    }
}
