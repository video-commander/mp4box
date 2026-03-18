use byteorder::{BigEndian, ByteOrder};

/// Result returned after a fixup pass.
#[derive(Debug, Default)]
pub struct EditStats {
    /// Net byte-count change applied to chunk offsets (may be 0).
    pub offset_delta: i64,
}

// ---- Public entry point -------------------------------------------------

/// Scan `buf` for all `stco` and `co64` boxes and adjust their chunk-offset
/// entries by `delta` bytes.
///
/// `delta` is computed by comparing the current location of the `mdat` box in
/// the (already-spliced) output buffer against the minimum `stco` entry, which
/// should equal the original `mdat` offset.
///
/// If no `mdat` is found, or if `delta` is zero, the buffer is left unchanged.
pub fn adjust_chunk_offsets(buf: &mut [u8]) -> anyhow::Result<EditStats> {
    let mdat_offset = match find_fourcc_offset(buf, b"mdat") {
        Some(off) => off,
        None => return Ok(EditStats::default()),
    };

    // The minimum stco entry should be the original mdat offset.
    let old_mdat_offset = match min_stco_entry(buf) {
        Some(v) => v,
        None => return Ok(EditStats::default()), // no stco/co64 found
    };

    let delta = mdat_offset as i64 - old_mdat_offset as i64;
    if delta == 0 {
        return Ok(EditStats { offset_delta: 0 });
    }

    rewrite_stco_entries(buf, delta)?;
    rewrite_co64_entries(buf, delta)?;

    Ok(EditStats {
        offset_delta: delta,
    })
}

// ---- Internal helpers ---------------------------------------------------

/// Find the file offset of the first box with the given four-byte type code.
/// The offset returned is the start of the box header (i.e. the size field).
fn find_fourcc_offset(buf: &[u8], fourcc: &[u8; 4]) -> Option<u64> {
    if buf.len() < 8 {
        return None;
    }

    let mut pos = 0usize;
    while pos + 8 <= buf.len() {
        let size32 = BigEndian::read_u32(&buf[pos..pos + 4]);
        let typ = &buf[pos + 4..pos + 8];

        let box_size = if size32 == 1 {
            if pos + 16 > buf.len() {
                break;
            }
            BigEndian::read_u64(&buf[pos + 8..pos + 16])
        } else if size32 == 0 {
            (buf.len() - pos) as u64
        } else {
            size32 as u64
        };

        if box_size < 8 {
            break;
        }

        if typ == fourcc.as_ref() {
            return Some(pos as u64);
        }

        let next = pos + box_size as usize;
        if next <= pos {
            break;
        }
        pos = next;
    }
    None
}

/// Walk all `stco` boxes in `buf` and return the minimum chunk-offset entry.
fn min_stco_entry(buf: &[u8]) -> Option<u64> {
    let mut min: Option<u64> = None;
    walk_boxes(buf, &mut |typ, body| {
        if typ == b"stco" && body.len() >= 8 {
            // FullBox: version(1) + flags(3) = 4 bytes already consumed by caller;
            // but here the body slice includes the full FullBox payload starting
            // at version byte for stco (because we receive raw box body after hdr).
            // stco body: version(1)+flags(3)+entry_count(4)+entries(4*n)
            if body.len() < 8 {
                return;
            }
            let entry_count = BigEndian::read_u32(&body[4..8]);
            let entries_start = 8usize;
            for i in 0..entry_count as usize {
                let off = entries_start + i * 4;
                if off + 4 <= body.len() {
                    let v = BigEndian::read_u32(&body[off..off + 4]) as u64;
                    min = Some(min.map_or(v, |m: u64| m.min(v)));
                }
            }
        }
        if typ == b"co64" && body.len() >= 8 {
            // co64 body: version(1)+flags(3)+entry_count(4)+entries(8*n)
            let entry_count = BigEndian::read_u32(&body[4..8]);
            let entries_start = 8usize;
            for i in 0..entry_count as usize {
                let off = entries_start + i * 8;
                if off + 8 <= body.len() {
                    let v = BigEndian::read_u64(&body[off..off + 8]);
                    min = Some(min.map_or(v, |m: u64| m.min(v)));
                }
            }
        }
    });
    min
}

/// Rewrite all `stco` entries in `buf` by adding `delta` to each one.
fn rewrite_stco_entries(buf: &mut [u8], delta: i64) -> anyhow::Result<()> {
    let patches = collect_stco_patches(buf);
    for byte_offset in patches {
        let old = BigEndian::read_u32(&buf[byte_offset..byte_offset + 4]) as i64;
        let new = (old + delta).max(0) as u32;
        BigEndian::write_u32(&mut buf[byte_offset..byte_offset + 4], new);
    }
    Ok(())
}

