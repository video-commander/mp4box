use clap::Parser;
use mp4box::{Box, get_boxes};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "Simple MP4 media info (like mp4info)")]
struct Args {
    /// MP4/ISOBMFF file path
    path: String,

    /// Output as JSON instead of human-readable text
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct TrackInfo {
    index: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    track_type: Option<String>, // "video" / "audio" / "other"

    #[serde(skip_serializing_if = "Option::is_none")]
    codec: Option<String>, // e.g. "avc1", "hvc1", "mp4a"

    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    timescale: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ticks: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    duration_seconds: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
}

#[derive(Debug, Serialize)]
struct MediaInfo {
    file: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    major_brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minor_version: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    compatible_brands: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    movie_timescale: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    movie_duration_ticks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    movie_duration_seconds: Option<f64>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    tracks: Vec<TrackInfo>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let path = PathBuf::from(&args.path);

    let mut file = std::fs::File::open(&path)?;
    let size = file.metadata()?.len();

    let boxes = get_boxes(&mut file, size, /*decode=*/ true)?;
    let mut info = MediaInfo {
        file: path.display().to_string(),
        major_brand: None,
        minor_version: None,
        compatible_brands: Vec::new(),
        movie_timescale: None,
        movie_duration_ticks: None,
        movie_duration_seconds: None,
        tracks: Vec::new(),
    };

