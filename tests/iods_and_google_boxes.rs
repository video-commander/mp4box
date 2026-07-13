use mp4box::boxes::{BoxHeader, BoxKey, FourCC};
use mp4box::known_boxes::KnownBox;
use mp4box::registry::{BoxValue, StructuredData, default_registry};
use std::io::Cursor;

/// Build an `iods` payload (the bytes *after* the FullBox version/flags):
/// a descriptor `tag`, an expandable length, then `body`.
fn iod_payload(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag, body.len() as u8];
    v.extend_from_slice(body);
    v
}

fn decode_iods(payload: &[u8]) -> StructuredData {
    let reg = default_registry();
    let hdr = BoxHeader {
        start: 0,
        size: 8 + 4 + payload.len() as u64,
        header_size: 8,
        typ: FourCC(*b"iods"),
        uuid: None,
    };
    let mut cur = Cursor::new(payload.to_vec());
    let res = reg
        .decode(
            &BoxKey::FourCC(FourCC(*b"iods")),
            &mut cur,
            &hdr,
            Some(0),
            Some(0),
        )
        .expect("iods decoder registered")
        .expect("iods decodes");
    match res {
        BoxValue::Structured(d) => d,
        other => panic!("expected structured iods, got {:?}", other),
    }
}

#[test]
fn iods_profile_levels() {
    // od_id=1, url=0, includeInline=1, reserved=0xF -> 0x005F
    let body = [0x00, 0x5F, 0xFF, 0xFF, 0x02, 0x15, 0xFF];
    match decode_iods(&iod_payload(0x10, &body)) {
        StructuredData::ObjectDescriptor(d) => {
            assert_eq!(d.od_id, 1);
            assert!(d.include_inline_profiles);
            assert!(d.url.is_none());
            assert_eq!(d.audio_profile_level, 0x02);
            assert_eq!(d.visual_profile_level, 0x15);
            assert_eq!(d.graphics_profile_level, 0xFF);
        }
        other => panic!("expected ObjectDescriptor, got {:?}", other),
    }
}

#[test]
fn iods_inline_url() {
    let url = b"http://iod.example/scene";
    // od_id=5, url_flag=1 -> (5<<6)|(1<<5)|0xF = 0x016F
    let mut body = vec![0x01, 0x6F, url.len() as u8];
    body.extend_from_slice(url);
    match decode_iods(&iod_payload(0x02, &body)) {
        StructuredData::ObjectDescriptor(d) => {
            assert_eq!(d.od_id, 5);
            assert_eq!(d.url.as_deref(), Some("http://iod.example/scene"));
        }
        other => panic!("expected ObjectDescriptor, got {:?}", other),
    }
}

#[test]
fn google_boxes_are_recognized() {
    for (cc, name) in [
        (b"gsst", "Google Start Time Box"),
        (b"gstd", "Google Track Duration Box"),
        (b"gssd", "Google Source Data Box"),
        (b"gspu", "Google Ping URL Box"),
        (b"gspm", "Google Ping Message Box"),
        (b"gshh", "Google Host Header Box"),
    ] {
        let kb = KnownBox::from(FourCC(*cc));
        assert!(
            !matches!(kb, KnownBox::Unknown(_)),
            "{} should be recognized",
            std::str::from_utf8(cc).unwrap()
        );
        assert_eq!(kb.full_name(), name);
    }
    assert_eq!(
        KnownBox::from(FourCC(*b"iods")).full_name(),
        "Object Descriptor Box"
    );
}
