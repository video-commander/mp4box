use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mp4box::registry::StructuredData;
use mp4box::{TrackSamples, get_boxes, track_samples_from_reader};

#[derive(Debug, Parser)]
#[command(
    name = "mp4samples",
    about = "Print MP4 track sample information with structured data parsing"
)]
struct Args {
    /// Input MP4 file
    input: PathBuf,

    /// Filter by track-id (default: all tracks)
    #[arg(long)]
    track_id: Option<u32>,

    /// Print JSON instead of text
    #[arg(long)]
    json: bool,

    /// Limit number of samples printed per track
    #[arg(long)]
    limit: Option<usize>,

    /// Show raw sample table data instead of calculated samples
    #[arg(long)]
    tables: bool,

    /// Show detailed timing information (DTS/PTS)
    #[arg(long)]
    timing: bool,

    /// Verbose output with sample table statistics
    #[arg(short, long)]
    verbose: bool,
}

/// Per-track sample-table statistics, read from the decoded box tree.
#[derive(Debug, Clone, Default)]
struct TableStats {
    stts_entries: u32,
    stsc_entries: u32,
    stco_entries: u32,
    keyframe_count: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let file = std::fs::File::open(&args.input)?;

    if args.tables {
        let mut file = file;
        let size = file.metadata()?.len();
        let boxes = get_boxes(&mut file, size, true)?;
        print_sample_tables(&boxes, &args)?;
        return Ok(());
    }

    // Real sample tables computed by the library (stts/ctts/stsc/stco/stsz).
    let tracks = track_samples_from_reader(file)?;

    // Table statistics for --verbose, from the same decoded box tree.
    let stats = if args.verbose {
        let mut file = std::fs::File::open(&args.input)?;
        let size = file.metadata()?.len();
        collect_table_stats(&get_boxes(&mut file, size, true)?)
    } else {
        Vec::new()
    };

    if args.json {
        print_json(&tracks, &stats, &args)?;
    } else {
        print_text(&tracks, &stats, &args)?;
    }

    Ok(())
}

/// Walk moov/trak/mdia/minf/stbl and collect table entry counts per track,
/// in document order (matching `track_samples_from_reader`'s track order).
fn collect_table_stats(boxes: &[mp4box::Box]) -> Vec<TableStats> {
    let mut stats = Vec::new();

    for moov in boxes.iter().filter(|b| b.typ == "moov") {
        let Some(moov_children) = &moov.children else {
            continue;
        };
        for trak in moov_children.iter().filter(|b| b.typ == "trak") {
            let mut s = TableStats::default();
            if let Some(stbl) = find_path(trak, &["mdia", "minf", "stbl"])
                && let Some(children) = &stbl.children
            {
                for child in children {
                    match &child.structured_data {
                        Some(StructuredData::DecodingTimeToSample(d)) => {
                            s.stts_entries = d.entry_count;
                        }
                        Some(StructuredData::SampleToChunk(d)) => {
                            s.stsc_entries = d.entry_count;
                        }
                        Some(StructuredData::ChunkOffset(d)) => {
                            s.stco_entries = d.entry_count;
                        }
                        Some(StructuredData::ChunkOffset64(d)) => {
                            s.stco_entries = d.entry_count;
                        }
                        Some(StructuredData::SyncSample(d)) => {
                            s.keyframe_count = d.entry_count;
                        }
                        _ => {}
                    }
                }
            }
            stats.push(s);
        }
    }

    stats
}

fn find_path<'a>(root: &'a mp4box::Box, path: &[&str]) -> Option<&'a mp4box::Box> {
    let mut cur = root;
    for name in path {
        cur = cur.children.as_ref()?.iter().find(|c| c.typ == *name)?;
    }
    Some(cur)
}

fn print_sample_tables(boxes: &[mp4box::Box], args: &Args) -> Result<()> {
    println!("Sample Table Analysis for: {:?}", args.input);
    println!("=========================================");

    analyze_boxes(boxes, 0, args);
    Ok(())
}