    // Walk top-level boxes: ftyp, moov, etc.
    for b in &boxes {
        match b.typ.as_str() {
            "ftyp" => parse_ftyp(b, &mut info),
            "moov" => parse_moov(b, &mut info),
            _ => {}
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        print_human(&info);
    }

    Ok(())
}

fn parse_ftyp(b: &Box, info: &mut MediaInfo) {
    let decoded = match &b.decoded {
        Some(s) => s,
        None => return,
    };

    // Example decoded string:
    // "major=isom minor=512 compatible=[\"isom\", \"iso2\", \"avc1\", \"mp41\"]"
    if let Some(major) = parse_string_field(decoded, "major=") {
        info.major_brand = Some(major);
    }
    if let Some(minor) = parse_u32_field(decoded, "minor=") {
        info.minor_version = Some(minor);
    }
    if let Some(compat) = parse_compatible_brands(decoded) {
        info.compatible_brands = compat;
    }
}

fn parse_moov(b: &Box, info: &mut MediaInfo) {
    let children = match &b.children {
        Some(c) => c,
        None => return,
    };

    // mvhd for overall movie duration
    if let Some(mvhd) = children.iter().find(|c| c.typ == "mvhd")
        && let Some(decoded) = &mvhd.decoded
    {
        // Example: "timescale=600000 duration=65536"
        if let Some(ts) = parse_u32_field(decoded, "timescale=") {
            info.movie_timescale = Some(ts);
        }
        if let Some(dur) = parse_u64_field(decoded, "duration=") {
            info.movie_duration_ticks = Some(dur);
            if let Some(ts) = info.movie_timescale {
                info.movie_duration_seconds = Some(dur as f64 / ts as f64);
            }
        }
    }

    // trak boxes for per-track timing
    for (i, trak) in children.iter().filter(|c| c.typ == "trak").enumerate() {
        parse_trak(trak, i + 1, info);
    }
}

fn parse_trak(trak: &Box, index: usize, info: &mut MediaInfo) {
    let mut ti = TrackInfo {
        index,
        track_type: None,
        codec: None,
        width: None,
        height: None,
        timescale: None,
        duration_ticks: None,
        duration_seconds: None,
        language: None,
    };

    // tkhd at the trak level: possible width/height
    if let Some(tkhd) = find_child(trak, "tkhd")
        && let Some(decoded) = &tkhd.decoded
    {
        // For “normal” tkhd decoders you’ll get something like:
        // "track_id=1 duration=... width=1920 height=1080"
        if let Some(w) = parse_u32_field(decoded, "width=") {
            ti.width = Some(w);
        }
        if let Some(h) = parse_u32_field(decoded, "height=") {
            ti.height = Some(h);
        }
    }

    // mdia -> mdhd + hdlr + minf
    let mdia = match find_child(trak, "mdia") {
        Some(m) => m,
        None => {
            info.tracks.push(ti);
            return;
        }
    };

    // mdhd: timescale / duration / language
    if let Some(mdhd) = find_child(mdia, "mdhd") {
        // Try structured data first
        if let Some(mp4box::registry::StructuredData::MediaHeader(mdhd_data)) =
            &mdhd.structured_data
        {
            ti.timescale = Some(mdhd_data.timescale);
            ti.duration_ticks = Some(mdhd_data.duration);
            ti.duration_seconds = Some(mdhd_data.duration as f64 / mdhd_data.timescale as f64);
            ti.language = Some(mdhd_data.language.clone());
        }
        // Fallback to text parsing
        else if let Some(decoded) = &mdhd.decoded {
            if let Some(ts) = parse_u32_field(decoded, "timescale=") {
                ti.timescale = Some(ts);
            }
            if let Some(dur) = parse_u64_field(decoded, "duration=") {
                ti.duration_ticks = Some(dur);
                if let Some(ts) = ti.timescale {
                    ti.duration_seconds = Some(dur as f64 / ts as f64);
                }
            }
            if let Some(lang) = parse_string_field(decoded, "language=") {
                ti.language = Some(lang);
            }
        }
    }

    // hdlr: determine track type (video/audio/other)
    if let Some(hdlr) = find_child(mdia, "hdlr") {
        // Try structured data first
        if let Some(mp4box::registry::StructuredData::HandlerReference(hdlr_data)) =
            &hdlr.structured_data
        {
            let tt = match hdlr_data.handler_type.as_str() {
                "vide" => "video",
                "soun" => "audio",
                _ => "other",
            };
            ti.track_type = Some(tt.to_string());
        }
        // Fallback to text parsing
        else if let Some(decoded) = &hdlr.decoded {
            // Ideally your hdlr decoder now prints "handler=vide name=..."
            if let Some(handler) = parse_string_field(decoded, "handler=") {
                let tt = match handler.as_str() {
                    "vide" => "video",
                    "soun" => "audio",
                    _ => "other",
                };
                ti.track_type = Some(tt.to_string());
            }
        }
    }

    // minf -> stbl -> stsd: codec + width/height from decoded text
    if let Some(minf) = find_child(mdia, "minf")
        && let Some(stbl) = find_child(minf, "stbl")
        && let Some(stsd) = find_child(stbl, "stsd")
        && let Some(decoded) = &stsd.decoded
    {
        // codec
        if let Some(c) = parse_string_field(decoded, "codec=") {
            ti.codec = Some(c.clone());

            // If no type from hdlr, infer from codec
            if ti.track_type.is_none() {
                let tt = match c.as_str() {
                    "avc1" | "hvc1" | "hev1" | "vp09" | "av01" => "video",
                    "mp4a" | "ac-3" | "ec-3" | "Opus" => "audio",
                    _ => "other",
                };
                ti.track_type = Some(tt.to_string());
            }
        }

        // width / height (for video)
        if let Some(w) = parse_u32_field(decoded, "width=") {
            ti.width = Some(w);
        }
        if let Some(h) = parse_u32_field(decoded, "height=") {
            ti.height = Some(h);
        }
    }

    info.tracks.push(ti);
}

fn find_child<'a>(parent: &'a Box, typ: &str) -> Option<&'a Box> {
    parent
        .children
        .as_ref()
        .and_then(|kids| kids.iter().find(|c| c.typ == typ))
}

// ---- tiny string parsers over the `decoded` text --------------------

fn parse_u32_field(s: &str, key: &str) -> Option<u32> {
    parse_u64_field(s, key).and_then(|v| u32::try_from(v).ok())
}

