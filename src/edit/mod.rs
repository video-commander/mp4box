pub mod encoder;
pub mod fixup;
pub mod helpers;

use crate::{
    boxes::{BoxKey, BoxRef, NodeKind},
    known_boxes::KnownBox,
    parser::{parse_children, read_box_header},
    registry::{BoxValue, default_registry},
};
use byteorder::ReadBytesExt;
use encoder::{default_encoder_registry, wrap_box_header, wrap_full_box_header};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

// ---- Public types --------------------------------------------------------

/// A single edit operation, addressed by slash-delimited 4CC path.
pub enum Command {
    Remove {
        box_path: String,
    },
    /// Insert a box from a file on disk.
    Insert {
        /// Parent path to insert into, e.g. `"moov/udta"`
        box_path: String,
        /// Raw box file (header + payload bytes)
        file_path: String,
        /// Child position: `None` = append, `Some(n)` = before the nth child
        position: Option<usize>,
    },
    /// Insert pre-built raw bytes (no file I/O needed).
    InsertInline {
        box_path: String,
        bytes: Vec<u8>,
        position: Option<usize>,
    },
    /// Replace a box with the contents of a file on disk.
    Replace {
        box_path: String,
        /// Raw box file (header + payload bytes)
        file_path: String,
    },
    /// Replace a box with pre-built raw bytes (no file I/O needed).
    ReplaceInline {
        box_path: String,
        bytes: Vec<u8>,
    },
    Set {
        box_path: String,
        field: String,
        value: String,
    },
}

/// Internal splice — every `Command` resolves to one of these.
enum Splice {
    Delete {
        offset: u64,
        size: u64,
    },
    Insert {
        offset: u64,
        new_bytes: Vec<u8>,
    },
    Replace {
        offset: u64,
        size: u64,
        new_bytes: Vec<u8>,
    },
}

/// Drives one or more edit commands against an MP4 source.
#[derive(Default)]
pub struct EditingProcessor {
    commands: Vec<Command>,
}

