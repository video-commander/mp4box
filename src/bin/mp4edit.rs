use clap::Parser;
use mp4box::edit::{Command, Editor};

/// mp4edit — non-destructive MP4/ISOBMFF box editor.
///
/// Commands apply in order to a single parsed tree; the output is written as
/// a new file with all box sizes and chunk offsets fixed up automatically.
/// Box paths are slash-delimited fourccs with optional indices for repeated
/// boxes, e.g. `moov/trak[1]/mdia/mdhd`.
#[derive(Parser, Debug)]
#[command(version, about = "Non-destructive MP4/ISOBMFF box editor")]
struct Args {
    /// Remove a box: --remove moov/udta
    #[arg(long = "remove", value_name = "PATH")]
    remove: Vec<String>,

    /// Remove every box with this fourcc, anywhere: --remove-all free
    #[arg(long = "remove-all", value_name = "FOURCC")]
    remove_all: Vec<String>,

    /// Insert a raw box file (header + payload) as a child.
    /// Format: PARENT_PATH:FILE[:POSITION]
    #[arg(long = "insert", value_name = "PATH:FILE[:POS]")]
    insert: Vec<String>,

    /// Replace a box wholesale with a raw box file. Format: PATH:FILE
    #[arg(long = "replace", value_name = "PATH:FILE")]
    replace: Vec<String>,

    /// Set a field of a known box (mvhd/tkhd/mdhd) in place.
    /// Format: PATH.FIELD=VALUE, e.g. --set moov/mvhd.creation_time=0
    #[arg(long = "set", value_name = "PATH.FIELD=VALUE")]
    set: Vec<String>,

    /// Set an iTunes tag (creates moov/udta/meta/ilst as needed).
    /// Format: NAME=VALUE with a friendly name (title, artist, album, ...)
    /// or a raw fourcc, e.g. --tag title="My Movie"
    #[arg(long = "tag", value_name = "NAME=VALUE")]
    tag: Vec<String>,

    /// Input MP4/ISOBMFF file (never modified)
    input: String,

    /// Output file path
    output: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut editor = Editor::new();

    for path in &args.remove {
        editor.add_command(Command::Remove { path: path.clone() });
    }

    for fourcc in &args.remove_all {
        editor.add_command(Command::RemoveAll {
            fourcc: fourcc.clone(),
        });
    }

    for spec in &args.insert {
        let parts: Vec<&str> = spec.splitn(3, ':').collect();
        anyhow::ensure!(
            parts.len() >= 2,
            "--insert format is PATH:FILE or PATH:FILE:POSITION, got '{}'",
            spec
        );
        let position =
            match parts.get(2) {
                Some(p) => Some(p.parse::<usize>().map_err(|_| {
                    anyhow::anyhow!("--insert position must be a number, got '{}'", p)
                })?),
                None => None,
            };
        editor.add_command(Command::Insert {
            parent: parts[0].to_string(),
            bytes: std::fs::read(parts[1])?,
            position,
        });
    }

    for spec in &args.replace {
        let (path, file) = spec
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("--replace format is PATH:FILE, got '{}'", spec))?;
        editor.add_command(Command::Replace {
            path: path.to_string(),
            bytes: std::fs::read(file)?,
        });
    }

    for spec in &args.set {
        // PATH.FIELD=VALUE — the field name never contains '.' or '='.
        let (lhs, value) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set format is PATH.FIELD=VALUE, got '{}'", spec))?;
        let (path, field) = lhs
            .rsplit_once('.')
            .ok_or_else(|| anyhow::anyhow!("--set format is PATH.FIELD=VALUE, got '{}'", spec))?;
        editor.add_command(Command::Set {
            path: path.to_string(),
            field: field.to_string(),
            value: value.to_string(),
        });
    }

    for spec in &args.tag {
        let (name, value) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--tag format is NAME=VALUE, got '{}'", spec))?;
        editor.set_tag(name, value)?;
    }

    let stats = editor.process_file(&args.input, &args.output)?;
    println!(
        "wrote {} ({} bytes, {} chunk offsets adjusted{})",
        args.output,
        stats.bytes_written,
        stats.chunk_offsets_adjusted,
        if stats.chunk_offsets_unmapped > 0 {
            format!(", {} unmapped!", stats.chunk_offsets_unmapped)
        } else {
            String::new()
        }
    );
    Ok(())
}
