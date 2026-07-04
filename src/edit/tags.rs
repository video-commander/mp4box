//! Builders for iTunes metadata boxes and the `SetTag` command's
//! find-or-create logic for the `moov/udta/meta/ilst` chain.

use super::tree::{EditNode, HeaderForm, Payload};
use crate::boxes::FourCC;

/// Map a friendly tag name (as returned by `get_itunes_tags`) to its
/// iTunes atom fourcc. Raw 4-byte names (including `©xxx`) pass through.
pub fn tag_fourcc(name: &str) -> anyhow::Result<[u8; 4]> {
    let cc: &[u8] = match name {
        "title" => b"\xa9nam",
        "artist" => b"\xa9ART",
        "album" => b"\xa9alb",
        "year" => b"\xa9day",
        "genre" => b"\xa9gen",
        "comment" => b"\xa9cmt",
        "description" => b"desc",
        "copyright" => b"cprt",
        "album_artist" => b"aART",
        "encoder" => b"\xa9too",
        "composer" => b"\xa9wrt",
        "lyrics" => b"\xa9lyr",
        "grouping" => b"\xa9grp",
        other => {
            // Accept a raw fourcc; '©' is 2 bytes in UTF-8, so re-encode it
            // as the single 0xA9 byte iTunes uses.
            let mut bytes = Vec::with_capacity(4);
            for ch in other.chars() {
                if ch == '©' {
                    bytes.push(0xA9);
                } else {
                    anyhow::ensure!(
                        ch.is_ascii(),
                        "tag {:?} is not a known name or fourcc",
                        other
                    );
                    bytes.push(ch as u8);
                }
            }
            anyhow::ensure!(
                bytes.len() == 4,
                "tag {:?} is not a known name or 4-character code",
                other
            );
            return Ok(bytes.try_into().unwrap());
        }
    };
    Ok(cc.try_into().unwrap())
}

fn plain_box(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(payload);
    v
}

/// Raw tag atom: `<tag>` containing a `data` atom with a UTF-8 value.
pub fn build_tag_atom(fourcc: &[u8; 4], value: &str) -> Vec<u8> {
    let mut data_payload = Vec::with_capacity(8 + value.len());
    data_payload.extend_from_slice(&1u32.to_be_bytes()); // type indicator: UTF-8
    data_payload.extend_from_slice(&0u32.to_be_bytes()); // locale
    data_payload.extend_from_slice(value.as_bytes());
    plain_box(fourcc, &plain_box(b"data", &data_payload))
}

/// A `hdlr` box marking an ISO `meta` as iTunes metadata (matches what
/// ffmpeg writes: empty name, single NUL).
fn build_mdir_hdlr() -> Vec<u8> {
    let mut p = Vec::with_capacity(25);
    p.extend_from_slice(&[0, 0, 0, 0]); // version/flags
    p.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    p.extend_from_slice(b"mdir");
    p.extend_from_slice(&[0u8; 12]); // reserved
    p.push(0); // empty name
    plain_box(b"hdlr", &p)
}

fn empty_container(typ: &[u8; 4], version_flags: Option<(u8, u32)>) -> EditNode {
    EditNode {
        typ: FourCC(*typ),
        uuid: None,
        header_form: HeaderForm::Compact,
        payload: Payload::Container {
            version_flags,
            prefix: Vec::new(),
            children: Vec::new(),
            suffix: Vec::new(),
        },
    }
}

/// Find or create `udta/meta/ilst` under the given `moov` node and set the
/// tag atom (replace if present, append otherwise).
pub fn set_tag_in_moov(moov: &mut EditNode, fourcc: &[u8; 4], value: &str) -> anyhow::Result<()> {
    let moov_children = moov
        .children_mut()
        .ok_or_else(|| anyhow::anyhow!("moov is not a container"))?;

    // udta
    if !moov_children.iter().any(|c| &c.typ.0 == b"udta") {
        moov_children.push(empty_container(b"udta", None));
    }
    let udta = moov_children
        .iter_mut()
        .find(|c| &c.typ.0 == b"udta")
        .unwrap();
    let udta_children = udta
        .children_mut()
        .ok_or_else(|| anyhow::anyhow!("udta is not a container"))?;

    // meta (ISO FullBox flavor, with the iTunes hdlr)
    if !udta_children.iter().any(|c| &c.typ.0 == b"meta") {
        let mut meta = empty_container(b"meta", Some((0, 0)));
        if let Some(kids) = meta.children_mut() {
            kids.push(EditNode::from_raw(&build_mdir_hdlr())?);
        }
        udta_children.push(meta);
    }
    let meta = udta_children
        .iter_mut()
        .find(|c| &c.typ.0 == b"meta")
        .unwrap();
    let meta_children = meta
        .children_mut()
        .ok_or_else(|| anyhow::anyhow!("meta is not a container"))?;

    // ilst
    if !meta_children.iter().any(|c| &c.typ.0 == b"ilst") {
        meta_children.push(empty_container(b"ilst", None));
    }
    let ilst = meta_children
        .iter_mut()
        .find(|c| &c.typ.0 == b"ilst")
        .unwrap();
    let ilst_children = ilst
        .children_mut()
        .ok_or_else(|| anyhow::anyhow!("ilst is not a container"))?;

    // the tag atom itself
    let atom = EditNode::from_raw(&build_tag_atom(fourcc, value))?;
    match ilst_children.iter_mut().find(|c| &c.typ.0 == fourcc) {
        Some(existing) => *existing = atom,
        None => ilst_children.push(atom),
    }
    Ok(())
}