impl EditingProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_command(&mut self, cmd: Command) -> &mut Self {
        self.commands.push(cmd);
        self
    }

    /// Insert pre-built raw bytes as a child of `parent_path`.
    pub fn add_inline_insert(
        &mut self,
        parent_path: String,
        bytes: Vec<u8>,
        position: Option<usize>,
    ) -> &mut Self {
        self.commands.push(Command::InsertInline {
            box_path: parent_path,
            bytes,
            position,
        });
        self
    }

    /// Replace the box at `box_path` with pre-built raw bytes.
    pub fn add_inline_replace(&mut self, box_path: String, bytes: Vec<u8>) -> &mut Self {
        self.commands
            .push(Command::ReplaceInline { box_path, bytes });
        self
    }

    /// Parse `src`, apply all registered commands, write the result to `dst`.
    ///
    /// Commands are batched: only one splice pass and one fixup pass are made
    /// regardless of how many commands were added.
    pub fn process<R, W>(&self, src: &mut R, dst: &mut W) -> anyhow::Result<EditStats>
    where
        R: Read + Seek,
        W: Write,
    {
        // 1. Read the entire source into memory.
        let mut buf = Vec::new();
        src.seek(SeekFrom::Start(0))?;
        src.read_to_end(&mut buf)?;

        // 2. Parse the box tree from the buffer.
        let file_len = buf.len() as u64;
        let mut cursor = Cursor::new(&buf);
        let boxes = parse_box_tree(&mut cursor, file_len)?;

        // 3. Resolve each command to a Splice.
        let mut splices: Vec<Splice> = Vec::new();
        let decoder_reg = default_registry();
        let encoder_reg = default_encoder_registry();

        for cmd in &self.commands {
            match cmd {
                Command::Remove { box_path } => {
                    let b = find_box(&boxes, box_path)?;
                    splices.push(Splice::Delete {
                        offset: b.hdr.start,
                        size: b.hdr.size,
                    });
                }

                Command::Insert {
                    box_path,
                    file_path,
                    position,
                } => {
                    let parent = find_box(&boxes, box_path)?;
                    let offset = resolve_insert_offset(parent, *position);
                    let new_bytes = std::fs::read(file_path)?;
                    splices.push(Splice::Insert { offset, new_bytes });
                }

                Command::InsertInline {
                    box_path,
                    bytes,
                    position,
                } => {
                    let parent = find_box(&boxes, box_path)?;
                    let offset = resolve_insert_offset(parent, *position);
                    splices.push(Splice::Insert {
                        offset,
                        new_bytes: bytes.clone(),
                    });
                }

                Command::Replace {
                    box_path,
                    file_path,
                } => {
                    let b = find_box(&boxes, box_path)?;
                    let new_bytes = std::fs::read(file_path)?;
                    splices.push(Splice::Replace {
                        offset: b.hdr.start,
                        size: b.hdr.size,
                        new_bytes,
                    });
                }

                Command::ReplaceInline { box_path, bytes } => {
                    let b = find_box(&boxes, box_path)?;
                    splices.push(Splice::Replace {
                        offset: b.hdr.start,
                        size: b.hdr.size,
                        new_bytes: bytes.clone(),
                    });
                }

                Command::Set {
                    box_path,
                    field,
                    value,
                } => {
                    let b = find_box(&boxes, box_path)?;
                    let key = BoxKey::FourCC(b.hdr.typ);

                    // Read box body from buffer
                    let (body_offset, body_len, is_full, version, flags) = box_body_region(b);
                    let body_bytes = &buf[body_offset as usize..(body_offset + body_len) as usize];
                    let mut body_cursor = Cursor::new(body_bytes);

                    // Decode current box value (we only call decode on the data region
                    // *after* version/flags — the decoder already had those stripped).
                    let current_value = decoder_reg
                        .decode(
                            &key,
                            &mut body_cursor,
                            &b.hdr,
                            if is_full { Some(version) } else { None },
                            if is_full { Some(flags) } else { None },
                        )
                        .ok_or_else(|| anyhow::anyhow!("no decoder for box '{}'", b.hdr.typ))??;

                    // Mutate the value with the requested field=value assignment.
                    let mutated = mutate_box_value(current_value, field, value)?;

                    // Encode back to body bytes.
                    let new_body = encoder_reg
                        .encode(&key, &mutated)
                        .ok_or_else(|| anyhow::anyhow!("no encoder for box '{}'", b.hdr.typ))??;

                    // Wrap with box header.
                    let fourcc = b.hdr.typ.0;
                    let new_bytes = if is_full {
                        wrap_full_box_header(&fourcc, version, flags, &new_body)
                    } else {
                        wrap_box_header(&fourcc, &new_body)
                    };

                    splices.push(Splice::Replace {
                        offset: b.hdr.start,
                        size: b.hdr.size,
                        new_bytes,
                    });
                }
            }
        }

        // 4. Sort splices descending by offset so back-to-front application
        //    keeps earlier offsets stable.
        splices.sort_by_key(|s| std::cmp::Reverse(splice_offset(s)));

        // 5. Apply splices to the buffer.
        let mut out = buf;
        for splice in splices {
            apply_splice(&mut out, splice)?;
        }

        // 6. Fixup chunk offsets.
        let stats = fixup::adjust_chunk_offsets(&mut out)?;

        // 7. Write output.
        dst.write_all(&out)?;
        Ok(stats)
    }
}

// ---- Box tree parsing ---------------------------------------------------

/// Parse the complete box tree from a cursor over a memory buffer.
fn parse_box_tree<R: Read + Seek>(r: &mut R, file_len: u64) -> anyhow::Result<Vec<BoxRef>> {
    let mut boxes = Vec::new();
    while r.stream_position()? < file_len {
        let h = read_box_header(r).map_err(|e| anyhow::anyhow!("{}", e))?;
        let box_end = if h.size == 0 {
            file_len
        } else {
            h.start + h.size
        };

        let kind = if KnownBox::from(h.typ).is_container() {
            r.seek(SeekFrom::Start(h.start + h.header_size))?;
            let children = parse_children(r, box_end).map_err(|e| anyhow::anyhow!("{}", e))?;
            NodeKind::Container(children)
        } else if KnownBox::from(h.typ).is_full_box() {
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
            NodeKind::Leaf {
                data_offset,
                data_len,
            }
        };

        r.seek(SeekFrom::Start(box_end))?;
        boxes.push(BoxRef { hdr: h, kind });
    }
    Ok(boxes)
}

// ---- Box path resolution ------------------------------------------------

/// Resolve a slash-delimited 4CC path like `"moov/udta/©nam"` against
/// the parsed box tree.
pub fn find_box<'a>(boxes: &'a [BoxRef], path: &str) -> anyhow::Result<&'a BoxRef> {
    let (head, tail) = match path.split_once('/') {
        Some((h, t)) => (h, Some(t)),
        None => (path, None),
    };

    let found = boxes
        .iter()
        .find(|b| b.hdr.typ.as_str_lossy() == head)
        .ok_or_else(|| anyhow::anyhow!("box '{}' not found", head))?;

    match tail {
        None => Ok(found),
        Some(rest) => {
            let children = match &found.kind {
                NodeKind::Container(ch) => ch.as_slice(),
                _ => anyhow::bail!("'{}' has no children", head),
            };
            find_box(children, rest)
        }
    }
}

