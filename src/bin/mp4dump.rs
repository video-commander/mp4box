use clap::{ArgAction, Parser};
use mp4box::{
    BoxHeader,
    boxes::{BoxKey, BoxRef, FourCC, NodeKind},
    known_boxes::KnownBox,
    parser::{parse_children, read_box_header},
    registry::{BoxValue, Registry, StructuredData, default_registry},
    util::{hex_dump, read_slice},
};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::{fs::File, str::FromStr};

#[derive(Parser, Debug)]
#[command(version, about = "Minimal MP4/ISOBMFF box explorer")]
struct Args {
    /// MP4/ISOBMFF file path
    path: String,

    /// Only print subtree(s) matching a dotted path (e.g. moov.trak[0].mdia.minf.stbl)
    #[arg(long = "filter")]
    filter: Option<String>,

    /// Dump raw payload of this 4CC (e.g. --raw stsd) or uuid:xxxxxxxx...
    #[arg(long = "raw")]
    raw: Option<String>,

    /// Limit recursion depth (for text/tree output)
    #[arg(long, default_value_t = 64)]
    max_depth: usize,

    /// Print structured values when a decoder exists
    #[arg(long, action = ArgAction::SetTrue)]
    decode: bool,

    /// Show bytes count when dumping raw (0 means entire box payload)
    #[arg(long, default_value_t = 0)]
    bytes: usize,

    /// Emit JSON instead of human-readable tree
    #[arg(long, action = ArgAction::SetTrue)]
    json: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut f = File::open(&args.path)?;

    let file_len = f.metadata()?.len();
    let top = {
        // Top-level loop
        let mut kids = Vec::new();
        while f.stream_position()? < file_len {
            let h = read_box_header(&mut f)?;
            let box_end = if h.size == 0 {
                file_len
            } else {
                h.start + h.size
            };

            let kind = if is_container(&h) {
                f.seek(SeekFrom::Start(h.start + h.header_size))?;
                NodeKind::Container(parse_children(&mut f, box_end)?)
            } else if is_full_box(&h) {
                f.seek(SeekFrom::Start(h.start + h.header_size))?;
                use byteorder::ReadBytesExt;
                let version = f.read_u8()?;
                let mut fl = [0u8; 3];
                f.read_exact(&mut fl)?;
                let flags = ((fl[0] as u32) << 16) | ((fl[1] as u32) << 8) | (fl[2] as u32);
                let data_offset = f.stream_position()?;
                let data_len = box_end.saturating_sub(data_offset);
                NodeKind::FullBox {
                    version,
                    flags,
                    data_offset,
                    data_len,
                }
            } else {
                let data_offset = h.start + h.header_size;
                let data_len = box_end.saturating_sub(data_offset);
                if &h.typ.0 == b"uuid" {
                    NodeKind::Unknown {
                        data_offset,
                        data_len,
                    }
                } else {
                    NodeKind::Leaf {
                        data_offset,
                        data_len,
                    }
                }
            };
            f.seek(SeekFrom::Start(box_end))?;
            kids.push(BoxRef { hdr: h, kind });
        }
        kids
    };

    let reg = default_registry();

    // Target roots for printing/JSON
    let targets: Vec<&BoxRef> = if let Some(path) = &args.filter {
        select_by_path(&top, path)
    } else {
        top.iter().collect()
    };

    // JSON mode: output JSON and exit (no tree or raw to keep output clean)
    if args.json {
        let mut json_file = File::open(&args.path)?; // fresh handle for decoding
        let json_boxes: Vec<JsonBox> = targets
            .iter()
            .map(|b| build_json_for_box(&mut json_file, b, args.decode, &reg))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_boxes)?);
        return Ok(());
    }

    // Text tree
    for b in &targets {
        print_box(&mut f, b, 0, args.max_depth, args.decode, &reg)?;
    }

    // Optional raw dump (unfiltered: still walks the whole tree)
    if let Some(sel) = args.raw.as_ref() {
        dump_raw(&mut f, &top, sel, args.bytes)?;
    }

    Ok(())
}

// ---------- Human-readable tree ----------

