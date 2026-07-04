use mp4box::boxes::{BoxHeader, BoxKey, FourCC};
use mp4box::registry::{BoxDecoder, BoxValue, Registry};
use std::io::Read;

struct DummyDecoder;

impl BoxDecoder for DummyDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        Ok(BoxValue::Bytes(buf))
    }
}

#[test]
fn registry_invokes_decoder() {
    let reg = Registry::new().with_decoder(
        BoxKey::FourCC(FourCC(*b"test")),
        "test",
        Box::new(DummyDecoder),
    );

    let hdr = BoxHeader {
        start: 0,
        size: 12,
        header_size: 8,
        typ: FourCC(*b"test"),
        uuid: None,
    };

    let payload = &[1u8, 2, 3, 4];
    let mut cursor = std::io::Cursor::new(payload.to_vec());

    let res = reg.decode(
        &BoxKey::FourCC(FourCC(*b"test")),
        &mut cursor,
        &hdr,
        None,
        None,
    );
    assert!(res.is_some());

    match res.unwrap().unwrap() {
        BoxValue::Bytes(b) => assert_eq!(b, payload),
        _ => panic!("expected bytes"),
    }
}

/// A custom decoder added on top of the default registry must fire through
/// the high-level get_boxes_with_registry API, alongside the built-ins.
#[test]
fn custom_decoder_extends_default_registry_in_get_boxes() {
    use mp4box::get_boxes_with_registry;
    use mp4box::registry::default_registry;
    use std::io::Cursor;

    struct XyzDecoder;
    impl BoxDecoder for XyzDecoder {
        fn decode(
            &self,
            r: &mut dyn Read,
            _hdr: &BoxHeader,
            _version: Option<u8>,
            _flags: Option<u32>,
        ) -> anyhow::Result<BoxValue> {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf)?;
            Ok(BoxValue::Text(format!("xyz payload={} bytes", buf.len())))
        }
    }

    let reg = default_registry().with_decoder(
        BoxKey::FourCC(FourCC(*b"xyz ")),
        "xyz ",
        Box::new(XyzDecoder),
    );

    // ftyp (decoded by the default registry) followed by a custom "xyz " box.
    let mut data = Vec::new();
    data.extend_from_slice(&16u32.to_be_bytes());
    data.extend_from_slice(b"ftypisom");
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&12u32.to_be_bytes());
    data.extend_from_slice(b"xyz ....");

    let len = data.len() as u64;
    let mut cur = Cursor::new(data);
    let boxes = get_boxes_with_registry(&mut cur, len, true, &reg).unwrap();

    let ftyp = boxes.iter().find(|b| b.typ == "ftyp").unwrap();
    assert!(
        ftyp.decoded.as_deref().unwrap_or("").contains("major=isom"),
        "built-in ftyp decoder must still fire"
    );
    let xyz = boxes.iter().find(|b| b.typ == "xyz ").unwrap();
    assert_eq!(xyz.decoded.as_deref(), Some("xyz payload=4 bytes"));
}
