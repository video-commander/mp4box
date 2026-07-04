use crate::boxes::{BoxHeader, BoxRef, FourCC, NodeKind};
use crate::known_boxes::KnownBox;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid box size")]
    InvalidSize,
}

pub type Result<T> = std::result::Result<T, ParseError>;

pub fn read_box_header<R: Read + Seek>(r: &mut R) -> Result<BoxHeader> {
    let start = r.stream_position()?;
    let size32 = r.read_u32::<BigEndian>()?;
    let mut typ = [0u8; 4];
    r.read_exact(&mut typ)?;
    let mut size = size32 as u64;

    if size32 == 1 {
        size = r.read_u64::<BigEndian>()?;
    }

    let mut uuid = None;
    if &typ == b"uuid" {
        let mut u = [0u8; 16];
        r.read_exact(&mut u)?;
        uuid = Some(u);
    }

    let header_size = match (size32 == 1, &typ == b"uuid") {
        (true, true) => 8 + 8 + 16,
        (true, false) => 8 + 8,
        (false, true) => 8 + 16,
        (false, false) => 8,
    } as u64;

    if size != 0 && size < header_size {
        return Err(ParseError::InvalidSize);
    }

    Ok(BoxHeader {
        size,
        typ: FourCC(typ),
        uuid,
        header_size,
        start,
    })
}

/// Parse all boxes in the byte range `[start, end)`.
///
/// This is the entry point for parsing a whole file (`start = 0`,
/// `end = file length`) or any sub-range. The first malformed box aborts
/// the parse with an error; see [`parse_boxes_tolerant`] for the recovering
/// variant.
pub fn parse_boxes<R: Read + Seek>(r: &mut R, start: u64, end: u64) -> Result<Vec<BoxRef>> {
    r.seek(SeekFrom::Start(start))?;
    parse_children(r, end)
}

/// A problem encountered while parsing in tolerant mode.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParseIssue {
    /// File offset where the problem was detected.
    pub offset: u64,
    /// Human-readable description.
    pub message: String,
}

impl std::fmt::Display for ParseIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}: {}", self.offset, self.message)
    }
}

/// Like [`parse_boxes`], but recovers from malformed boxes instead of
/// aborting: the tree parsed so far is returned together with a list of
/// [`ParseIssue`]s describing what was wrong and where.
///
/// Damage is contained to the enclosing container: an unreadable child
/// header abandons the *rest of that container's* bytes, while siblings of
/// the container and everything above it keep parsing. A box whose interior
/// fails to parse is downgraded to an opaque leaf. Boxes whose declared size
/// overruns their parent are clamped (as in strict mode) and reported.
///
/// Only returns `Err` for I/O failures on the initial seek.
pub fn parse_boxes_tolerant<R: Read + Seek>(
    r: &mut R,
    start: u64,
    end: u64,
) -> Result<(Vec<BoxRef>, Vec<ParseIssue>)> {
    r.seek(SeekFrom::Start(start))?;
    let mut issues = Vec::new();
    let boxes = match parse_children_impl(r, end, &mut Some(&mut issues)) {
        Ok(boxes) => boxes,
        // parse_children_impl only propagates errors in strict mode or on
        // seek failures; treat the latter as a terminal issue.
        Err(e) => {
            issues.push(ParseIssue {
                offset: r.stream_position().unwrap_or(start),
                message: format!("parse aborted: {}", e),
            });
            Vec::new()
        }
    };
    Ok((boxes, issues))
}

/// Parse sibling boxes from the current stream position up to `parent_end`.
pub fn parse_children<R: Read + Seek>(r: &mut R, parent_end: u64) -> Result<Vec<BoxRef>> {
    parse_children_impl(r, parent_end, &mut None)
}

/// Shared strict/tolerant implementation: `issues: None` is strict mode
/// (first error propagates), `Some` records problems and recovers.
fn parse_children_impl<R: Read + Seek>(
    r: &mut R,
    parent_end: u64,
    issues: &mut Option<&mut Vec<ParseIssue>>,
) -> Result<Vec<BoxRef>> {
    let mut kids = Vec::new();
    // A box header needs at least 8 bytes; ignore trailing padding shorter
    // than that instead of erroring out.
    while r.stream_position()? + 8 <= parent_end {
        let header_start = r.stream_position()?;
        let h = match read_box_header(r) {
            Ok(h) => h,
            Err(e) => {
                let Some(issues) = issues else {
                    return Err(e);
                };
                // No reliable way to resync after a bad header; abandon the
                // rest of this container.
                issues.push(ParseIssue {
                    offset: header_start,
                    message: format!(
                        "unreadable box header ({}); skipping remaining {} bytes of container",
                        e,
                        parent_end - header_start
                    ),
                });
                break;
            }
        };

        if let Some(issues) = issues
            && h.size != 0
            && h.start + h.size > parent_end
        {
            issues.push(ParseIssue {
                offset: h.start,
                message: format!(
                    "box '{}' declares size {} which overruns its container by {} bytes; clamped",
                    h.typ,
                    h.size,
                    h.start + h.size - parent_end
                ),
            });
        }
        let box_end = box_end(&h, parent_end);

        let kind = match classify_box(r, &h, box_end, issues) {
            Ok(kind) => kind,
            Err(e) => {
                let Some(issues) = issues else {
                    return Err(e);
                };
                // The box's interior couldn't be parsed; keep it as an
                // opaque leaf so the tree stays navigable.
                issues.push(ParseIssue {
                    offset: h.start,
                    message: format!("failed to parse contents of '{}': {}", h.typ, e),
                });
                let data_offset = h.start + h.header_size;
                NodeKind::Leaf {
                    data_offset,
                    data_len: box_end.saturating_sub(data_offset),
                }
            }
        };

        // Skip to end of box
        r.seek(SeekFrom::Start(box_end))?;
        kids.push(BoxRef { hdr: h, kind });
    }
    Ok(kids)
}