fn print_box(
    f: &mut File,
    b: &BoxRef,
    depth: usize,
    max_depth: usize,
    decode: bool,
    reg: &Registry,
) -> anyhow::Result<()> {
    let indent = "  ".repeat(depth);
    let hdr = &b.hdr;
    match &b.kind {
        NodeKind::FullBox { version, flags, .. } => {
            println!(
                "{indent}{:>6} {:>10} {} (ver={}, flags=0x{:06x})",
                format!("{:#x}", hdr.start),
                hdr.size,
                display_type(hdr),
                version,
                flags
            );
            if decode {
                maybe_decode(f, b, reg)?;
            }
        }
        NodeKind::Leaf { .. } | NodeKind::Unknown { .. } => {
            println!(
                "{indent}{:>6} {:>10} {}",
                format!("{:#x}", hdr.start),
                hdr.size,
                display_type(hdr)
            );
            if decode {
                maybe_decode(f, b, reg)?;
            }
        }
        NodeKind::Container(children) => {
            println!(
                "{indent}{:>6} {:>10} {} (container)",
                format!("{:#x}", hdr.start),
                hdr.size,
                display_type(hdr)
            );
            if depth < max_depth {
                for c in children {
                    print_box(f, c, depth + 1, max_depth, decode, reg)?;
                }
            }
        }
    }
    Ok(())
}

fn display_type(h: &mp4box::boxes::BoxHeader) -> String {
    if &h.typ.0 == b"uuid" {
        let u = h.uuid.unwrap_or([0u8; 16]);
        format!(
            "uuid:{}",
            u.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        )
    } else {
        let kb = KnownBox::from(h.typ);
        match kb {
            KnownBox::Unknown(_) => h.typ.to_string(),
            _ => format!("{} ({})", h.typ, kb.full_name()),
        }
    }
}

// ---------- Decoding helpers (shared by text + JSON) ----------

fn payload_region(b: &BoxRef) -> Option<(BoxKey, u64, u64)> {
    let key = if &b.hdr.typ.0 == b"uuid" {
        BoxKey::Uuid(b.hdr.uuid.unwrap())
    } else {
        BoxKey::FourCC(b.hdr.typ)
    };

    match &b.kind {
        NodeKind::FullBox {
            data_offset,
            data_len,
            ..
        } => Some((key, *data_offset, *data_len)),
        NodeKind::Leaf { .. } | NodeKind::Unknown { .. } => {
            let hdr = &b.hdr;
            if hdr.size == 0 {
                return None;
            }
            let off = hdr.start + hdr.header_size;
            let len = hdr.size.saturating_sub(hdr.header_size);
            if len == 0 {
                return None;
            }
            Some((key, off, len))
        }
        NodeKind::Container(_) => None,
    }
}

fn format_structured(data: &StructuredData) -> String {
    use mp4box::registry::*;
    match data {
        StructuredData::TrackHeader(d) => format!(
            "track_id={} duration={} width={} height={} flags=0x{:06X}",
            d.track_id, d.duration, d.width, d.height, d.flags
        ),
        StructuredData::MediaHeader(d) => format!(
            "timescale={} duration={} language={}",
            d.timescale, d.duration, d.language
        ),
        StructuredData::HandlerReference(d) => {
            format!("handler_type={} name={:?}", d.handler_type, d.name)
        }
        StructuredData::SampleDescription(d) => {
            let e = d.entries.first();
            match e {
                Some(e) => match (e.width, e.height) {
                    (Some(w), Some(h)) => {
                        format!("codec={} {}x{} entries={}", e.codec, w, h, d.entry_count)
                    }
                    _ => format!("codec={} entries={}", e.codec, d.entry_count),
                },
                None => "entries=0".to_string(),
            }
        }
        StructuredData::DecodingTimeToSample(d) => {
            let summary: Vec<String> = d
                .entries
                .iter()
                .take(4)
                .map(|e| format!("{}×{}", e.sample_count, e.sample_delta))
                .collect();
            let ellipsis = if d.entry_count > 4 { ", …" } else { "" };
            format!(
                "entries={} [{}{}]",
                d.entry_count,
                summary.join(", "),
                ellipsis
            )
        }
        StructuredData::CompositionTimeToSample(d) => format!("entries={}", d.entry_count),
        StructuredData::SampleToChunk(d) => format!("entries={}", d.entry_count),
        StructuredData::SampleSize(d) => {
            if d.sample_size > 0 {
                format!("fixed_size={} count={}", d.sample_size, d.sample_count)
            } else {
                format!("variable count={}", d.sample_count)
            }
        }
        StructuredData::SyncSample(d) => format!("keyframes={}", d.entry_count),
        StructuredData::ChunkOffset(d) => format!("chunks={}", d.entry_count),
        StructuredData::ChunkOffset64(d) => format!("chunks={}", d.entry_count),
        StructuredData::TrackFragmentRun(d) => {
            let mut parts = vec![format!("samples={}", d.sample_count)];
            if let Some(off) = d.data_offset {
                parts.push(format!("data_offset={}", off));
            }
            let has_dur = d.flags & 0x100 != 0;
            let has_size = d.flags & 0x200 != 0;
            if (has_dur || has_size) && let Some(first) = d.samples.first() {
                if let Some(dur) = first.duration {
                    parts.push(format!("first_dur={}", dur));
                }
                if let Some(sz) = first.size {
                    parts.push(format!("first_size={}", sz));
                }
            }
            parts.join(" ")
        }
    }
}

