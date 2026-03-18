use clap::Parser;
use mp4box::edit::{Command, EditingProcessor};
use std::fs::File;

/// mp4edit — non-destructive MP4/ISOBMFF box editor
///
/// All commands apply in the order given; the output is a single, patched
/// copy of the input with chunk offsets fixed up automatically.
#[derive(Parser, Debug)]
#[command(version, about = "Non-destructive MP4/ISOBMFF box editor")]
struct Args {
    /// Remove a box: --remove moov/udta
    #[arg(long = "remove", value_name = "PATH")]
    remove: Vec<String>,

    /// Insert a raw box file as a child.  Format: PARENT_PATH:FILE[:POSITION]
    /// Example: --insert moov/udta:new.box:0
    #[arg(long = "insert", value_name = "PATH:FILE[:POS]")]
    insert: Vec<String>,

    /// Replace a box wholesale with a raw box file.  Format: BOX_PATH:FILE
    /// Example: --replace moov/udta/©nam:new_title.box
    #[arg(long = "replace", value_name = "PATH:FILE")]
    replace: Vec<String>,

    /// Set a named field inside a known box (uses encoder registry).
    /// Format: BOX_PATH.FIELD=VALUE
    /// Example: --set moov/mvhd.creation_time=0
    #[arg(long = "set", value_name = "PATH.FIELD=VALUE")]
    set: Vec<String>,

    /// Input MP4/ISOBMFF file
    input: String,

    /// Output file path
    output: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut processor = EditingProcessor::new();

    // ---- Parse --remove args ----
    for path in &args.remove {
        processor.add_command(Command::Remove {
            box_path: path.clone(),
        });
    }

    // ---- Parse --insert args  (PARENT_PATH:FILE or PARENT_PATH:FILE:POS) ----
    for spec in &args.insert {
        let parts: Vec<&str> = spec.splitn(3, ':').collect();
        if parts.len() < 2 {
            anyhow::bail!(
                "--insert format is PATH:FILE or PATH:FILE:POSITION, got '{}'",
                spec
            );
        }
        let box_path = parts[0].to_string();
        let file_path = parts[1].to_string();
        let position = if parts.len() == 3 {
            Some(parts[2].parse::<usize>().map_err(|_| {
                anyhow::anyhow!("--insert position must be an integer, got '{}'", parts[2])
            })?)
        } else {
            None
        };
        processor.add_command(Command::Insert {
            box_path,
            file_path,
            position,
        });
    }

    // ---- Parse --replace args  (BOX_PATH:FILE) ----
    for spec in &args.replace {
        let (box_path, file_path) = spec
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("--replace format is PATH:FILE, got '{}'", spec))?;
        processor.add_command(Command::Replace {
            box_path: box_path.to_string(),
            file_path: file_path.to_string(),
        });
    }

    // ---- Parse --set args  (BOX_PATH.FIELD=VALUE) ----
    for spec in &args.set {
        // Split on the last '.' before the '=' to separate box_path from field=value.
        // E.g. "moov/mvhd.creation_time=0" → path="moov/mvhd", rest="creation_time=0"
        // E.g. "moov/udta/©nam.value=My Movie" → path="moov/udta/©nam", rest="value=My Movie"
        let dot_pos = spec
            .rfind('.')
            .ok_or_else(|| anyhow::anyhow!("--set format is PATH.FIELD=VALUE, got '{}'", spec))?;
        let box_path = &spec[..dot_pos];
        let rest = &spec[dot_pos + 1..];
        let (field, value) = rest
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set format is PATH.FIELD=VALUE, got '{}'", spec))?;
        processor.add_command(Command::Set {
            box_path: box_path.to_string(),
            field: field.to_string(),
            value: value.to_string(),
        });
    }

    // ---- Run ----
    let mut src = File::open(&args.input)
        .map_err(|e| anyhow::anyhow!("cannot open input '{}': {}", args.input, e))?;
    let mut dst = File::create(&args.output)
        .map_err(|e| anyhow::anyhow!("cannot create output '{}': {}", args.output, e))?;

    let stats = processor.process(&mut src, &mut dst)?;

    if stats.offset_delta != 0 {
        eprintln!(
            "mp4edit: chunk offsets adjusted by {} bytes",
            stats.offset_delta
        );
    }

    eprintln!("mp4edit: wrote '{}'", args.output);
    Ok(())
}
