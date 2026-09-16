//! Self-contained ProRes sample-entry and QuickTime color-box regressions.

use mp4box::known_boxes::KnownBox;
use mp4box::registry::{ColrData, StructuredData};
use mp4box::{Box, FourCC, get_boxes};
use std::io::Cursor;

const PRORES: [[u8; 4]; 6] = [*b"apch", *b"apcn", *b"apcs", *b"apco", *b"ap4h", *b"ap4x"];

fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut data = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    data.extend_from_slice(typ);
    data.extend_from_slice(payload);
    data
}

fn color_payload(typ: &[u8; 4], codes: [u16; 3]) -> Vec<u8> {
    let mut data = typ.to_vec();
    for code in codes {
        data.extend_from_slice(&code.to_be_bytes());
    }
    data
}

fn visual_payload() -> Vec<u8> {
    let mut data = vec![0; 78];
    data[6..8].copy_from_slice(&1u16.to_be_bytes());
    data[24..26].copy_from_slice(&1920u16.to_be_bytes());
    data[26..28].copy_from_slice(&1080u16.to_be_bytes());
    data
}

fn stsd(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut data = vec![0; 4]; // version and flags
    data.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for entry in entries {
        data.extend_from_slice(entry);
    }
    boxed(b"stsd", &data)
}

fn parse(data: &[u8], decode: bool) -> Vec<Box> {
    get_boxes(&mut Cursor::new(data), data.len() as u64, decode).expect("parse failed")
}

fn color(b: &Box) -> &ColrData {
    let Some(StructuredData::ColourInformation(data)) = &b.structured_data else {
        panic!("expected structured color data for {}", b.typ);
    };
    data
}

#[test]
fn prores_fourccs_have_known_names() {
    let expected = [
        (KnownBox::Apch, "Apple ProRes 422 HQ Sample Entry"),
        (KnownBox::Apcn, "Apple ProRes 422 Sample Entry"),
        (KnownBox::Apcs, "Apple ProRes 422 LT Sample Entry"),
        (KnownBox::Apco, "Apple ProRes 422 Proxy Sample Entry"),
        (KnownBox::Ap4h, "Apple ProRes 4444 Sample Entry"),
        (KnownBox::Ap4x, "Apple ProRes 4444 XQ Sample Entry"),
    ];
    for (codec, (known, name)) in PRORES.iter().zip(expected) {
        assert_eq!(KnownBox::from(FourCC(*codec)), known);
        assert_eq!(known.full_name(), name);
        assert!(!known.is_full_box());
    }
}

#[test]
fn all_prores_entries_expose_dimensions_and_color_children() {
    let entries: Vec<_> = PRORES
        .iter()
        .map(|codec| {
            let mut payload = visual_payload();
            payload.extend_from_slice(&boxed(b"colr", &color_payload(b"nclc", [1, 1, 1])));
            payload.extend_from_slice(&boxed(b"fiel", &[2, 1]));
            boxed(codec, &payload)
        })
        .collect();
    let boxes = parse(&stsd(&entries), true);
    let entries = boxes[0].children.as_ref().unwrap();
    assert_eq!(entries.len(), PRORES.len());
    for (entry, codec) in entries.iter().zip(PRORES) {
        assert_eq!(entry.typ.as_bytes(), codec);
        assert_eq!(entry.kind, "container");
        let children = entry.children.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].typ, "colr");
        assert_eq!(children[1].typ, "fiel");
        assert_eq!(color(&children[0]).primaries, Some(1));
    }
    let Some(StructuredData::SampleDescription(data)) = &boxes[0].structured_data else {
        panic!("expected structured sample description");
    };
    for entry in &data.entries {
        assert_eq!(entry.data_reference_index, 1);
        assert_eq!(entry.width, Some(1920));
        assert_eq!(entry.height, Some(1080));
        assert_eq!(entry.channel_count, None);
    }
}

#[test]
fn prores_child_tree_is_available_without_decoding() {
    let mut payload = visual_payload();
    payload.extend_from_slice(&boxed(b"colr", &color_payload(b"nclc", [1, 1, 1])));
    let boxes = parse(&stsd(&[boxed(b"apch", &payload)]), false);
    let entry = &boxes[0].children.as_ref().unwrap()[0];
    assert_eq!(entry.kind, "container");
    assert!(boxes[0].structured_data.is_none());
    let child = &entry.children.as_ref().unwrap()[0];
    assert_eq!(child.typ, "colr");
    assert!(child.structured_data.is_none());
}

#[test]
fn prores_without_child_boxes_still_decodes_dimensions() {
    let boxes = parse(&stsd(&[boxed(b"apch", &visual_payload())]), true);
    assert!(boxes[0].children.as_ref().unwrap()[0].children.is_none());
    let Some(StructuredData::SampleDescription(data)) = &boxes[0].structured_data else {
        panic!("expected structured sample description");
    };
    assert_eq!(data.entries[0].width, Some(1920));
    assert_eq!(data.entries[0].height, Some(1080));
}

#[test]
fn truncated_prores_fixed_fields_do_not_become_child_boxes() {
    let payload = visual_payload();
    for codec in PRORES {
        for len in 0..78 {
            let boxes = parse(&stsd(&[boxed(&codec, &payload[..len])]), true);
            assert!(boxes[0].children.as_ref().unwrap()[0].children.is_none());
        }
    }
}