fn decode_value(f: &mut File, b: &BoxRef, reg: &Registry) -> Option<String> {
    let (key, off, len) = payload_region(b)?;
    if len == 0 {
        return None;
    }

    if f.seek(SeekFrom::Start(off)).is_err() {
        return None;
    }
    let mut limited = f.take(len);

    // Extract version and flags from the box if it's a FullBox
    let (version, flags) = match &b.kind {
        mp4box::boxes::NodeKind::FullBox { version, flags, .. } => (Some(*version), Some(*flags)),
        _ => (None, None),
    };

    if let Some(res) = reg.decode(&key, &mut limited, &b.hdr, version, flags) {
        match res {
            Ok(BoxValue::Text(s)) => Some(s),
            Ok(BoxValue::Bytes(bytes)) => Some(format!("{} bytes", bytes.len())),
            Ok(BoxValue::Structured(data)) => Some(format_structured(&data)),
            Err(e) => Some(format!("[decode error: {}]", e)),
        }
    } else {
        None
    }
}

fn maybe_decode(f: &mut File, b: &BoxRef, reg: &Registry) -> anyhow::Result<()> {
    if let Some(s) = decode_value(f, b, reg) {
        println!("        -> {}", s);
    }
    Ok(())
}

// ---------- Raw dump ----------

fn dump_raw(f: &mut File, boxes: &[BoxRef], sel: &str, limit: usize) -> anyhow::Result<()> {
    let mut matches = Vec::new();
    select_boxes(boxes, sel, &mut matches);
    for (i, (off, len, hdr)) in matches.into_iter().enumerate() {
        let to_read = if limit == 0 || limit as u64 > len {
            len
        } else {
            limit as u64
        };
        let data = read_slice(f, off, to_read)?;
        println!(
            "\n== Dump {} ({}) payload: offset={:#x}, len={} ==",
            i,
            display_type(&hdr),
            off,
            to_read
        );
        print!("{}", hex_dump(&data, off));
    }
    Ok(())
}

fn select_boxes(list: &[BoxRef], sel: &str, out: &mut Vec<(u64, u64, mp4box::boxes::BoxHeader)>) {
    use mp4box::boxes::{BoxHeader, FourCC, NodeKind};

    for b in list {
        let matches_sel = if let Some(u) = b.hdr.uuid {
            if sel.starts_with("uuid:") {
                let hex = sel.trim_start_matches("uuid:").to_ascii_lowercase();
                let flat: String = u.iter().map(|x| format!("{:02x}", x)).collect();
                flat.starts_with(&hex)
            } else {
                false
            }
        } else if sel.len() == 4 {
            b.hdr.typ == FourCC::from_str(sel).unwrap()
        } else {
            false
        };

        if matches_sel {
            match &b.kind {
                NodeKind::FullBox {
                    data_offset,
                    data_len,
                    ..
                } => {
                    out.push((*data_offset, *data_len, b.hdr.clone()));
                }
                NodeKind::Leaf {
                    data_offset,
                    data_len,
                } => {
                    out.push((*data_offset, *data_len, b.hdr.clone()));
                }
                NodeKind::Unknown {
                    data_offset,
                    data_len,
                } => {
                    out.push((*data_offset, *data_len, b.hdr.clone()));
                }
                NodeKind::Container(_) => {
                    let hdr: &BoxHeader = &b.hdr;
                    if hdr.size != 0 && hdr.size > hdr.header_size {
                        let off = hdr.start + hdr.header_size;
                        let len = hdr.size - hdr.header_size;
                        out.push((off, len, hdr.clone()));
                    }
                }
            }
        }

        if let NodeKind::Container(kids) = &b.kind {
            select_boxes(kids, sel, out);
        }
    }
}

// ---------- Filter path: moov.trak[0].mdia.minf.stbl ----------

