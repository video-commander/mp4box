use mp4box::registry::StructuredData;
use mp4box::{get_boxes, track_samples_from_path};
use std::env;
use std::fs::File;

// This example demonstrates fragmented MP4 (fMP4/DASH/CMAF) support:
// it lists each movie fragment (moof) with its track fragment headers,
// decode times, and run summaries, then prints the per-track samples that
// track_samples_from_path() assembles from those fragments using the
// tfhd/trex defaulting rules.
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <file.mp4>", args[0]);
        std::process::exit(1);
    }

    let mut file = File::open(&args[1])?;
    let size = file.metadata()?.len();
    let boxes = get_boxes(&mut file, size, /*decode=*/ true)?;

    // trex defaults declared in moov/mvex (one per track)
    for moov in boxes.iter().filter(|b| b.typ == "moov") {
        for mvex in child_boxes(moov).iter().filter(|b| b.typ == "mvex") {
            for trex in child_boxes(mvex) {
                if let Some(StructuredData::TrackExtends(d)) = &trex.structured_data {
                    println!(
                        "trex: track {} default_duration={} default_size={} default_flags=0x{:08X}",
                        d.track_id,
                        d.default_sample_duration,
                        d.default_sample_size,
                        d.default_sample_flags
                    );
                }
            }
        }
    }

    // Walk the fragments
    let moofs: Vec<_> = boxes.iter().filter(|b| b.typ == "moof").collect();
    if moofs.is_empty() {
        println!("\nNo moof boxes: this is not a fragmented MP4.");
    }
    for (i, moof) in moofs.iter().enumerate() {
        println!("\nfragment {} @ {:#x}:", i, moof.offset);
        for traf in child_boxes(moof).iter().filter(|b| b.typ == "traf") {
            let kids = child_boxes(traf);

            let track_id = kids.iter().find_map(|b| match &b.structured_data {
                Some(StructuredData::TrackFragmentHeader(d)) => Some(d.track_id),
                _ => None,
            });
            let decode_time = kids.iter().find_map(|b| match &b.structured_data {
                Some(StructuredData::TrackFragmentDecodeTime(d)) => Some(d.base_media_decode_time),
                _ => None,
            });
            let sample_count: u32 = kids
                .iter()
                .filter_map(|b| match &b.structured_data {
                    Some(StructuredData::TrackFragmentRun(d)) => Some(d.sample_count),
                    _ => None,
                })
                .sum();

            println!(
                "  track {:?}: base_decode_time={:?} samples={}",
                track_id, decode_time, sample_count
            );
        }
    }

    // Samples assembled across all fragments (falls back to stbl for
    // progressive files, so this works on both kinds)
    println!();
    for track in track_samples_from_path(&args[1])? {
        let keyframes = track.samples.iter().filter(|s| s.is_sync).count();
        let seconds = track.duration as f64 / track.timescale as f64;
        println!(
            "track {} ({}): {} samples, {} keyframes, {:.3} s",
            track.track_id, track.handler_type, track.sample_count, keyframes, seconds
        );
        for s in track.samples.iter().take(3) {
            println!(
                "  sample {}: dts={} pts={} dur={} size={} offset={:#x}{}",
                s.index,
                s.dts,
                s.pts,
                s.duration,
                s.size,
                s.file_offset,
                if s.is_sync { " [sync]" } else { "" }
            );
        }
    }

    Ok(())
}

fn child_boxes(b: &mp4box::Box) -> &[mp4box::Box] {
    b.children.as_deref().unwrap_or_default()
}
