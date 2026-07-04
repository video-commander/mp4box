//! Non-destructive MP4/ISOBMFF box editing.
//!
//! Editing works on a tree where unmodified boxes are *references* into the
//! source file, so serialization streams untouched bytes through verbatim
//! (an `mdat` is never loaded into memory) and re-serializing an unedited
//! tree reproduces the source byte for byte. Box sizes are recomputed
//! bottom-up during layout, so every ancestor of an edited box is correct by
//! construction, and `stco`/`co64` chunk offsets are remapped through the
//! exact old-offset → new-offset extent map — data that didn't move keeps
//! its offsets, data that moved shifts by exactly the right amount.
//!
//! ```no_run
//! use mp4box::edit::{Command, Editor};
//! use std::fs::File;
//!
//! let mut editor = Editor::new();
//! editor.add_command(Command::Remove { path: "moov/udta".into() });
//! editor.set_tag("title", "My Movie")?;
//!
//! let mut src = File::open("in.mp4")?;
//! let mut dst = File::create("out.mp4")?;
//! let stats = editor.process(&mut src, &mut dst)?;
//! println!("wrote {} bytes, {} chunk offsets adjusted",
//!     stats.bytes_written, stats.chunk_offsets_adjusted);
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! Fragmented (`moof`/`sidx`) and HEIF (`iloc`) files are refused: their
//! internal offsets are not covered by the fixup pass yet, and editing them
//! would corrupt the output.

mod fields;
mod fixup;
mod tags;
mod tree;

pub use fixup::FixupStats;
pub use tree::{EditNode, EditTree, HeaderForm, Payload};

use crate::boxes::FourCC;
use crate::parser::parse_boxes;
use std::io::{Read, Seek, SeekFrom, Write};

/// A single edit operation. Paths are slash-delimited fourcc segments with
/// optional indices for repeated boxes: `"moov/trak[1]/mdia/mdhd"`.
/// An omitted index means the first match.
pub enum Command {
    /// Delete the box at `path`.
    Remove { path: String },
    /// Delete every box with this fourcc, anywhere in the tree.
    RemoveAll { fourcc: String },
    /// Insert a complete raw box (header + payload) as a child of `parent`.
    /// `position: None` appends; `Some(n)` inserts before the nth child.
    Insert {
        parent: String,
        bytes: Vec<u8>,
        position: Option<usize>,
    },
    /// Replace the box at `path` with a complete raw box.
    Replace { path: String, bytes: Vec<u8> },
    /// Set a named field of a known box in place (mvhd/tkhd/mdhd), e.g.
    /// `path: "moov/mvhd", field: "creation_time", value: "0"`. All other
    /// bytes of the box are preserved.
    Set {
        path: String,
        field: String,
        value: String,
    },
    /// Set an iTunes metadata tag, creating `moov/udta/meta/ilst` as needed.
    /// `tag` is a friendly name (`"title"`, `"artist"`, ...) or a raw fourcc.
    SetTag { tag: String, value: String },
}

/// Statistics from a completed edit.
#[derive(Debug, Default)]
pub struct EditStats {
    pub bytes_written: u64,
    pub chunk_offsets_adjusted: usize,
    /// Chunk offsets pointing at data that no longer exists (left unchanged).
    pub chunk_offsets_unmapped: usize,
}

/// Applies a batch of [`Command`]s to an MP4 source and writes the result.
#[derive(Default)]
pub struct Editor {
    commands: Vec<Command>,
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_command(&mut self, cmd: Command) -> &mut Self {
        self.commands.push(cmd);
        self
    }

    /// Convenience for [`Command::Remove`].
    pub fn remove(&mut self, path: impl Into<String>) -> &mut Self {
        self.add_command(Command::Remove { path: path.into() })
    }

    /// Convenience for [`Command::RemoveAll`].
    pub fn remove_all(&mut self, fourcc: impl Into<String>) -> &mut Self {
        self.add_command(Command::RemoveAll {
            fourcc: fourcc.into(),
        })
    }

    /// Convenience for [`Command::Set`].
    pub fn set_field(
        &mut self,
        path: impl Into<String>,
        field: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        self.add_command(Command::Set {
            path: path.into(),
            field: field.into(),
            value: value.into(),
        })
    }

    /// Convenience for [`Command::SetTag`]. Validates the tag name eagerly.
    pub fn set_tag(
        &mut self,
        tag: impl Into<String>,
        value: impl Into<String>,
    ) -> anyhow::Result<&mut Self> {
        let tag = tag.into();
        tags::tag_fourcc(&tag)?; // fail fast on unknown names
        self.add_command(Command::SetTag {
            tag,
            value: value.into(),
        });
        Ok(self)
    }

