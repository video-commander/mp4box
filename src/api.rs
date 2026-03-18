use crate::{
    boxes::{BoxRef, NodeKind},
    parser::read_box_header,
    registry::{BoxValue, Registry, default_registry},
    util::{hex_dump, read_slice},
};
use byteorder::ReadBytesExt;
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};

/// A JSON-serializable representation of a single MP4 box.
///
/// This structure contains all the metadata and content information about an MP4 box,
/// making it suitable for serialization to JSON for use in web UIs, CLIs, or APIs.
#[derive(Serialize)]
pub struct Box {
    /// Absolute byte offset of this box in the file
    pub offset: u64,
    /// Total size of this box including header and payload
    pub size: u64,
    /// Size of just the box header (8 bytes for normal boxes, 16+ for large boxes)
    pub header_size: u64,
    /// Absolute offset where payload data starts (None for containers)
    pub payload_offset: Option<u64>,
    /// Size of payload data (None for containers)
    pub payload_size: Option<u64>,

    /// Four-character box type code (e.g., "ftyp", "moov")
    pub typ: String,
    /// UUID for UUID boxes (16-byte hex string)
    pub uuid: Option<String>,
    /// Version field for FullBox types
    pub version: Option<u8>,
    /// Flags field for FullBox types  
    pub flags: Option<u32>,
    /// Box classification: "leaf", "full", "container", or "unknown"
    pub kind: String,
    /// Human-readable box type name (e.g., "File Type Box")
    pub full_name: String,
    /// Decoded box content if decode=true and decoder available
    pub decoded: Option<String>,
    /// Structured data if decode=true and structured decoder available
    pub structured_data: Option<crate::registry::StructuredData>,
    /// Child boxes for container types
    pub children: Option<Vec<Box>>,
}

/// Parse an MP4/ISOBMFF file and return the complete box tree as JSON-serializable structures.
///
/// # Parameters
/// - `r`: A reader that implements `Read + Seek` (e.g., `File`, `Cursor<Vec<u8>>`)
/// - `size`: The total size of the MP4 data to parse (typically file length)  
/// - `decode`: Whether to decode known box types using the default registry
///
/// # Returns
/// A vector of `Box` structs representing the top-level boxes in the file.
/// Each box contains metadata (offset, size, type) and optionally decoded content.
///
/// # Example
/// ```no_run
/// use mp4box::get_boxes;
/// use std::fs::File;
///
/// let mut file = File::open("video.mp4")?;
/// let size = file.metadata()?.len();
/// let boxes = get_boxes(&mut file, size, true)?; // decode known boxes
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn get_boxes<R: Read + Seek>(r: &mut R, size: u64, decode: bool) -> anyhow::Result<Vec<Box>> {
    get_boxes_with_registry(r, size, decode, default_registry())
}