fn parse_u64_field(s: &str, key: &str) -> Option<u64> {
    let idx = s.find(key)?;
    let rest = &s[idx + key.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn parse_string_field(s: &str, key: &str) -> Option<String> {
    let idx = s.find(key)?;
    let mut rest = &s[idx + key.len()..];

    // Trim leading whitespace
    rest = rest.trim_start();

    // Take until next space or end
    let token: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();

    if token.is_empty() {
        None
    } else {
        Some(token.trim_matches('"').to_string())
    }
}

fn parse_compatible_brands(s: &str) -> Option<Vec<String>> {
    // e.g. compatible=["isom", "iso2", "avc1", "mp41"]
    let idx = s.find("compatible=[")?;
    let rest = &s[idx + "compatible=[".len()..];
    let end = rest.find(']')?;
    let inside = &rest[..end];
    if inside.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in inside.split(',') {
        let trimmed = part.trim().trim_matches('"');
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    Some(out)
}

// ---- human-readable output -----------------------------------------

fn print_human(info: &MediaInfo) {
    println!("File: {}", info.file);
    if let Some(major) = &info.major_brand {
        println!("Major brand: {}", major);
    }
    if let Some(minor) = info.minor_version {
        println!("Minor version: {}", minor);
    }
    if !info.compatible_brands.is_empty() {
        println!("Compatible brands: {}", info.compatible_brands.join(", "));
    }

    if let (Some(ts), Some(dur)) = (info.movie_timescale, info.movie_duration_ticks) {
        let sec = dur as f64 / ts as f64;
        println!("Movie duration: {} ticks @ {} -> {:.3} s", dur, ts, sec);
    }

    if info.tracks.is_empty() {
        println!("Tracks: (none)");
        return;
    }

    println!("Tracks:");
    for t in &info.tracks {
        println!("  Track {}:", t.index);

        if let Some(tt) = &t.track_type {
            println!("    type: {}", tt);
        }
        if let Some(codec) = &t.codec {
            println!("    codec: {}", codec);
        }
        if let (Some(w), Some(h)) = (t.width, t.height) {
            println!("    size: {}x{}", w, h);
        }

        if let Some(ts) = t.timescale {
            println!("    timescale: {}", ts);
        }
        if let Some(dur) = t.duration_ticks {
            if let Some(sec) = t.duration_seconds {
                println!("    duration: {} ticks -> {:.3} s", dur, sec);
            } else {
                println!("    duration: {} ticks", dur);
            }
        }
        if let Some(lang) = &t.language {
            println!("    language: {}", lang);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_u64_field_extracts_number() {
        let s = "timescale=600000 duration=65536";
        assert_eq!(parse_u64_field(s, "timescale="), Some(600000));
        assert_eq!(parse_u64_field(s, "duration="), Some(65536));
        assert_eq!(parse_u64_field(s, "missing="), None);
    }

    #[test]
    fn parse_u32_field_clamps_to_u32() {
        let s = "timescale=12345";
        assert_eq!(parse_u32_field(s, "timescale="), Some(12345));
    }

    #[test]
    fn parse_string_field_basic() {
        let s = "major=isom minor=512";
        assert_eq!(parse_string_field(s, "major="), Some("isom".to_string()));
        assert_eq!(parse_string_field(s, "minor="), Some("512".to_string()));
        assert_eq!(parse_string_field(s, "missing="), None);
    }

    #[test]
    fn parse_string_field_trims_quotes() {
        let s = r#"language="und""#;
        assert_eq!(parse_string_field(s, "language="), Some("und".to_string()));
    }

    #[test]
    fn parse_compatible_brands_parses_list() {
        let s = r#"major=isom minor=512 compatible=["isom", "iso2", "avc1", "mp41"]"#;
        let brands = parse_compatible_brands(s).unwrap();
        assert_eq!(brands, vec!["isom", "iso2", "avc1", "mp41"]);
    }

    #[test]
    fn parse_compatible_brands_empty() {
        let s = r#"compatible=[]"#;
        let brands = parse_compatible_brands(s).unwrap();
        assert!(brands.is_empty());
    }
}