/// Compute the exclusive end offset of a box, clamped to its parent so a
/// corrupt size can't leak into sibling/parent data. `size == 0` means
/// "extends to the end of the enclosing range".
fn box_end(h: &BoxHeader, parent_end: u64) -> u64 {
    if h.size == 0 {
        parent_end
    } else {
        (h.start + h.size).min(parent_end)
    }
}

fn read_version_flags<R: Read>(r: &mut R) -> Result<(u8, u32)> {
    let version = r.read_u8()?;
    let mut f = [0u8; 3];
    r.read_exact(&mut f)?;
    let flags = ((f[0] as u32) << 16) | ((f[1] as u32) << 8) | (f[2] as u32);
    Ok((version, flags))
}

/// Decide what kind of node a box is and parse its interior accordingly.
/// Leaves the stream position unspecified; callers seek to `box_end` after.
fn classify_box<R: Read + Seek>(
    r: &mut R,
    h: &BoxHeader,
    box_end: u64,
    issues: &mut Option<&mut Vec<ParseIssue>>,
) -> Result<NodeKind> {
    let kb = KnownBox::from(h.typ);
    let content_start = h.start + h.header_size;

    if kb == KnownBox::Stsd {
        return parse_stsd(r, h, box_end, issues);
    }

    if kb.is_full_container() {
        // QuickTime writes `meta` without version/flags; sniff for that.
        if kb == KnownBox::Meta && meta_is_quicktime_style(r, content_start, box_end)? {
            r.seek(SeekFrom::Start(content_start))?;
            return Ok(NodeKind::Container(parse_children_impl(
                r, box_end, issues,
            )?));
        }
        r.seek(SeekFrom::Start(content_start))?;
        let (version, flags) = read_version_flags(r)?;
        let data_offset = r.stream_position()?;
        let data_len = box_end.saturating_sub(data_offset);
        let children = parse_children_impl(r, box_end, issues)?;
        return Ok(NodeKind::FullContainer {
            version,
            flags,
            data_offset,
            data_len,
            children,
        });
    }

    if kb.is_container() {
        r.seek(SeekFrom::Start(content_start))?;
        return Ok(NodeKind::Container(parse_children_impl(
            r, box_end, issues,
        )?));
    }

    if kb.is_full_box() {
        r.seek(SeekFrom::Start(content_start))?;
        let (version, flags) = read_version_flags(r)?;
        let data_offset = r.stream_position()?;
        let data_len = box_end.saturating_sub(data_offset);
        return Ok(NodeKind::FullBox {
            version,
            flags,
            data_offset,
            data_len,
        });
    }

    let data_offset = content_start;
    let data_len = box_end.saturating_sub(data_offset);
    if &h.typ.0 == b"uuid" {
        Ok(NodeKind::Unknown {
            data_offset,
            data_len,
        })
    } else {
        Ok(NodeKind::Leaf {
            data_offset,
            data_len,
        })
    }
}

/// ISO `meta` is a FullBox (4 bytes of version/flags before children), but
/// QuickTime writes it as a plain container. Detect the QT flavor by checking
/// whether the first 8 bytes already look like a valid child box header:
/// for the ISO flavor those bytes are version/flags (almost always
/// 0x00000000), which can't be a valid box size.
fn meta_is_quicktime_style<R: Read + Seek>(
    r: &mut R,
    content_start: u64,
    box_end: u64,
) -> Result<bool> {
    if content_start + 8 > box_end {
        return Ok(false);
    }
    r.seek(SeekFrom::Start(content_start))?;
    let size = r.read_u32::<BigEndian>()? as u64;
    let mut typ = [0u8; 4];
    r.read_exact(&mut typ)?;
    Ok(size >= 8 && content_start + size <= box_end && fourcc_is_printable(&FourCC(typ)))
}

fn fourcc_is_printable(cc: &FourCC) -> bool {
    cc.0.iter().all(|&c| (0x20..=0x7e).contains(&c))
}

// ---------- stsd and sample entries ----------

