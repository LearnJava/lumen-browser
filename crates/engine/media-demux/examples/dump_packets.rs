//! Сверка демуксера с ffprobe: `cargo run -p lumen-media-demux --example dump_packets -- file [-v] [--seek us]`.
//! Печатает дорожки, длительность и число пакетов по дорожкам; с `-v` — каждый пакет.

use lumen_media_demux::Demuxer;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: dump_packets <file> [-v] [--seek us]");
        return Ok(());
    };
    let mut verbose = false;
    let mut seek = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "-v" => verbose = true,
            "--seek" => seek = args.next().and_then(|v| v.parse::<i64>().ok()),
            _ => {}
        }
    }
    let bytes = std::fs::read(&path)?;
    let mut d = match Demuxer::open(&bytes) {
        Ok(d) => d,
        Err(e) => {
            println!("ERR {e}");
            return Ok(());
        }
    };
    println!("container={:?} duration_us={:?}", d.container(), d.duration_us());
    for t in d.tracks() {
        println!(
            "track {} {:?} {:?} {} {}x{} {}Hz/{}ch extradata={}B",
            t.id,
            t.kind,
            t.codec,
            t.codec_string,
            t.width,
            t.height,
            t.sample_rate,
            t.channels,
            t.extradata.len()
        );
    }
    if let Some(us) = seek {
        d.seek_us(us)?;
    }
    let mut counts: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    while let Some(p) = d.next_packet()? {
        let c = counts.entry(p.track).or_default();
        c.0 += 1;
        c.1 += usize::from(p.keyframe);
        if verbose {
            println!(
                "t{} pts={} dts={} dur={} key={} len={}",
                p.track,
                p.pts_us,
                p.dts_us,
                p.duration_us,
                p.keyframe,
                p.data.len()
            );
        }
    }
    for (t, (n, k)) in counts {
        println!("track {t}: {n} packets, {k} keyframes");
    }
    Ok(())
}
