use mp4box::get_boxes;
use std::env;

/// Walk the box tree and print every box with indentation.
/// Returns counts of (total_boxes, decoded_boxes).
fn walk(boxes: &[mp4box::Box], depth: usize) -> (usize, usize) {
    let indent = "  ".repeat(depth);
    let mut total = 0;
    let mut decoded = 0;

    for b in boxes {
        total += 1;

        // Build the type annotation: show version/flags for FullBoxes
        let type_ann = match (b.version, b.flags) {
            (Some(v), Some(f)) => format!(" ver={} flags=0x{:06X}", v, f),
            _ => String::new(),
        };

        // Show size or "container" label
        let size_str = match b.kind.as_str() {
            "container" => "container".to_string(),
            _ => format!("{} bytes", b.size),
        };

        println!(
            "{}{:<4}  {:>12}  {}{}",
            indent, b.typ, size_str, b.full_name, type_ann
        );

        // Print decoded value if present
        if let Some(ref text) = b.decoded {
            println!("{}      -> {}", indent, text);
            decoded += 1;
        }

        // Recurse into children
        if let Some(ref children) = b.children {
            let (c_total, c_decoded) = walk(children, depth + 1);
            total += c_total;
            decoded += c_decoded;
        }
    }

    (total, decoded)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: {} <file.mp4> [--no-decode]", args[0]);
        std::process::exit(1);
    }

    let decode = !args.iter().any(|a| a == "--no-decode");
    let mut file = std::fs::File::open(&args[1])?;
    let file_size = file.metadata()?.len();

    println!("File: {} ({} bytes)", args[1], file_size);
    println!("{:-<60}", "");

    let boxes = get_boxes(&mut file, file_size, decode)?;
    let (total, decoded_count) = walk(&boxes, 0);

    println!("{:-<60}", "");
    println!(
        "Total: {} boxes  ({} decoded)",
        total, decoded_count
    );

    // ---- Walk examples for specific box types ----

    // Find the ftyp box and print compatible brands
    if let Some(ftyp) = boxes.iter().find(|b| b.typ == "ftyp") {
        if let Some(ref text) = ftyp.decoded {
            println!("\nFile type info: {}", text);
        }
    }

    // Collect all track headers to summarize tracks
    let mut tracks: Vec<(u32, u64, f32, f32)> = Vec::new();
    if let Some(moov) = boxes.iter().find(|b| b.typ == "moov") {
        if let Some(ref children) = moov.children {
            for trak in children.iter().filter(|b| b.typ == "trak") {
                let tkhd = trak
                    .children
                    .as_ref()
                    .and_then(|c| c.iter().find(|b| b.typ == "tkhd"));
                if let Some(tkhd) = tkhd {
                    if let Some(mp4box::registry::StructuredData::TrackHeader(ref d)) =
                        tkhd.structured_data
                    {
                        tracks.push((d.track_id, d.duration, d.width, d.height));
                    }
                }
            }
        }
    }

    if !tracks.is_empty() {
        println!("\nTracks ({}):", tracks.len());
        for (id, dur, w, h) in &tracks {
            if *w > 0.0 {
                println!("  Track #{}: duration={} {}x{}", id, dur, w, h);
            } else {
                println!("  Track #{}: duration={} (audio/other)", id, dur);
            }
        }
    }

    // Print sample-table summary for each track
    if let Some(moov) = boxes.iter().find(|b| b.typ == "moov") {
        if let Some(ref children) = moov.children {
            for trak in children.iter().filter(|b| b.typ == "trak") {
                let track_id = trak
                    .children
                    .as_ref()
                    .and_then(|c| c.iter().find(|b| b.typ == "tkhd"))
                    .and_then(|t| t.structured_data.as_ref())
                    .and_then(|sd| {
                        if let mp4box::registry::StructuredData::TrackHeader(d) = sd {
                            Some(d.track_id)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);

                // Navigate moov/trak/mdia/minf/stbl
                let stbl = trak
                    .children
                    .as_ref()
                    .and_then(|c| c.iter().find(|b| b.typ == "mdia"))
                    .and_then(|b| b.children.as_ref())
                    .and_then(|c| c.iter().find(|b| b.typ == "minf"))
                    .and_then(|b| b.children.as_ref())
                    .and_then(|c| c.iter().find(|b| b.typ == "stbl"));

                if let Some(stbl) = stbl {
                    if let Some(ref stbl_children) = stbl.children {
                        // Find codec from stsd
                        let codec = stbl_children
                            .iter()
                            .find(|b| b.typ == "stsd")
                            .and_then(|b| b.structured_data.as_ref())
                            .and_then(|sd| {
                                if let mp4box::registry::StructuredData::SampleDescription(d) = sd {
                                    d.entries.first().map(|e| e.codec.clone())
                                } else {
                                    None
                                }
                            });

                        // Find sample count from stsz
                        let sample_count = stbl_children
                            .iter()
                            .find(|b| b.typ == "stsz")
                            .and_then(|b| b.structured_data.as_ref())
                            .and_then(|sd| {
                                if let mp4box::registry::StructuredData::SampleSize(d) = sd {
                                    Some(d.sample_count)
                                } else {
                                    None
                                }
                            });

                        // Find keyframe count from stss
                        let keyframes = stbl_children
                            .iter()
                            .find(|b| b.typ == "stss")
                            .and_then(|b| b.structured_data.as_ref())
                            .and_then(|sd| {
                                if let mp4box::registry::StructuredData::SyncSample(d) = sd {
                                    Some(d.entry_count)
                                } else {
                                    None
                                }
                            });

                        println!(
                            "\n  Track #{} sample table: codec={} samples={} keyframes={}",
                            track_id,
                            codec.as_deref().unwrap_or("?"),
                            sample_count.map_or("?".into(), |n| n.to_string()),
                            keyframes.map_or("?".into(), |n| n.to_string()),
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