fn select_by_path<'a>(roots: &'a [BoxRef], path: &str) -> Vec<&'a BoxRef> {
    let mut current: Vec<&'a BoxRef> = roots.iter().collect();

    for (depth, seg) in path.split('.').enumerate() {
        let (name, idx) = parse_segment(seg);
        let fourcc = FourCC::from_str(name).unwrap_or(FourCC(*b"????"));
        let mut next = Vec::new();

        if depth == 0 {
            // match at top level
            let mut matches: Vec<&BoxRef> = current
                .into_iter()
                .filter(|b| b.hdr.typ == fourcc)
                .collect();
            if let Some(i) = idx {
                if i < matches.len() {
                    next.push(matches[i]);
                }
            } else {
                next.append(&mut matches);
            }
        } else {
            // match in children of current set
            for b in &current {
                if let NodeKind::Container(kids) = &b.kind {
                    let mut matches: Vec<&BoxRef> =
                        kids.iter().filter(|c| c.hdr.typ == fourcc).collect();
                    if let Some(i) = idx {
                        if i < matches.len() {
                            next.push(matches[i]);
                        }
                    } else {
                        next.append(&mut matches);
                    }
                }
            }
        }

        current = next;
        if current.is_empty() {
            break;
        }
    }

    current
}

fn parse_segment(seg: &str) -> (&str, Option<usize>) {
    if let Some(l) = seg.find('[') {
        let name = &seg[..l];
        if let Some(r) = seg[l + 1..].find(']') {
            let idx_str = &seg[l + 1..l + 1 + r];
            let idx = idx_str.parse::<usize>().ok();
            return (name, idx);
        }
        (name, None)
    } else {
        (seg, None)
    }
}

// ---------- JSON representation ----------

#[derive(Serialize)]
struct JsonBox {
    offset: u64,
    size: u64,
    header_size: u64,
    payload_offset: Option<u64>,
    payload_size: Option<u64>,

    typ: String,
    uuid: Option<String>,
    version: Option<u8>,
    flags: Option<u32>,
    kind: String,
    full_name: String,
    decoded: Option<String>,
    children: Option<Vec<JsonBox>>,
}

fn payload_geometry(b: &BoxRef) -> Option<(u64, u64)> {
    match &b.kind {
        NodeKind::FullBox {
            data_offset,
            data_len,
            ..
        } => Some((*data_offset, *data_len)),
        NodeKind::Leaf { .. } | NodeKind::Unknown { .. } => {
            let hdr = &b.hdr;
            if hdr.size == 0 {
                return None;
            }
            let off = hdr.start + hdr.header_size;
            let len = hdr.size.saturating_sub(hdr.header_size);
            if len == 0 {
                return None;
            }
            Some((off, len))
        }
        NodeKind::Container(_) => None,
    }
}

fn build_json_for_box(f: &mut File, b: &BoxRef, decode: bool, reg: &Registry) -> JsonBox {
    let hdr = &b.hdr;
    let uuid_str = hdr
        .uuid
        .map(|u| u.iter().map(|b| format!("{:02x}", b)).collect::<String>());

    let kb = mp4box::known_boxes::KnownBox::from(hdr.typ);
    let full_name = kb.full_name().to_string();

    let header_size = hdr.header_size;
    let (payload_offset, payload_size) = payload_geometry(b)
        .map(|(off, len)| (Some(off), Some(len)))
        .unwrap_or((None, None));

    let (version, flags, kind_str, children) = match &b.kind {
        NodeKind::FullBox { version, flags, .. } => {
            (Some(*version), Some(*flags), "full".to_string(), None)
        }
        NodeKind::Leaf { .. } => (None, None, "leaf".to_string(), None),
        NodeKind::Unknown { .. } => (None, None, "unknown".to_string(), None),
        NodeKind::Container(kids) => {
            let child_nodes = kids
                .iter()
                .map(|c| build_json_for_box(f, c, decode, reg))
                .collect();
            (None, None, "container".to_string(), Some(child_nodes))
        }
    };

    let decoded = if decode {
        decode_value(f, b, reg)
    } else {
        None
    };

    JsonBox {
        offset: hdr.start,
        size: hdr.size,
        header_size,
        payload_offset,
        payload_size,

        typ: hdr.typ.to_string(),
        uuid: uuid_str,
        version,
        flags,
        kind: kind_str,
        full_name,
        decoded,
        children,
    }
}

fn is_container(h: &BoxHeader) -> bool {
    KnownBox::from(h.typ).is_container()
}

fn is_full_box(h: &BoxHeader) -> bool {
    KnownBox::from(h.typ).is_full_box()
}