    /// Parse `src`, apply all commands in order, and write the edited file
    /// to `dst`. `src` is only read; the output is always a new file.
    pub fn process<R: Read + Seek, W: Write>(
        &self,
        src: &mut R,
        dst: &mut W,
    ) -> anyhow::Result<EditStats> {
        let file_len = src.seek(SeekFrom::End(0))?;
        let boxes = parse_boxes(src, 0, file_len)?;
        let mut edit_tree = tree::build_tree(src, &boxes, file_len)?;

        if !self.commands.is_empty() {
            guard_unsupported(&edit_tree)?;
        }

        for cmd in &self.commands {
            apply_command(src, &mut edit_tree, cmd)?;
        }

        // Layout: compute where every unmodified extent lands in the output.
        let map = tree::layout(&edit_tree);

        // Remap chunk offsets when any data moved.
        let moved = map.iter().any(|m| m.new_offset != m.old_offset);
        let fixup_stats = if moved {
            fixup::fix_chunk_offsets(src, &mut edit_tree, &map)?
        } else {
            FixupStats::default()
        };

        let bytes_written = tree::write_tree(src, &edit_tree, dst)?;

        Ok(EditStats {
            bytes_written,
            chunk_offsets_adjusted: fixup_stats.entries_adjusted,
            chunk_offsets_unmapped: fixup_stats.entries_unmapped,
        })
    }

    /// Convenience wrapper over [`Editor::process`] for file paths.
    /// Refuses `output == input`; the source is never modified.
    pub fn process_file(
        &self,
        input: impl AsRef<std::path::Path>,
        output: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<EditStats> {
        let input = input.as_ref();
        let output = output.as_ref();
        anyhow::ensure!(
            input != output,
            "in-place editing is not supported; choose a different output path"
        );
        let mut src = std::fs::File::open(input)?;
        let mut dst = std::io::BufWriter::new(std::fs::File::create(output)?);
        let stats = self.process(&mut src, &mut dst)?;
        std::io::Write::flush(&mut dst)?;
        Ok(stats)
    }
}

/// Refuse file kinds whose internal offsets the fixup pass does not cover.
fn guard_unsupported(tree: &EditTree) -> anyhow::Result<()> {
    fn scan(nodes: &[EditNode]) -> Option<&'static str> {
        for n in nodes {
            match &n.typ.0 {
                b"moof" => return Some("fragmented MP4 (moof)"),
                b"sidx" => return Some("indexed segments (sidx)"),
                b"iloc" => return Some("HEIF item locations (iloc)"),
                _ => {}
            }
            if let Some(kids) = n.children()
                && let Some(hit) = scan(kids)
            {
                return Some(hit);
            }
        }
        None
    }
    if let Some(kind) = scan(&tree.roots) {
        anyhow::bail!(
            "editing is not supported for {} yet: byte offsets inside these \
             structures are not fixed up, and editing would corrupt the file",
            kind
        );
    }
    Ok(())
}

// ---------- command application ----------

fn apply_command<R: Read + Seek>(
    src: &mut R,
    tree: &mut EditTree,
    cmd: &Command,
) -> anyhow::Result<()> {
    match cmd {
        Command::Remove { path } => {
            let (siblings, idx) = resolve_parent_mut(&mut tree.roots, path)?;
            siblings.remove(idx);
        }

        Command::RemoveAll { fourcc } => {
            let cc = seg_fourcc(fourcc)?;
            remove_all(&mut tree.roots, &cc);
        }

        Command::Insert {
            parent,
            bytes,
            position,
        } => {
            let node = EditNode::from_raw(bytes)?;
            let target = resolve_node_mut(&mut tree.roots, parent)?;
            let children = target
                .children_mut()
                .ok_or_else(|| anyhow::anyhow!("'{}' is not a container", parent))?;
            let at = position.unwrap_or(children.len()).min(children.len());
            children.insert(at, node);
        }

        Command::Replace { path, bytes } => {
            let node = EditNode::from_raw(bytes)?;
            let (siblings, idx) = resolve_parent_mut(&mut tree.roots, path)?;
            siblings[idx] = node;
        }

        Command::Set { path, field, value } => {
            let node = resolve_node_mut(&mut tree.roots, path)?;
            set_field_on_node(src, node, field, value)?;
        }

        Command::SetTag { tag, value } => {
            let cc = tags::tag_fourcc(tag)?;
            let moov = tree
                .roots
                .iter_mut()
                .find(|n| &n.typ.0 == b"moov")
                .ok_or_else(|| anyhow::anyhow!("no moov box: cannot set tags"))?;
            tags::set_tag_in_moov(moov, &cc, value)?;
        }
    }
    Ok(())
}