/// `stsd` is a FullBox whose payload is `entry_count` followed by sample
/// entry boxes (avc1, mp4a, ...). Each sample entry in turn has a
/// codec-family-specific fixed header followed by child boxes (avcC, esds,
/// pasp, btrt, ...).
fn parse_stsd<R: Read + Seek>(
    r: &mut R,
    h: &BoxHeader,
    box_end: u64,
    issues: &mut Option<&mut Vec<ParseIssue>>,
) -> Result<NodeKind> {
    let content_start = h.start + h.header_size;
    r.seek(SeekFrom::Start(content_start))?;
    let (version, flags) = read_version_flags(r)?;
    let data_offset = r.stream_position()?;
    let data_len = box_end.saturating_sub(data_offset);
    let _entry_count = r.read_u32::<BigEndian>()?;

    let mut children = Vec::new();
    while r.stream_position()? + 8 <= box_end {
        let entry_start = r.stream_position()?;
        let eh = match read_box_header(r) {
            Ok(eh) => eh,
            Err(e) => {
                let Some(issues) = issues else {
                    return Err(e);
                };
                issues.push(ParseIssue {
                    offset: entry_start,
                    message: format!("unreadable stsd sample entry header ({})", e),
                });
                break;
            }
        };
        let entry_end = self::box_end(&eh, box_end);
        let kind = parse_sample_entry(r, &eh, entry_end)?;
        r.seek(SeekFrom::Start(entry_end))?;
        children.push(BoxRef { hdr: eh, kind });
    }

    Ok(NodeKind::FullContainer {
        version,
        flags,
        data_offset,
        data_len,
        children,
    })
}

/// Size of the fixed (non-box) fields of a sample entry, from the end of its
/// box header to where child boxes begin.
fn sample_entry_fixed_len<R: Read + Seek>(
    r: &mut R,
    typ: FourCC,
    content_start: u64,
    entry_end: u64,
) -> Result<Option<u64>> {
    // SampleEntry base: 6 reserved bytes + data_reference_index (2).
    // VisualSampleEntry adds 70 bytes of fixed fields.
    const VISUAL: u64 = 8 + 70;
    // AudioSampleEntry (version 0) adds 20 bytes of fixed fields.
    const AUDIO_V0: u64 = 8 + 20;
    // QuickTime sound sample description v1 adds 16 bytes, v2 is 64 total.
    const AUDIO_V1: u64 = AUDIO_V0 + 16;
    const AUDIO_V2: u64 = 8 + 56;

    match KnownBox::from(typ) {
        KnownBox::Avc1
        | KnownBox::Avc2
        | KnownBox::Avc3
        | KnownBox::Avc4
        | KnownBox::Hev1
        | KnownBox::Hvc1
        | KnownBox::Vvc1
        | KnownBox::Mp4v
        | KnownBox::Vp08
        | KnownBox::Vp09
        | KnownBox::Av01
        | KnownBox::Dvh1
        | KnownBox::Dvhe
        | KnownBox::Dav1
        | KnownBox::Encv => Ok(Some(VISUAL)),
        KnownBox::Mp4a
        | KnownBox::Ac3
        | KnownBox::Ec3
        | KnownBox::Opus
        | KnownBox::Samr
        | KnownBox::Sawb
        | KnownBox::Alac
        | KnownBox::Flac
        | KnownBox::Enca
        | KnownBox::Ipcm
        | KnownBox::Fpcm => {
            // The first 2 bytes of the reserved area hold the QuickTime
            // sound sample description version (0 in plain ISO files).
            if content_start + 10 > entry_end {
                return Ok(None);
            }
            r.seek(SeekFrom::Start(content_start + 8))?;
            let qt_version = r.read_u16::<BigEndian>()?;
            Ok(Some(match qt_version {
                1 => AUDIO_V1,
                2 => AUDIO_V2,
                _ => AUDIO_V0,
            }))
        }
        _ => Ok(None),
    }
}

/// Parse the child boxes of a sample entry, falling back to a leaf if the
/// entry type is unknown or its contents don't look like boxes.
fn parse_sample_entry<R: Read + Seek>(
    r: &mut R,
    h: &BoxHeader,
    entry_end: u64,
) -> Result<NodeKind> {
    let content_start = h.start + h.header_size;
    let leaf = NodeKind::Leaf {
        data_offset: content_start,
        data_len: entry_end.saturating_sub(content_start),
    };

    let Some(fixed) = sample_entry_fixed_len(r, h.typ, content_start, entry_end)? else {
        return Ok(leaf);
    };

    let child_start = content_start + fixed;
    if child_start + 8 > entry_end {
        return Ok(leaf);
    }

    r.seek(SeekFrom::Start(child_start))?;
    match parse_children(r, entry_end) {
        Ok(kids) if !kids.is_empty() && kids.iter().all(|k| fourcc_is_printable(&k.hdr.typ)) => {
            Ok(NodeKind::Container(kids))
        }
        // Unexpected layout (e.g. vendor extensions before the child boxes):
        // expose the entry as an opaque leaf rather than a garbage subtree.
        _ => Ok(leaf),
    }
}
