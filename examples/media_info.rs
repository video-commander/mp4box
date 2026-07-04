use mp4box::registry::StructuredData;
use mp4box::{get_boxes, get_itunes_tags};
use std::env;
use std::fs::File;

// This example demonstrates reading media information through typed
// structured data instead of parsing decoded text: the movie header (mvhd),
// per-track headers (tkhd/mdhd), codec details from stsd sample entries,
// edit lists (elst), and iTunes metadata tags.
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <file.mp4>", args[0]);
        std::process::exit(1);
    }

    let mut file = File::open(&args[1])?;
    let size = file.metadata()?.len();
    let boxes = get_boxes(&mut file, size, /*decode=*/ true)?;

    let Some(moov) = boxes.iter().find(|b| b.typ == "moov") else {
        anyhow::bail!("no moov box found");
    };
    let moov_children = moov.children.as_deref().unwrap_or_default();

    // Movie header: typed access, no text parsing needed
    if let Some(StructuredData::MovieHeader(mvhd)) = moov_children
        .iter()
        .find(|b| b.typ == "mvhd")
        .and_then(|b| b.structured_data.as_ref())
    {
        let seconds = mvhd.duration as f64 / mvhd.timescale as f64;
        println!("Movie:");
        println!("  timescale: {} ticks/s", mvhd.timescale);
        println!("  duration:  {} ticks ({:.3} s)", mvhd.duration, seconds);
        println!("  rate: {}  volume: {}", mvhd.rate, mvhd.volume);
        println!("  next_track_id: {}", mvhd.next_track_id);
    }

    // Per-track info
    for trak in moov_children.iter().filter(|b| b.typ == "trak") {
        let kids = trak.children.as_deref().unwrap_or_default();

        if let Some(StructuredData::TrackHeader(tkhd)) = kids
            .iter()
            .find(|b| b.typ == "tkhd")
            .and_then(|b| b.structured_data.as_ref())
        {
            println!("\nTrack {}:", tkhd.track_id);
            if tkhd.width > 0.0 {
                println!("  display size: {}x{}", tkhd.width, tkhd.height);
            }
        }

        let mdia = kids.iter().find(|b| b.typ == "mdia");
        let mdia_kids = mdia.and_then(|m| m.children.as_deref()).unwrap_or_default();

        if let Some(StructuredData::MediaHeader(mdhd)) = mdia_kids
            .iter()
            .find(|b| b.typ == "mdhd")
            .and_then(|b| b.structured_data.as_ref())
        {
            println!(
                "  media: timescale={} duration={} language={}",
                mdhd.timescale, mdhd.duration, mdhd.language
            );
        }

        // Codec details from the sample description
        if let Some(stbl) = mdia_kids
            .iter()
            .find(|b| b.typ == "minf")
            .and_then(|m| m.children.as_deref())
            .and_then(|kids| kids.iter().find(|b| b.typ == "stbl"))
            && let Some(StructuredData::SampleDescription(stsd)) = stbl
                .children
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|b| b.typ == "stsd")
                .and_then(|b| b.structured_data.as_ref())
        {
            for entry in &stsd.entries {
                print!("  codec: {}", entry.codec);
                if let (Some(w), Some(h)) = (entry.width, entry.height) {
                    print!("  {}x{}", w, h);
                }
                if let Some(ch) = entry.channel_count {
                    print!("  {} ch", ch);
                }
                if let Some(sr) = entry.sample_rate {
                    print!("  {} Hz", sr);
                }
                if let Some(bits) = entry.sample_size {
                    print!("  {}-bit", bits);
                }
                println!();
            }
        }

        // Edit list: initial delays / empty edits
        if let Some(StructuredData::EditList(elst)) = kids
            .iter()
            .find(|b| b.typ == "edts")
            .and_then(|e| e.children.as_deref())
            .and_then(|kids| kids.iter().find(|b| b.typ == "elst"))
            .and_then(|b| b.structured_data.as_ref())
        {
            for (i, e) in elst.entries.iter().enumerate() {
                println!(
                    "  edit[{}]: duration={} media_time={} rate={}.{}",
                    i,
                    e.segment_duration,
                    e.media_time,
                    e.media_rate_integer,
                    e.media_rate_fraction
                );
            }
        }
    }

    // iTunes metadata tags (empty map if the file has none)
    let mut file = File::open(&args[1])?;
    let tags = get_itunes_tags(&mut file, size)?;
    if !tags.is_empty() {
        println!("\nTags:");
        let mut sorted: Vec<_> = tags.iter().collect();
        sorted.sort();
        for (key, value) in sorted {
            println!("  {}: {}", key, value);
        }
    }

    Ok(())
}