fn set_field_on_node<R: Read + Seek>(
    src: &mut R,
    node: &mut EditNode,
    field: &str,
    value: &str,
) -> anyhow::Result<()> {
    // Materialize the payload (version/flags + body) so it can be patched.
    let mut payload = match &node.payload {
        Payload::Bytes(b) => b.clone(),
        Payload::Extent(e) => {
            let mut buf = vec![0u8; e.len as usize];
            src.seek(SeekFrom::Start(e.offset))?;
            src.read_exact(&mut buf)?;
            buf
        }
        Payload::Container { .. } => {
            anyhow::bail!("'{}' is a container; --set applies to leaf boxes", node.typ)
        }
    };
    anyhow::ensure!(!payload.is_empty(), "'{}' has an empty payload", node.typ);

    let version = payload[0];
    let (offset, kind) = fields::field_spec(&node.typ.0, version, field).ok_or_else(|| {
        anyhow::anyhow!(
            "no known field '{}' in '{}' (version {})",
            field,
            node.typ,
            version
        )
    })?;
    fields::patch_field(&mut payload, offset, kind, value)?;
    node.payload = Payload::Bytes(payload);
    Ok(())
}

fn remove_all(nodes: &mut Vec<EditNode>, cc: &FourCC) {
    nodes.retain(|n| &n.typ != cc);
    for n in nodes {
        if let Some(kids) = n.children_mut() {
            remove_all(kids, cc);
        }
    }
}

// ---------- path resolution ----------

/// Parse one path segment: `"trak[1]"` → (`trak`, 1); `"trak"` → (`trak`, 0).
fn parse_segment(seg: &str) -> anyhow::Result<(FourCC, usize)> {
    if let Some(open) = seg.find('[') {
        let close = seg
            .rfind(']')
            .ok_or_else(|| anyhow::anyhow!("unclosed '[' in path segment '{}'", seg))?;
        let idx: usize = seg[open + 1..close]
            .parse()
            .map_err(|_| anyhow::anyhow!("bad index in path segment '{}'", seg))?;
        Ok((seg_fourcc(&seg[..open])?, idx))
    } else {
        Ok((seg_fourcc(seg)?, 0))
    }
}

/// Convert a path segment to a fourcc; '©' (2 bytes in UTF-8) maps to the
/// single 0xA9 byte used by iTunes atoms.
fn seg_fourcc(seg: &str) -> anyhow::Result<FourCC> {
    let mut bytes = Vec::with_capacity(4);
    for ch in seg.chars() {
        if ch == '©' {
            bytes.push(0xA9);
        } else {
            anyhow::ensure!(ch.is_ascii(), "invalid character in fourcc '{}'", seg);
            bytes.push(ch as u8);
        }
    }
    anyhow::ensure!(bytes.len() == 4, "'{}' is not a 4-character box type", seg);
    Ok(FourCC(bytes.try_into().unwrap()))
}

fn find_child_idx(nodes: &[EditNode], cc: &FourCC, nth: usize) -> Option<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| &n.typ == cc)
        .map(|(i, _)| i)
        .nth(nth)
}

/// Resolve `path` to a mutable node reference.
fn resolve_node_mut<'a>(
    roots: &'a mut Vec<EditNode>,
    path: &str,
) -> anyhow::Result<&'a mut EditNode> {
    let (siblings, idx) = resolve_parent_mut(roots, path)?;
    Ok(&mut siblings[idx])
}

/// Resolve `path` to its parent's child list and the index within it, so the
/// caller can remove or replace the node.
fn resolve_parent_mut<'a>(
    roots: &'a mut Vec<EditNode>,
    path: &str,
) -> anyhow::Result<(&'a mut Vec<EditNode>, usize)> {
    anyhow::ensure!(!path.is_empty(), "empty box path");
    let segments: Vec<&str> = path.split('/').collect();

    let mut current: &'a mut Vec<EditNode> = roots;
    for (depth, seg) in segments.iter().enumerate() {
        let (cc, nth) = parse_segment(seg)?;
        let idx = find_child_idx(current, &cc, nth)
            .ok_or_else(|| anyhow::anyhow!("box '{}' not found (in path '{}')", seg, path))?;

        if depth == segments.len() - 1 {
            return Ok((current, idx));
        }

        current = current[idx]
            .children_mut()
            .ok_or_else(|| anyhow::anyhow!("'{}' has no children (in path '{}')", seg, path))?;
    }
    unreachable!("loop always returns on the last segment");
}