/// Parse an MP4/ISOBMFF file and return the complete box tree as JSON-serializable structures.
///
/// # Parameters
/// - `r`: A reader that implements `Read + Seek` (e.g., `File`, `Cursor<Vec<u8>>`)
/// - `size`: The total size of the MP4 data to parse (typically file length)
/// - `decode`: Whether to decode known box types using the default registry
///
/// # Returns
/// A vector of `Box` structs representing the top-level boxes in the file.
/// Each box contains metadata (offset, size, type) and optionally decoded content.
///
/// # Example
/// ```no_run
/// use mp4box::{get_boxes_with_registry, registry::default_registry};
/// use std::fs::File;
///
/// let mut file = File::open("video.mp4")?;
/// let size = file.metadata()?.len();
/// let boxes = get_boxes_with_registry(&mut file, size, true, default_registry())?; // decode known boxes
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn get_boxes_with_registry<R: Read + Seek>(r: &mut R, size: u64, decode: bool, registry: Registry) -> anyhow::Result<Vec<Box>> {
    // let mut f = File::open(&path)?;
    // let file_len = f.metadata()?.len();

    // parse top-level boxes
    let mut boxes = Vec::new();
    while r.stream_position()? < size {
        let h = read_box_header(r)?;
        let box_end = if h.size == 0 { size } else { h.start + h.size };

        let kind = if crate::known_boxes::KnownBox::from(h.typ).is_container() {
            r.seek(SeekFrom::Start(h.start + h.header_size))?;
            NodeKind::Container(crate::parser::parse_children(r, box_end)?)
        } else if crate::known_boxes::KnownBox::from(h.typ).is_full_box() {
            r.seek(SeekFrom::Start(h.start + h.header_size))?;
            let version = r.read_u8()?;
            let mut fl = [0u8; 3];
            r.read_exact(&mut fl)?;
            let flags = ((fl[0] as u32) << 16) | ((fl[1] as u32) << 8) | (fl[2] as u32);
            let data_offset = r.stream_position()?;
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

        r.seek(SeekFrom::Start(box_end))?;
        boxes.push(BoxRef { hdr: h, kind });
    }

    // build JSON tree
    let json_boxes = boxes
        .iter()
        .map(|b| build_box(r, b, decode, &registry))
        .collect();

    Ok(json_boxes)
}

fn payload_region(b: &BoxRef) -> Option<(crate::boxes::BoxKey, u64, u64)> {
    let key = if &b.hdr.typ.0 == b"uuid" {
        crate::boxes::BoxKey::Uuid(b.hdr.uuid.unwrap())
    } else {
        crate::boxes::BoxKey::FourCC(b.hdr.typ)
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

fn decode_value<R: Read + Seek>(
    r: &mut R,
    b: &BoxRef,
    reg: &Registry,
) -> (Option<String>, Option<crate::registry::StructuredData>) {
    let (key, off, len) = match payload_region(b) {
        Some(region) => region,
        None => return (None, None),
    };
    if len == 0 {
        return (None, None);
    }

    if r.seek(SeekFrom::Start(off)).is_err() {
        return (None, None);
    }
    let mut limited = r.take(len);

    // Extract version and flags from the box if it's a FullBox
    let (version, flags) = match &b.kind {
        crate::boxes::NodeKind::FullBox { version, flags, .. } => (Some(*version), Some(*flags)),
        _ => (None, None),
    };

    if let Some(res) = reg.decode(&key, &mut limited, &b.hdr, version, flags) {
        match res {
            Ok(BoxValue::Text(s)) => (Some(s), None),
            Ok(BoxValue::Bytes(bytes)) => (Some(format!("{} bytes", bytes.len())), None),
            Ok(BoxValue::Structured(data)) => {
                let debug_str = format!("structured: {:?}", data);
                (Some(debug_str), Some(data))
            }
            Err(e) => (Some(format!("[decode error: {}]", e)), None),
        }
    } else {
        (None, None)
    }
}

fn build_box<R: Read + Seek>(r: &mut R, b: &BoxRef, decode: bool, reg: &Registry) -> Box {
    let hdr = &b.hdr;
    let uuid_str = hdr
        .uuid
        .map(|u| u.iter().map(|b| format!("{:02x}", b)).collect::<String>());

    let kb = crate::known_boxes::KnownBox::from(hdr.typ);
    let full_name = kb.full_name().to_string();

    // basic geometry
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
            let child_nodes = kids.iter().map(|c| build_box(r, c, decode, reg)).collect();
            (None, None, "container".to_string(), Some(child_nodes))
        }
    };

    let (decoded, structured_data) = if decode {
        decode_value(r, b, reg)
    } else {
        (None, None)
    };

    Box {
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
        structured_data,
        children,
    }
}

/// Result of a hex dump operation containing the formatted hex output.
#[derive(Serialize)]
pub struct HexDump {
    /// Starting offset of the dumped data
    pub offset: u64,
    /// Actual number of bytes that were read and dumped
    pub length: u64,
    /// Formatted hex dump string with addresses and ASCII representation
    pub hex: String,
}

/// Hex-dump a range of bytes from an MP4 data source.
///
/// # Parameters
/// - `r`: A reader that implements `Read + Seek`
/// - `size`: The total size of the data (typically file length)
/// - `offset`: Byte offset to start reading from
/// - `max_len`: Maximum number of bytes to read
///
/// This function never reads past EOF; if `offset + max_len` goes beyond the data size,
/// the returned length will be smaller than `max_len`.
///
/// This is useful for building a hex viewer UI:
///
/// ```no_run
/// use mp4box::hex_range;
/// use std::fs::File;
///
/// fn main() -> anyhow::Result<()> {
///     let mut file = File::open("video.mp4")?;
///     let size = file.metadata()?.len();
///     let dump = hex_range(&mut file, size, 0, 256)?;
///     println!("{}", dump.hex);
///     Ok(())
/// }
/// ```
pub fn hex_range<R: Read + Seek>(
    r: &mut R,
    size: u64,
    offset: u64,
    max_len: u64,
) -> anyhow::Result<HexDump> {
    use std::cmp::min;

    // let path = path.as_ref().to_path_buf();
    // let mut f = File::open(&path)?;
    // let file_len = f.metadata()?.len();

    // How many bytes are actually available from this offset to EOF.
    let available = size.saturating_sub(offset);

    // Don't read past EOF or more than the caller requested.
    let to_read = min(available, max_len);

    // If nothing is available, just return an empty dump.
    if to_read == 0 {
        return Ok(HexDump {
            offset,
            length: 0,
            hex: String::new(),
        });
    }

    let data = read_slice(r, offset, to_read)?;
    let hex_str = hex_dump(&data, offset);

    Ok(HexDump {
        offset,
        length: to_read, // <-- IMPORTANT: actual bytes read, not max_len
        hex: hex_str,
    })
}
