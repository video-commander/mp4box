//! The edit tree: a mutable representation of an MP4 file where unmodified
//! payloads are *references into the source file* (extents) rather than
//! copies. Serializing an unmodified tree reproduces the source byte for
//! byte; mutations replace nodes with in-memory bytes and every enclosing
//! box size is recomputed during layout, so ancestor sizes are correct by
//! construction.

use crate::boxes::{BoxRef, FourCC, NodeKind};
use std::io::{Read, Seek, SeekFrom, Write};

/// How the original box header was written, preserved on round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderForm {
    /// 32-bit size
    Compact,
    /// size32 == 1 with a 64-bit largesize
    Large,
    /// size32 == 0: box extends to the end of the file
    ToEof,
}

/// A byte range in the source file.
#[derive(Debug, Clone, Copy)]
pub struct Extent {
    pub offset: u64,
    pub len: u64,
}

/// Payload of an edit node.
#[derive(Debug)]
pub enum Payload {
    /// Unmodified bytes living in the source file (everything after the box
    /// header, including version/flags for FullBox leaves).
    Extent(Extent),
    /// Replaced payload bytes (everything after the box header).
    Bytes(Vec<u8>),
    /// A container of child boxes.
    Container {
        /// version/flags for FullBox containers (`meta`, `stsd`, ...)
        version_flags: Option<(u8, u32)>,
        /// Non-box bytes between the header (or version/flags) and the first
        /// child: stsd's entry_count, a sample entry's fixed fields, ...
        prefix: Vec<u8>,
        children: Vec<EditNode>,
        /// Trailing non-box bytes inside the box after the last child
        /// (padding smaller than a box header), preserved verbatim.
        suffix: Vec<u8>,
    },
}

/// One box in the edit tree.
#[derive(Debug)]
pub struct EditNode {
    pub typ: FourCC,
    pub uuid: Option<[u8; 16]>,
    pub header_form: HeaderForm,
    pub payload: Payload,
}

impl EditNode {
    /// Create a node from a complete raw box (header + payload), e.g. for
    /// inserted boxes. The interior is kept opaque.
    pub fn from_raw(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() >= 8, "raw box shorter than a box header");
        let size32 = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        let typ = FourCC([bytes[4], bytes[5], bytes[6], bytes[7]]);

        let (header_form, declared, mut header_len) = if size32 == 1 {
            anyhow::ensure!(bytes.len() >= 16, "largesize box shorter than 16 bytes");
            let large = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
            (HeaderForm::Large, large, 16usize)
        } else if size32 == 0 {
            (HeaderForm::ToEof, bytes.len() as u64, 8usize)
        } else {
            (HeaderForm::Compact, size32 as u64, 8usize)
        };
        anyhow::ensure!(
            declared == bytes.len() as u64,
            "raw box declares size {} but {} bytes were provided",
            declared,
            bytes.len()
        );

        let mut uuid = None;
        if &typ.0 == b"uuid" {
            anyhow::ensure!(bytes.len() >= header_len + 16, "uuid box too short");
            let mut u = [0u8; 16];
            u.copy_from_slice(&bytes[header_len..header_len + 16]);
            uuid = Some(u);
            header_len += 16;
        }

