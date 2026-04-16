use crate::{
    boxes::{BoxRef, NodeKind},
    edit::encoder::wrap_box_header,
    edit::{Command, EditingProcessor},
};

/// Edit a string iTunes metadata tag (`©nam`, `©ART`, `©alb`, etc.).
///
/// Looks for the tag box anywhere under `moov/udta/meta/ilst`.
/// If the box exists, it is replaced; if it does not exist it is appended.
///
/// The raw replacement bytes are built inline as a `data` atom carrying
/// a UTF-8 string value.
pub fn set_itunes_tag(
    processor: &mut EditingProcessor,
    boxes: &[BoxRef],
    tag: &str,
    value: &str,
) -> anyhow::Result<()> {
    let ilst_path = "moov/udta/meta/ilst";

    // Build raw `data` atom (type = 1, locale = 0, UTF-8 payload)
    let raw = build_itunes_text_box(tag, value);

    match find_box_path(boxes, &format!("{}/{}", ilst_path, tag)) {
        Some(_) => {
            processor.add_command(Command::Replace {
                box_path: format!("{}/{}", ilst_path, tag),
                file_path: "__inline__".to_string(), // overridden below
            });
            // Replace the last command with an inline version
            processor.add_inline_replace(format!("{}/{}", ilst_path, tag), raw);
        }
        None => {
            processor.add_inline_insert(ilst_path.to_string(), raw, None);
        }
    }

    Ok(())
}

/// Remove all boxes with the given 4CC anywhere in `boxes` (recursive).
pub fn strip_boxes(processor: &mut EditingProcessor, boxes: &[BoxRef], fourcc: &str) {
    let paths = collect_paths_by_fourcc(boxes, fourcc, "");
    for path in paths {
        processor.add_command(Command::Remove { box_path: path });
    }
}

/// Insert or replace a `udta` child box with raw bytes (header + payload).
pub fn upsert_udta_child(
    processor: &mut EditingProcessor,
    boxes: &[BoxRef],
    fourcc: &str,
    body: Vec<u8>,
) {
    let fourcc_bytes: [u8; 4] = fourcc.as_bytes().try_into().unwrap_or(*b"    ");
    let raw = wrap_box_header(&fourcc_bytes, &body);
    let path = format!("moov/udta/{}", fourcc);

    match find_box_path(boxes, &path) {
        Some(_) => {
            processor.add_inline_replace(path, raw);
        }
        None => {
            processor.add_inline_insert("moov/udta".to_string(), raw, None);
        }
    }
}

/// Rewrite `mvhd` `creation_time` and `modification_time`.
///
/// Uses the `--set` pathway so it goes through the encoder registry.
pub fn set_timestamps(
    processor: &mut EditingProcessor,
    _boxes: &[BoxRef],
    creation_time: u64,
    modification_time: u64,
) -> anyhow::Result<()> {
    processor.add_command(Command::Set {
        box_path: "moov/mvhd".to_string(),
        field: "creation_time".to_string(),
        value: creation_time.to_string(),
    });
    processor.add_command(Command::Set {
        box_path: "moov/mvhd".to_string(),
        field: "modification_time".to_string(),
        value: modification_time.to_string(),
    });
    Ok(())
}

// ---- Internal helpers ---------------------------------------------------

/// Build a complete iTunes text tag box: outer `tag` box containing a `data` atom.
fn build_itunes_text_box(tag: &str, value: &str) -> Vec<u8> {
    let tag_bytes: [u8; 4] = tag.as_bytes().try_into().unwrap_or(*b"    ");
    let utf8 = value.as_bytes();

    // data atom body: type_indicator(4) + locale(4) + utf8_bytes
    let mut data_body = Vec::with_capacity(8 + utf8.len());
    data_body.extend_from_slice(&1u32.to_be_bytes()); // type = 1 (UTF-8)
    data_body.extend_from_slice(&0u32.to_be_bytes()); // locale = 0
    data_body.extend_from_slice(utf8);

    let data_box = wrap_box_header(b"data", &data_body);
    wrap_box_header(&tag_bytes, &data_box)
}

/// Search `boxes` recursively for a box at `path`; return `Some(())` if found.
fn find_box_path<'a>(boxes: &'a [BoxRef], path: &str) -> Option<&'a BoxRef> {
    let (head, tail) = match path.split_once('/') {
        Some((h, t)) => (h, Some(t)),
        None => (path, None),
    };

    let found = boxes.iter().find(|b| b.hdr.typ.as_str_lossy() == head)?;

    match tail {
        None => Some(found),
        Some(rest) => {
            if let NodeKind::Container(children) = &found.kind {
                find_box_path(children, rest)
            } else {
                None
            }
        }
    }
}

/// Collect slash-delimited paths for every box with the given 4CC.
fn collect_paths_by_fourcc(boxes: &[BoxRef], fourcc: &str, prefix: &str) -> Vec<String> {
    let mut result = Vec::new();
    for b in boxes {
        let typ = b.hdr.typ.as_str_lossy();
        let path = if prefix.is_empty() {
            typ.clone()
        } else {
            format!("{}/{}", prefix, typ)
        };

        if typ == fourcc {
            result.push(path.clone());
        }

        if let NodeKind::Container(children) = &b.kind {
            let sub = collect_paths_by_fourcc(children, fourcc, &path);
            result.extend(sub);
        }
    }
    result
}
