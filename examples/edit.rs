use mp4box::edit::Editor;
use std::env;

// This example demonstrates the edit API: it copies an MP4 while setting
// iTunes tags (creating moov/udta/meta/ilst if needed) and zeroing the
// movie timestamps. Box sizes and stco/co64 chunk offsets are fixed up
// automatically; the input file is never modified.
//
// Requires the "edit" feature (enabled by default):
//   cargo run --example edit -- in.mp4 out.mp4
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <input.mp4> <output.mp4>", args[0]);
        std::process::exit(1);
    }

    let mut editor = Editor::new();
    editor.set_tag("title", "Edited with mp4box")?;
    editor.set_tag("encoder", "mp4box edit example")?;
    editor.set_field("moov/mvhd", "creation_time", "0");
    editor.set_field("moov/mvhd", "modification_time", "0");

    let stats = editor.process_file(&args[1], &args[2])?;

    println!("wrote {} bytes to {}", stats.bytes_written, args[2]);
    println!("chunk offsets adjusted: {}", stats.chunk_offsets_adjusted);
    if stats.chunk_offsets_unmapped > 0 {
        eprintln!(
            "warning: {} chunk offsets point at removed data",
            stats.chunk_offsets_unmapped
        );
    }
    Ok(())
}