        Ok(EditNode {
            typ,
            uuid,
            header_form: if header_form == HeaderForm::ToEof {
                HeaderForm::Compact // re-emit with a real size
            } else {
                header_form
            },
            payload: Payload::Bytes(bytes[header_len..].to_vec()),
        })
    }

    /// The children of this node, if it is a container.
    pub fn children(&self) -> Option<&Vec<EditNode>> {
        match &self.payload {
            Payload::Container { children, .. } => Some(children),
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<EditNode>> {
        match &mut self.payload {
            Payload::Container { children, .. } => Some(children),
            _ => None,
        }
    }
}

/// The whole file: top-level boxes plus any trailing bytes after the last
/// box (shorter than a box header).
pub struct EditTree {
    pub roots: Vec<EditNode>,
    pub trailing: Vec<u8>,
}

// ---------- building ----------

/// Build an edit tree from a parsed box tree, reading small non-box regions
/// (version/flags, prefixes, padding) from the source.
pub fn build_tree<R: Read + Seek>(
    r: &mut R,
    boxes: &[BoxRef],
    file_len: u64,
) -> anyhow::Result<EditTree> {
    let roots = boxes
        .iter()
        .map(|b| build_node(r, b, file_len))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Preserve trailing bytes after the last top-level box.
    let last_end = boxes
        .iter()
        .last()
        .map(|b| box_end(b, file_len))
        .unwrap_or(0);
    let trailing = read_range(r, last_end, file_len.saturating_sub(last_end))?;

    Ok(EditTree { roots, trailing })
}

fn box_end(b: &BoxRef, parent_end: u64) -> u64 {
    if b.hdr.size == 0 {
        parent_end
    } else {
        (b.hdr.start + b.hdr.size).min(parent_end)
    }
}

fn read_range<R: Read + Seek>(r: &mut R, offset: u64, len: u64) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    r.seek(SeekFrom::Start(offset))?;
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn header_form_of(b: &BoxRef) -> HeaderForm {
    if b.hdr.size == 0 {
        HeaderForm::ToEof
    } else {
        // header_size: 8/24 compact, 16/32 large (uuid adds 16)
        let base = b.hdr.header_size - if b.hdr.uuid.is_some() { 16 } else { 0 };
        if base == 16 {
            HeaderForm::Large
        } else {
            HeaderForm::Compact
        }
    }
}

fn build_node<R: Read + Seek>(r: &mut R, b: &BoxRef, parent_end: u64) -> anyhow::Result<EditNode> {
    let end = box_end(b, parent_end);
    let content_start = b.hdr.start + b.hdr.header_size;

    let payload = match &b.kind {
        NodeKind::Leaf {
            data_offset: _,
            data_len: _,
        }
        | NodeKind::Unknown { .. }
        | NodeKind::FullBox { .. } => {
            // Everything after the header, including version/flags.
            Payload::Extent(Extent {
                offset: content_start,
                len: end.saturating_sub(content_start),
            })
        }
        NodeKind::Container(kids) => {
            container_payload(r, None, content_start, end, kids, parent_end_for(kids, end))?
        }
        NodeKind::FullContainer {
            version,
            flags,
            data_offset,
            children,
            ..
        } => container_payload(
            r,
            Some((*version, *flags)),
            *data_offset,
            end,
            children,
            parent_end_for(children, end),
        )?,
    };

    Ok(EditNode {
        typ: b.hdr.typ,
        uuid: b.hdr.uuid,
        header_form: header_form_of(b),
        payload,
    })
}

fn parent_end_for(_kids: &[BoxRef], end: u64) -> u64 {
    end
}

fn container_payload<R: Read + Seek>(
    r: &mut R,
    version_flags: Option<(u8, u32)>,
    content_start: u64,
    end: u64,
    kids: &[BoxRef],
    child_parent_end: u64,
) -> anyhow::Result<Payload> {
    // prefix: bytes between content start and the first child (stsd
    // entry_count, sample entry fixed fields, ...)
    let first_child_start = kids.first().map(|k| k.hdr.start).unwrap_or(end);
    let prefix = read_range(r, content_start, first_child_start - content_start)?;

    let mut children = Vec::with_capacity(kids.len());
    for k in kids {
        children.push(build_node(r, k, child_parent_end)?);
    }

    // suffix: padding after the last child, inside this box
    let last_child_end = kids.last().map(|k| box_end(k, end)).unwrap_or(end);
    let suffix = read_range(r, last_child_end, end.saturating_sub(last_child_end))?;

    Ok(Payload::Container {
        version_flags,
        prefix,
        children,
        suffix,
    })
}

// ---------- layout ----------

/// Where a source extent lands in the output.
#[derive(Debug, Clone, Copy)]
pub struct ExtentMapping {
    pub old_offset: u64,
    pub len: u64,
    pub new_offset: u64,
}

/// Size of a node's header for its (possibly upgraded) form.
fn header_len(node: &EditNode, payload_len: u64) -> u64 {
    let uuid_len = if node.uuid.is_some() { 16 } else { 0 };
    match node.header_form {
        HeaderForm::Large => 16 + uuid_len,
        HeaderForm::ToEof | HeaderForm::Compact => {
            // Upgrade to largesize if the compact form can't express it.
            if node.header_form == HeaderForm::Compact
                && 8 + uuid_len + payload_len > u32::MAX as u64
            {
                16 + uuid_len
            } else {
                8 + uuid_len
            }
        }
    }
}

pub fn payload_len(node: &EditNode) -> u64 {
    match &node.payload {
        Payload::Extent(e) => e.len,
        Payload::Bytes(b) => b.len() as u64,
        Payload::Container {
            version_flags,
            prefix,
            children,
            suffix,
        } => {
            let vf = if version_flags.is_some() { 4 } else { 0 };
            vf + prefix.len() as u64
                + children.iter().map(node_size).sum::<u64>()
                + suffix.len() as u64
        }
    }
}

/// Total serialized size of a node (header + payload).
pub fn node_size(node: &EditNode) -> u64 {
    let p = payload_len(node);
    header_len(node, p) + p
}

/// Compute the extent map for the current tree layout: every source extent
/// paired with the offset it will occupy in the output.
pub fn layout(tree: &EditTree) -> Vec<ExtentMapping> {
    let mut map = Vec::new();
    let mut pos = 0u64;
    for node in &tree.roots {
        layout_node(node, &mut pos, &mut map);
    }
    map
}

fn layout_node(node: &EditNode, pos: &mut u64, map: &mut Vec<ExtentMapping>) {
    let p = payload_len(node);
    *pos += header_len(node, p);
    match &node.payload {
        Payload::Extent(e) => {
            map.push(ExtentMapping {
                old_offset: e.offset,
                len: e.len,
                new_offset: *pos,
            });
            *pos += e.len;
        }
        Payload::Bytes(b) => {
            *pos += b.len() as u64;
        }
        Payload::Container {
            version_flags,
            prefix,
            children,
            suffix,
        } => {
            if version_flags.is_some() {
                *pos += 4;
            }
            *pos += prefix.len() as u64;
            for c in children {
                layout_node(c, pos, map);
            }
            *pos += suffix.len() as u64;
        }
    }
}

// ---------- writing ----------

/// Serialize the tree to `dst`, streaming unmodified extents from `src`.
pub fn write_tree<R: Read + Seek, W: Write>(
    src: &mut R,
    tree: &EditTree,
    dst: &mut W,
) -> anyhow::Result<u64> {
    let mut written = 0u64;
    for node in &tree.roots {
        written += write_node(src, node, dst)?;
    }
    dst.write_all(&tree.trailing)?;
    written += tree.trailing.len() as u64;
    Ok(written)
}

fn write_header<W: Write>(node: &EditNode, p: u64, dst: &mut W) -> anyhow::Result<u64> {
    let hlen = header_len(node, p);
    let total = hlen + p;
    let large = matches!(hlen - if node.uuid.is_some() { 16 } else { 0 }, 16);

    if node.header_form == HeaderForm::ToEof {
        dst.write_all(&0u32.to_be_bytes())?;
        dst.write_all(&node.typ.0)?;
    } else if large {
        dst.write_all(&1u32.to_be_bytes())?;
        dst.write_all(&node.typ.0)?;
        dst.write_all(&total.to_be_bytes())?;
    } else {
        dst.write_all(&(total as u32).to_be_bytes())?;
        dst.write_all(&node.typ.0)?;
    }
    if let Some(u) = &node.uuid {
        dst.write_all(u)?;
    }
    Ok(hlen)
}

fn write_node<R: Read + Seek, W: Write>(
    src: &mut R,
    node: &EditNode,
    dst: &mut W,
) -> anyhow::Result<u64> {
    let p = payload_len(node);
    let hlen = write_header(node, p, dst)?;

    match &node.payload {
        Payload::Extent(e) => copy_extent(src, e, dst)?,
        Payload::Bytes(b) => dst.write_all(b)?,
        Payload::Container {
            version_flags,
            prefix,
            children,
            suffix,
        } => {
            if let Some((version, flags)) = version_flags {
                dst.write_all(&[
                    *version,
                    ((flags >> 16) & 0xFF) as u8,
                    ((flags >> 8) & 0xFF) as u8,
                    (flags & 0xFF) as u8,
                ])?;
            }
            dst.write_all(prefix)?;
            for c in children {
                write_node(src, c, dst)?;
            }
            dst.write_all(suffix)?;
        }
    }

    Ok(hlen + p)
}

/// Copy `extent` from the source to the output in chunks (an mdat is never
/// held in memory).
fn copy_extent<R: Read + Seek, W: Write>(
    src: &mut R,
    extent: &Extent,
    dst: &mut W,
) -> anyhow::Result<()> {
    src.seek(SeekFrom::Start(extent.offset))?;
    let mut remaining = extent.len;
    let mut buf = vec![0u8; 1 << 20]; // 1 MiB chunks
    while remaining > 0 {
        let n = remaining.min(buf.len() as u64) as usize;
        src.read_exact(&mut buf[..n])?;
        dst.write_all(&buf[..n])?;
        remaining -= n as u64;
    }
    Ok(())
}
