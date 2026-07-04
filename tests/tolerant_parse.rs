//! Tests for tolerant parsing: malformed boxes produce a partial tree plus
//! located issues instead of aborting, and damage is contained to the
//! enclosing container.

use mp4box::{get_boxes, get_boxes_tolerant, parse_boxes_tolerant};
use std::io::Cursor;

fn plain_box(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(payload);
    v
}

fn full_box(typ: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(4 + payload.len());
    inner.push(version);
    inner.extend_from_slice(&flags.to_be_bytes()[1..4]);
    inner.extend_from_slice(payload);
    plain_box(typ, &inner)
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.iter().flatten().copied().collect()
}

fn mvhd() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 8]); // creation + modification
    p.extend_from_slice(&1000u32.to_be_bytes()); // timescale
    p.extend_from_slice(&5000u32.to_be_bytes()); // duration
    p.extend_from_slice(&[0u8; 80]);
    full_box(b"mvhd", 0, 0, &p)
}

fn parse_tolerant(data: &[u8]) -> (Vec<mp4box::Box>, Vec<mp4box::ParseIssue>) {
    let len = data.len() as u64;
    let mut cur = Cursor::new(data);
    get_boxes_tolerant(&mut cur, len, true).unwrap()
}

#[test]
fn clean_file_has_no_issues_and_matches_strict() {
    let data = concat(&[
        plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
        plain_box(b"moov", &mvhd()),
    ]);

    let (boxes, issues) = parse_tolerant(&data);
    assert!(issues.is_empty(), "clean file must produce no issues");

    let len = data.len() as u64;
    let mut cur = Cursor::new(&data);
    let strict = get_boxes(&mut cur, len, true).unwrap();
    assert_eq!(
        serde_json::to_string(&boxes).unwrap(),
        serde_json::to_string(&strict).unwrap(),
        "tolerant tree must equal strict tree on clean input"
    );
}

#[test]
fn truncated_file_returns_partial_tree() {
    let mut data = concat(&[
        plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
        plain_box(b"moov", &mvhd()),
    ]);
    // Truncate mid-way through mvhd's version/flags (ftyp is 20 bytes, moov
    // header ends at 28, mvhd header at 36): the mvhd interior becomes
    // unreadable and both moov and mvhd overrun the file.
    data.truncate(38);

    // Strict mode fails outright.
    let len = data.len() as u64;
    let mut cur = Cursor::new(&data);
    assert!(get_boxes(&mut cur, len, false).is_err());

    // Tolerant mode keeps ftyp and reports the damage with offsets.
    let (boxes, issues) = parse_tolerant(&data);
    assert_eq!(boxes[0].typ, "ftyp");
    assert!(!issues.is_empty());
    assert!(
        issues.iter().any(|i| i.message.contains("overruns")),
        "expected a clamp issue, got: {issues:?}"
    );
}

#[test]
fn bad_child_size_is_contained_to_its_container() {
    // moov contains: mvhd, a corrupt box (size=3, invalid), then udta.
    // trak-level damage must not take down the sibling top-level box.
    let corrupt = {
        let mut v = Vec::new();
        v.extend_from_slice(&3u32.to_be_bytes()); // size 3 < 8: invalid
        v.extend_from_slice(b"bad!");
        v.extend_from_slice(&[0u8; 8]);
        v
    };
    let moov_payload = concat(&[mvhd(), corrupt, plain_box(b"udta", &[])]);
    let data = concat(&[
        plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
        plain_box(b"moov", &moov_payload),
        plain_box(b"free", &[0u8; 8]),
    ]);

    let (boxes, issues) = parse_tolerant(&data);

    // Top level fully present: the damage stayed inside moov.
    let types: Vec<&str> = boxes.iter().map(|b| b.typ.as_str()).collect();
    assert_eq!(types, ["ftyp", "moov", "free"]);

    // moov kept the children before the corruption; the rest was abandoned.
    let moov_kids: Vec<&str> = boxes[1]
        .children
        .as_ref()
        .unwrap()
        .iter()
        .map(|b| b.typ.as_str())
        .collect();
    assert_eq!(moov_kids, ["mvhd"]);

    assert_eq!(issues.len(), 1, "issues: {issues:?}");
    assert!(issues[0].message.contains("unreadable box header"));
    // The reported offset points at the corrupt child:
    // ftyp (20 bytes) + moov header (8) + mvhd.
    let mvhd_len = mvhd().len() as u64;
    assert_eq!(issues[0].offset, 20 + 8 + mvhd_len);
}

#[test]
fn unparsable_fullbox_interior_degrades_to_leaf() {
    // A meta (FullBox container) too short to hold version/flags.
    let meta = plain_box(b"meta", &[0x00, 0x01]); // 2-byte payload
    let data = concat(&[plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"), meta]);

    let (boxes, issues) = parse_tolerant(&data);
    let types: Vec<&str> = boxes.iter().map(|b| b.typ.as_str()).collect();
    assert_eq!(types, ["ftyp", "meta"]);
    // Degraded to an opaque leaf rather than dropped.
    assert_eq!(boxes[1].kind, "leaf");
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("meta"), "issues: {issues:?}");
}

#[test]
fn oversized_child_is_clamped_and_reported() {
    // A child declaring a size that overruns its parent.
    let mut inner = Vec::new();
    inner.extend_from_slice(&999u32.to_be_bytes()); // way past parent end
    inner.extend_from_slice(b"cust");
    inner.extend_from_slice(&[0u8; 8]);
    let data = concat(&[
        plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
        plain_box(b"udta", &inner),
    ]);

    let (boxes, issues) = parse_tolerant(&data);
    let udta_kids = boxes[1].children.as_ref().unwrap();
    assert_eq!(udta_kids[0].typ, "cust");
    assert_eq!(issues.len(), 1);
    assert!(
        issues[0].message.contains("overruns its container"),
        "issues: {issues:?}"
    );
}

#[test]
fn parser_level_api_returns_boxrefs() {
    let data = concat(&[
        plain_box(b"ftyp", b"isom\x00\x00\x02\x00isom"),
        plain_box(b"free", &[0u8; 4]),
    ]);
    let len = data.len() as u64;
    let mut cur = Cursor::new(&data);
    let (boxes, issues) = parse_boxes_tolerant(&mut cur, 0, len).unwrap();
    assert_eq!(boxes.len(), 2);
    assert!(issues.is_empty());
}

#[test]
fn real_files_parse_without_issues() {
    for path in [
        "BigBuckBunny.mp4",
        "tears-of-steel-360p.mp4",
        "video_counter_10min_unfragmented_avc.mp4",
    ] {
        if !std::path::Path::new(path).exists() {
            eprintln!("skipping: {path} not present");
            continue;
        }
        let mut file = std::fs::File::open(path).unwrap();
        let size = file.metadata().unwrap().len();
        let (boxes, issues) = get_boxes_tolerant(&mut file, size, false).unwrap();
        assert!(!boxes.is_empty());
        assert!(issues.is_empty(), "{path}: unexpected issues {issues:?}");
    }
}
