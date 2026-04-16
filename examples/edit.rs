/// Demonstrates the `mp4box` edit API by programmatically removing and
/// modifying boxes in an MP4 file — no CLI arguments required.
///
/// Run with:
///   cargo run --example edit --features edit -- input.mp4 output.mp4
///
/// What this example does:
///   1. Strips timestamps (sets mvhd creation_time / modification_time to 0).
///   2. Removes the `moov/udta` metadata container if present.
///   3. Reports how many bytes the chunk offsets were adjusted.
use mp4box::edit::{Command, EditingProcessor};
use std::{env, fs::File};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <input.mp4> <output.mp4>", args[0]);
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let mut processor = EditingProcessor::new();

    // Zero out creation / modification timestamps stored in the movie header.
    // These are the fields most commonly used to fingerprint recordings.
    processor
        .add_command(Command::Set {
            box_path: "moov/mvhd".to_string(),
            field: "creation_time".to_string(),
            value: "0".to_string(),
        })
        .add_command(Command::Set {
            box_path: "moov/mvhd".to_string(),
            field: "modification_time".to_string(),
            value: "0".to_string(),
        });

    // Remove the user-data box.  Many encoders write vendor strings,
    // chapter lists, or other metadata here that you may want to strip.
    // `EditingProcessor` silently succeeds even if the path is absent —
    // callers should check presence first if that matters.
    processor.add_command(Command::Remove {
        box_path: "moov/udta".to_string(),
    });

    // Open source and destination files, then run the edit pass.
    let mut src = File::open(input_path)
        .map_err(|e| anyhow::anyhow!("cannot open '{}': {}", input_path, e))?;
    let mut dst = File::create(output_path)
        .map_err(|e| anyhow::anyhow!("cannot create '{}': {}", output_path, e))?;

    let stats = processor.process(&mut src, &mut dst)?;

    println!("Wrote '{}'", output_path);
    if stats.offset_delta != 0 {
        println!("Chunk offsets adjusted by {} bytes", stats.offset_delta);
    }

    Ok(())
}