/// Rewrite all `co64` entries in `buf` by adding `delta` to each one.
fn rewrite_co64_entries(buf: &mut [u8], delta: i64) -> anyhow::Result<()> {
    let patches = collect_co64_patches(buf);
    for byte_offset in patches {
        let old = BigEndian::read_u64(&buf[byte_offset..byte_offset + 8]) as i64;
        let new = (old + delta).max(0) as u64;
        BigEndian::write_u64(&mut buf[byte_offset..byte_offset + 8], new);
    }
    Ok(())
}

/// Return the byte offsets (in `buf`) of each stco entry value.
fn collect_stco_patches(buf: &[u8]) -> Vec<usize> {
    let mut patches = Vec::new();
    walk_boxes_with_offset(buf, &mut |typ, body_start, body| {
        if typ == b"stco" && body.len() >= 8 {
            let entry_count = BigEndian::read_u32(&body[4..8]) as usize;
            let base = body_start + 8;
            for i in 0..entry_count {
                let off = base + i * 4;
                if off + 4 <= body_start + body.len() {
                    patches.push(off);
                }
            }
        }
    });
    patches
}

/// Return the byte offsets (in `buf`) of each co64 entry value.
fn collect_co64_patches(buf: &[u8]) -> Vec<usize> {
    let mut patches = Vec::new();
    walk_boxes_with_offset(buf, &mut |typ, body_start, body| {
        if typ == b"co64" && body.len() >= 8 {
            let entry_count = BigEndian::read_u32(&body[4..8]) as usize;
            let base = body_start + 8;
            for i in 0..entry_count {
                let off = base + i * 8;
                if off + 8 <= body_start + body.len() {
                    patches.push(off);
                }
            }
        }
    });
    patches
}

// ---- Generic box walkers ------------------------------------------------

/// Walk every box in `buf` recursively, calling `f(fourcc, body_slice)` for each.
fn walk_boxes<F: FnMut(&[u8], &[u8])>(buf: &[u8], f: &mut F) {
    walk_range(buf, 0, buf.len(), f);
}

fn walk_range<F: FnMut(&[u8], &[u8])>(buf: &[u8], start: usize, end: usize, f: &mut F) {
    let mut pos = start;
    while pos + 8 <= end {
        let size32 = BigEndian::read_u32(&buf[pos..pos + 4]);
        let typ = &buf[pos + 4..pos + 8];

        let (header_size, box_size) = if size32 == 1 {
            if pos + 16 > end {
                break;
            }
            let s = BigEndian::read_u64(&buf[pos + 8..pos + 16]);
            (16usize, s as usize)
        } else if size32 == 0 {
            (8usize, end - pos)
        } else {
            (8usize, size32 as usize)
        };

        if box_size < header_size {
            break;
        }

        let box_end = pos + box_size;
        if box_end > end {
            break;
        }

        let body = &buf[pos + header_size..box_end];
        f(typ, body);

        // Recurse into known containers
        if is_container_fourcc(typ) {
            walk_range(buf, pos + header_size, box_end, f);
        }

        pos = box_end;
    }
}

/// Like `walk_boxes` but also supplies the absolute byte offset of the body start.
fn walk_boxes_with_offset<F: FnMut(&[u8], usize, &[u8])>(buf: &[u8], f: &mut F) {
    walk_range_with_offset(buf, 0, buf.len(), f);
}

fn walk_range_with_offset<F: FnMut(&[u8], usize, &[u8])>(
    buf: &[u8],
    start: usize,
    end: usize,
    f: &mut F,
) {
    let mut pos = start;
    while pos + 8 <= end {
        let size32 = BigEndian::read_u32(&buf[pos..pos + 4]);
        let typ = &buf[pos + 4..pos + 8];

        let (header_size, box_size) = if size32 == 1 {
            if pos + 16 > end {
                break;
            }
            let s = BigEndian::read_u64(&buf[pos + 8..pos + 16]);
            (16usize, s as usize)
        } else if size32 == 0 {
            (8usize, end - pos)
        } else {
            (8usize, size32 as usize)
        };

        if box_size < header_size {
            break;
        }

        let box_end = pos + box_size;
        if box_end > end {
            break;
        }

        let body_start = pos + header_size;
        let body = &buf[body_start..box_end];
        f(typ, body_start, body);

        if is_container_fourcc(typ) {
            walk_range_with_offset(buf, body_start, box_end, f);
        }

        pos = box_end;
    }
}

/// Quick check for the container types we need to recurse into.
fn is_container_fourcc(typ: &[u8]) -> bool {
    matches!(
        typ,
        b"moov"
            | b"trak"
            | b"mdia"
            | b"minf"
            | b"stbl"
            | b"udta"
            | b"edts"
            | b"dinf"
            | b"ilst"
            | b"meta"
            | b"moof"
            | b"traf"
            | b"mvex"
    )
}
