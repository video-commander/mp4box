//! Chunk-offset fixup: after layout, every `stco`/`co64` entry is remapped
//! through the extent map — the exact record of where each source byte range
//! lands in the output. No heuristics: offsets into data that didn't move are
//! unchanged, offsets into moved data shift by exactly that extent's delta.

use super::tree::{EditNode, EditTree, Extent, ExtentMapping, Payload};
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Default)]
pub struct FixupStats {
    /// Chunk-offset entries rewritten to a new value.
    pub entries_adjusted: usize,
    /// Entries pointing at bytes that no longer exist in the output
    /// (e.g. data inside a removed box); left unchanged.
    pub entries_unmapped: usize,
}

/// Remap all stco/co64 entries in the tree. Nodes whose entries change are
/// converted from extents to in-memory bytes; payload sizes are unchanged,
/// so the layout (and the extent map itself) stays valid.
pub fn fix_chunk_offsets<R: Read + Seek>(
    src: &mut R,
    tree: &mut EditTree,
    map: &[ExtentMapping],
) -> anyhow::Result<FixupStats> {
    // Sort by old offset for binary search. Extents are disjoint.
    let mut sorted: Vec<ExtentMapping> = map.to_vec();
    sorted.sort_by_key(|m| m.old_offset);

    let mut stats = FixupStats::default();
    for node in &mut tree.roots {
        fix_node(src, node, &sorted, &mut stats)?;
    }
    Ok(stats)
}

fn fix_node<R: Read + Seek>(
    src: &mut R,
    node: &mut EditNode,
    map: &[ExtentMapping],
    stats: &mut FixupStats,
) -> anyhow::Result<()> {
    let is_stco = &node.typ.0 == b"stco";
    let is_co64 = &node.typ.0 == b"co64";

    if is_stco || is_co64 {
        // Only extent payloads need attention: Bytes payloads were produced
        // by this edit session and carry no stale offsets... unless a caller
        // replaced them, in which case they're responsible for the values.
        if let Payload::Extent(e) = &node.payload {
            let bytes = read_extent(src, e)?;
            if let Some(patched) = remap_entries(bytes, is_co64, map, stats) {
                node.payload = Payload::Bytes(patched);
            }
        }
        return Ok(());
    }

    if let Payload::Container { children, .. } = &mut node.payload {
        for c in children {
            fix_node(src, c, map, stats)?;
        }
    }
    Ok(())
}

fn read_extent<R: Read + Seek>(src: &mut R, e: &Extent) -> anyhow::Result<Vec<u8>> {
    let mut buf = vec![0u8; e.len as usize];
    src.seek(SeekFrom::Start(e.offset))?;
    src.read_exact(&mut buf)?;
    Ok(buf)
}

/// Payload layout (after the box header): version(1) flags(3) entry_count(4)
/// then 4-byte (stco) or 8-byte (co64) entries. Returns the patched payload,
/// or `None` when nothing changed.
fn remap_entries(
    mut payload: Vec<u8>,
    is_co64: bool,
    map: &[ExtentMapping],
    stats: &mut FixupStats,
) -> Option<Vec<u8>> {
    if payload.len() < 8 {
        return None;
    }
    let entry_count = u32::from_be_bytes(payload[4..8].try_into().unwrap()) as usize;
    let width = if is_co64 { 8 } else { 4 };

    let mut changed = false;
    for i in 0..entry_count {
        let at = 8 + i * width;
        if at + width > payload.len() {
            break;
        }
        let old = if is_co64 {
            u64::from_be_bytes(payload[at..at + 8].try_into().unwrap())
        } else {
            u32::from_be_bytes(payload[at..at + 4].try_into().unwrap()) as u64
        };

        let Some(new) = remap_offset(old, map) else {
            stats.entries_unmapped += 1;
            continue;
        };
        if new == old {
            continue;
        }

        if is_co64 {
            payload[at..at + 8].copy_from_slice(&new.to_be_bytes());
        } else {
            let Ok(new32) = u32::try_from(new) else {
                stats.entries_unmapped += 1;
                continue; // would need stco→co64 conversion; out of scope
            };
            payload[at..at + 4].copy_from_slice(&new32.to_be_bytes());
        }
        stats.entries_adjusted += 1;
        changed = true;
    }

    changed.then_some(payload)
}

/// Map a source-file offset to its output offset via the extent that
/// contains it.
fn remap_offset(old: u64, sorted: &[ExtentMapping]) -> Option<u64> {
    // Last extent whose old_offset <= old
    let idx = sorted.partition_point(|m| m.old_offset <= old);
    let m = sorted.get(idx.checked_sub(1)?)?;
    if old < m.old_offset + m.len {
        Some(m.new_offset + (old - m.old_offset))
    } else {
        None
    }
}