#[test]
fn short_prores_entry_does_not_read_dimensions_from_its_sibling() {
    let payload = visual_payload();
    let boxes = parse(
        &stsd(&[boxed(b"apch", &payload[..8]), boxed(b"apcn", &payload)]),
        true,
    );
    let Some(StructuredData::SampleDescription(data)) = &boxes[0].structured_data else {
        panic!("expected structured sample description");
    };
    assert_eq!(data.entries.len(), 2);
    assert_eq!(data.entries[0].width, None);
    assert_eq!(data.entries[0].height, None);
    assert_eq!(data.entries[1].width, Some(1920));
    assert_eq!(data.entries[1].height, Some(1080));
}

#[test]
fn unknown_sample_entries_remain_opaque() {
    let mut payload = visual_payload();
    payload.extend_from_slice(&boxed(b"colr", &color_payload(b"nclc", [1, 1, 1])));
    let boxes = parse(&stsd(&[boxed(b"zzzz", &payload)]), true);
    let entry = &boxes[0].children.as_ref().unwrap()[0];
    assert!(entry.children.is_none());
    let Some(StructuredData::SampleDescription(data)) = &boxes[0].structured_data else {
        panic!("expected structured sample description");
    };
    assert_eq!(data.entries[0].width, None);
}

#[test]
fn nclc_decodes_bt709_without_a_range_flag() {
    let boxes = parse(&boxed(b"colr", &color_payload(b"nclc", [1, 1, 1])), true);
    let data = color(&boxes[0]);
    assert_eq!(data.colour_type, "nclc");
    assert_eq!(data.primaries, Some(1));
    assert_eq!(data.transfer, Some(1));
    assert_eq!(data.matrix, Some(1));
    assert_eq!(data.primaries_name.as_deref(), Some("BT.709"));
    assert_eq!(data.transfer_name.as_deref(), Some("BT.709"));
    assert_eq!(data.matrix_name.as_deref(), Some("BT.709"));
    assert_eq!(data.full_range, None);
    assert_eq!(
        boxes[0].decoded.as_deref(),
        Some("type=nclc primaries=1 (BT.709) transfer=1 (BT.709) matrix=1 (BT.709)")
    );
    let json = serde_json::to_value(data).unwrap();
    assert_eq!(json["primaries"], 1);
    assert!(json.get("full_range").is_none());
}

#[test]
fn nclc_preserves_unspecified_and_unrecognized_codes() {
    let boxes = parse(
        &boxed(b"colr", &color_payload(b"nclc", [2, u16::MAX, 0])),
        true,
    );
    let data = color(&boxes[0]);
    assert_eq!(data.primaries, Some(2));
    assert_eq!(data.primaries_name.as_deref(), Some("unspecified"));
    assert_eq!(data.transfer, Some(u16::MAX));
    assert_eq!(data.transfer_name, None);
    assert_eq!(data.matrix, Some(0));
    assert_eq!(data.full_range, None);
}

#[test]
fn nclc_trailing_bytes_do_not_declare_range() {
    let mut payload = color_payload(b"nclc", [9, 16, 9]);
    payload.push(0x80);
    let boxes = parse(&boxed(b"colr", &payload), true);
    assert_eq!(
        color(&boxes[0]).transfer_name.as_deref(),
        Some("PQ / SMPTE ST 2084")
    );
    assert_eq!(color(&boxes[0]).full_range, None);
    assert!(!boxes[0].decoded.as_deref().unwrap().contains("full_range"));
}

#[test]
fn nclx_preserves_range_and_decoded_text() {
    for (flag, full_range) in [(0, false), (0x80, true)] {
        let mut payload = color_payload(b"nclx", [9, 16, 9]);
        payload.push(flag);
        let boxes = parse(&boxed(b"colr", &payload), true);
        assert_eq!(color(&boxes[0]).full_range, Some(full_range));
        assert_eq!(
            boxes[0].decoded.as_deref().unwrap(),
            format!(
                "type=nclx primaries=9 (BT.2020) transfer=16 (PQ / SMPTE ST 2084) matrix=9 (BT.2020 non-constant luminance) full_range={}",
                u8::from(full_range)
            )
        );
    }
}

#[test]
fn truncated_color_payloads_do_not_read_into_siblings() {
    for typ in [b"nclc", b"nclx"] {
        let mut payload = color_payload(typ, [1, 1, 1]);
        if typ == b"nclx" {
            payload.push(0x80);
        }
        for len in 0..payload.len() {
            let mut data = boxed(b"colr", &payload[..len]);
            data.extend_from_slice(&boxed(b"free", &[0xff; 32]));
            let boxes = parse(&data, true);
            assert_eq!(boxes.len(), 2);
            if len >= 4 {
                let data = color(&boxes[0]);
                assert_eq!(data.primaries, None);
                assert_eq!(data.transfer, None);
                assert_eq!(data.matrix, None);
                assert_eq!(data.full_range, None);
            } else {
                assert!(boxes[0].structured_data.is_none());
            }
        }
    }
}

#[test]
fn icc_color_types_remain_type_only() {
    for typ in [b"prof", b"rICC"] {
        let boxes = parse(&boxed(b"colr", &color_payload(typ, [1, 1, 1])), true);
        let data = color(&boxes[0]);
        assert_eq!(data.primaries, None);
        assert_eq!(data.transfer, None);
        assert_eq!(data.matrix, None);
        assert_eq!(data.full_range, None);
        assert_eq!(
            boxes[0].decoded.as_deref().unwrap(),
            format!("type={}", String::from_utf8_lossy(typ))
        );
    }
}