fn analyze_boxes(boxes: &[mp4box::Box], depth: usize, args: &Args) {
    let indent = "  ".repeat(depth);

    for box_info in boxes {
        if let Some(decoded) = &box_info.decoded {
            match box_info.typ.as_str() {
                "stts" => {
                    println!("{}📊 Decoding Time-to-Sample Box (stts):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "stsc" => {
                    println!("{}🗂️  Sample-to-Chunk Box (stsc):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "stsz" | "stz2" => {
                    println!("{}📏 Sample Size Box ({}):", indent, box_info.typ);
                    println!("{}   {}", indent, decoded);
                }
                "stco" => {
                    println!("{}📍 Chunk Offset Box (stco):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "co64" => {
                    println!("{}📍 64-bit Chunk Offset Box (co64):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "stss" => {
                    println!("{}🎯 Sync Sample Box (stss):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "ctts" => {
                    println!("{}⏰ Composition Time-to-Sample Box (ctts):", indent);
                    println!("{}   {}", indent, decoded);
                }
                "stsd" => {
                    println!("{}🎬 Sample Description Box (stsd):", indent);
                    println!("{}   {}", indent, decoded);
                }
                _ => {
                    if args.verbose && !decoded.is_empty() {
                        println!("{}📦 {} Box:", indent, box_info.typ);
                        println!("{}   {}", indent, decoded);
                    }
                }
            }
        }

        // Recurse into children
        if let Some(children) = &box_info.children {
            analyze_boxes(children, depth + 1, args);
        }
    }
}

fn print_json(tracks: &[TrackSamples], stats: &[TableStats], args: &Args) -> Result<()> {
    use serde_json::json;

    let value = json!({
        "tracks": tracks.iter().enumerate()
            .filter(|(_, t)| args.track_id.is_none_or(|tid| t.track_id == tid))
            .map(|(i, t)| {
                let mut samples = t.samples.clone();
                if let Some(lim) = args.limit {
                    samples.truncate(lim);
                }
                let mut track_data = json!({
                    "track_id": t.track_id,
                    "handler_type": t.handler_type,
                    "timescale": t.timescale,
                    "duration": t.duration,
                    "sample_count": t.sample_count,
                    "samples": samples,
                });

                if args.verbose && let Some(s) = stats.get(i) {
                    track_data["sample_tables"] = json!({
                        "stts_entries": s.stts_entries,
                        "stsz_entries": t.sample_count,
                        "stsc_entries": s.stsc_entries,
                        "stco_entries": s.stco_entries,
                        "keyframes": s.keyframe_count,
                    });
                }

                track_data
            }).collect::<Vec<_>>()
    });

    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn print_text(tracks: &[TrackSamples], stats: &[TableStats], args: &Args) -> Result<()> {
    for (i, t) in tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| args.track_id.is_none_or(|tid| t.track_id == tid))
    {
        println!(
            "Track {} ({}) timescale={} duration={} sample_count={}",
            t.track_id, t.handler_type, t.timescale, t.duration, t.sample_count
        );

        if args.verbose
            && let Some(s) = stats.get(i)
        {
            println!("  Sample Table Info:");
            println!("    STTS entries: {}", s.stts_entries);
            println!("    STSC entries: {}", s.stsc_entries);
            println!("    STCO entries: {}", s.stco_entries);
            println!("    Keyframes: {}", s.keyframe_count);
            println!();
        }

        if args.timing {
            println!("idx    DTS(ts)    PTS(ts)    start(s)   dur(ts)  size   offset      sync");
            println!("-------------------------------------------------------------------------");
        } else {
            println!("idx    start(s)   dur(ts)  size   offset      sync");
            println!("----------------------------------------------------");
        }

        for (count, s) in t.samples.iter().enumerate() {
            if let Some(lim) = args.limit
                && count >= lim
            {
                break;
            }

            if args.timing {
                println!(
                    "{:5} {:10} {:10} {:10.4} {:8} {:6} {:10} {}",
                    s.index,
                    s.dts,
                    s.pts,
                    s.start_time,
                    s.duration,
                    s.size,
                    s.file_offset,
                    if s.is_sync { "*" } else { "" },
                );
            } else {
                println!(
                    "{:5} {:10.4} {:8} {:6} {:10} {}",
                    s.index,
                    s.start_time,
                    s.duration,
                    s.size,
                    s.file_offset,
                    if s.is_sync { "*" } else { "" },
                );
            }
        }
        println!();
    }
    Ok(())
}