/// Resolve the byte offset at which to insert a new child into `parent`.
fn resolve_insert_offset(parent: &BoxRef, position: Option<usize>) -> u64 {
    match &parent.kind {
        NodeKind::Container(children) => match position {
            None => {
                // Append: just before the end of the parent box
                parent.hdr.start + parent.hdr.size
            }
            Some(0) => {
                // Prepend: right after the parent header
                parent.hdr.start + parent.hdr.header_size
            }
            Some(n) => {
                let child = children.get(n).or_else(|| children.last());
                match child {
                    Some(c) => c.hdr.start,
                    None => parent.hdr.start + parent.hdr.header_size,
                }
            }
        },
        _ => parent.hdr.start + parent.hdr.size,
    }
}

// ---- Box body helpers ---------------------------------------------------

/// Return `(data_offset, data_len, is_full_box, version, flags)`.
fn box_body_region(b: &BoxRef) -> (u64, u64, bool, u8, u32) {
    match &b.kind {
        NodeKind::FullBox {
            version,
            flags,
            data_offset,
            data_len,
        } => (*data_offset, *data_len, true, *version, *flags),
        NodeKind::Leaf {
            data_offset,
            data_len,
        } => (*data_offset, *data_len, false, 0, 0),
        NodeKind::Unknown {
            data_offset,
            data_len,
        } => (*data_offset, *data_len, false, 0, 0),
        NodeKind::Container(_) => {
            let off = b.hdr.start + b.hdr.header_size;
            let len = b.hdr.size.saturating_sub(b.hdr.header_size);
            (off, len, false, 0, 0)
        }
    }
}

// ---- Field mutation -----------------------------------------------------

/// Apply a field=value mutation to a decoded `BoxValue`.
///
/// For `BoxValue::Text`, fields are represented as space-separated `key=val`
/// tokens; we replace the matching token in-place (or append if absent).
fn mutate_box_value(current: BoxValue, field: &str, value: &str) -> anyhow::Result<BoxValue> {
    match current {
        BoxValue::Text(text) => {
            let prefix = format!("{}=", field);
            let mut parts: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();

            if let Some(pos) = parts.iter().position(|p| p.starts_with(&prefix)) {
                parts[pos] = format!("{}={}", field, value);
            } else {
                parts.push(format!("{}={}", field, value));
            }

            Ok(BoxValue::Text(parts.join(" ")))
        }
        BoxValue::Structured(s) => {
            // For structured types we currently only support text fall-through.
            // Encode the structured data as text first, then mutate.
            let text = format!("{:?}", s);
            mutate_box_value(BoxValue::Text(text), field, value)
        }
        other => Err(anyhow::anyhow!(
            "cannot apply --set on BoxValue variant {:?}",
            std::mem::discriminant(&other)
        )),
    }
}

// ---- Splice application -------------------------------------------------

fn splice_offset(s: &Splice) -> u64 {
    match s {
        Splice::Delete { offset, .. } => *offset,
        Splice::Insert { offset, .. } => *offset,
        Splice::Replace { offset, .. } => *offset,
    }
}

fn apply_splice(buf: &mut Vec<u8>, splice: Splice) -> anyhow::Result<()> {
    match splice {
        Splice::Delete { offset, size } => {
            let start = offset as usize;
            let end = (offset + size) as usize;
            if end > buf.len() {
                anyhow::bail!(
                    "Delete splice out of bounds: offset={} size={} buf_len={}",
                    offset,
                    size,
                    buf.len()
                );
            }
            buf.drain(start..end);
        }
        Splice::Insert { offset, new_bytes } => {
            let pos = offset as usize;
            if pos > buf.len() {
                anyhow::bail!(
                    "Insert splice out of bounds: offset={} buf_len={}",
                    offset,
                    buf.len()
                );
            }
            buf.splice(pos..pos, new_bytes);
        }
        Splice::Replace {
            offset,
            size,
            new_bytes,
        } => {
            let start = offset as usize;
            let end = (offset + size) as usize;
            if end > buf.len() {
                anyhow::bail!(
                    "Replace splice out of bounds: offset={} size={} buf_len={}",
                    offset,
                    size,
                    buf.len()
                );
            }
            buf.splice(start..end, new_bytes);
        }
    }
    Ok(())
}

// ---- Re-export EditStats for public callers ------------------------------
pub use fixup::EditStats;
