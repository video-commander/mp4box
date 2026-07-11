//! Tests for system-specific pssh payload decoding (crate::drm): the
//! Widevine protobuf, PlayReady Objects, raw pssh box parsing, and their
//! integration with the pssh box decoder.
//!
//! All fixtures are built synthetically so the tests are self-contained.

use mp4box::drm::{
    PLAYREADY_SYSTEM_ID, WIDEVINE_SYSTEM_ID, parse_playready_pssh_data, parse_pssh_boxes,
    parse_widevine_pssh_data, pssh_from_raw_widevine,
};
use mp4box::registry::StructuredData;
use mp4box::util::base64_decode;
use mp4box::{Box, get_boxes};
use std::io::Cursor;

// ---------- fixture builders ----------

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

fn parse(data: &[u8]) -> Vec<Box> {
    let len = data.len() as u64;
    let mut cur = Cursor::new(data);
    get_boxes(&mut cur, len, true).expect("parse failed")
}

/// Protobuf helpers for building Widevine payloads.
fn proto_varint(field: u64, value: u64) -> Vec<u8> {
    let mut v = vec![(field << 3) as u8];
    let mut value = value;
    loop {
        if value < 0x80 {
            v.push(value as u8);
            break;
        }
        v.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    v
}

fn proto_bytes(field: u64, data: &[u8]) -> Vec<u8> {
    let mut v = vec![((field << 3) | 2) as u8, data.len() as u8];
    v.extend_from_slice(data);
    v
}

/// A representative Widevine payload: algorithm, one KID, provider,
/// content ID, and protection scheme.
fn widevine_payload(kid: &[u8; 16]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(proto_varint(1, 1)); // algorithm = AESCTR
    p.extend(proto_bytes(2, kid));
    p.extend(proto_bytes(3, b"widevine_test"));
    p.extend(proto_bytes(4, b"content-1"));
    p.extend(proto_varint(9, u64::from(u32::from_be_bytes(*b"cbcs"))));
    p
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// PlayReady Object with a single type-1 (WRMHEADER) record.
fn playready_object(xml: &str) -> Vec<u8> {
    let record = utf16le(xml);
    let mut p = Vec::new();
    p.extend_from_slice(&((10 + record.len()) as u32).to_le_bytes());
    p.extend_from_slice(&1u16.to_le_bytes()); // record count
    p.extend_from_slice(&1u16.to_le_bytes()); // record type
    p.extend_from_slice(&(record.len() as u16).to_le_bytes());
    p.extend_from_slice(&record);
    p
}

fn pssh_box(system_id: &[u8; 16], version: u8, kids: &[[u8; 16]], data: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(system_id);
    if version >= 1 {
        p.extend_from_slice(&(kids.len() as u32).to_be_bytes());
        for kid in kids {
            p.extend_from_slice(kid);
        }
    }
    p.extend_from_slice(&(data.len() as u32).to_be_bytes());
    p.extend_from_slice(data);
    full_box(b"pssh", version, 0, &p)
}

// KID whose PlayReady base64 form is known: 16 bytes 00..0f in CENC order.
// The GUID little-endian layout reorders the first 8 bytes.
const CENC_KID: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

fn playready_kid_b64() -> String {
    // CENC 00010203-0405-0607 → LE GUID 03020100-0504-0706, tail unchanged.
    let le: [u8; 16] = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    base64_encode(&le)
}

/// Test-only standard base64 encoder (the crate only needs decode).
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ---------- base64 ----------

#[test]
fn base64_decode_roundtrip_and_variants() {
    let data: Vec<u8> = (0u8..=255).collect();
    assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
    // Unpadded and URL-safe forms.
    assert_eq!(base64_decode("aGk").unwrap(), b"hi");
    assert_eq!(base64_decode("_-8").unwrap(), vec![0xFF, 0xEF]);
    // Rejects: bad chars, data after padding, impossible length.
    assert!(base64_decode("a GG").is_none());
    assert!(base64_decode("aa=a").is_none());
    assert!(base64_decode("aaaaa").is_none());
}

// ---------- Widevine ----------

#[test]
fn widevine_payload_decodes_all_fields() {
    let kid = [0xAB; 16];
    let wv = parse_widevine_pssh_data(&widevine_payload(&kid)).expect("widevine parse");
    assert_eq!(wv.algorithm.as_deref(), Some("AESCTR"));
    assert_eq!(wv.key_ids, vec!["ab".repeat(16)]);
    assert_eq!(wv.provider.as_deref(), Some("widevine_test"));
    assert_eq!(
        wv.content_id.as_deref(),
        Some(hex::encode(b"content-1").as_str())
    );
    assert_eq!(wv.content_id_text.as_deref(), Some("content-1"));
    assert_eq!(wv.protection_scheme.as_deref(), Some("cbcs"));
}

#[test]
fn widevine_payload_skips_unknown_fields() {
    let mut p = widevine_payload(&[1; 16]);
    p.extend(proto_bytes(8, b"grouped-license-blob")); // known-unknown field
    p.extend(proto_varint(15, 7)); // arbitrary varint field
    let wv = parse_widevine_pssh_data(&p).expect("widevine parse");
    assert_eq!(wv.provider.as_deref(), Some("widevine_test"));
}

#[test]
fn widevine_rejects_garbage() {
    assert!(parse_widevine_pssh_data(&[]).is_none());
    assert!(parse_widevine_pssh_data(&[0xFF; 24]).is_none());
    // A PlayReady Object must not parse as Widevine.
    let pro = playready_object("<WRMHEADER></WRMHEADER>");
    assert!(parse_widevine_pssh_data(&pro).is_none());
}

// Minimal local hex encoder so the test doesn't depend on the hex crate.
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ---------- PlayReady ----------

fn wrmheader_v40(kid_b64: &str) -> String {
    format!(
        r#"<WRMHEADER xmlns="http://schemas.microsoft.com/DRM/2007/03/PlayReadyHeader" version="4.0.0.0"><DATA><PROTECTINFO><KEYLEN>16</KEYLEN><ALGID>AESCTR</ALGID></PROTECTINFO><KID>{kid_b64}</KID><LA_URL>https://example.com/rightsmanager.asmx?a=1&amp;b=2</LA_URL></DATA></WRMHEADER>"#
    )
}

fn wrmheader_v42(kid_b64: &str) -> String {
    format!(
        r#"<WRMHEADER xmlns="http://schemas.microsoft.com/DRM/2007/03/PlayReadyHeader" version="4.2.0.0"><DATA><PROTECTINFO><KIDS><KID ALGID="AESCTR" VALUE="{kid_b64}"></KID></KIDS></PROTECTINFO></DATA></WRMHEADER>"#
    )
}

#[test]
fn playready_v40_header_decodes() {
    let pro = playready_object(&wrmheader_v40(&playready_kid_b64()));
    let pr = parse_playready_pssh_data(&pro).expect("playready parse");
    assert_eq!(pr.record_count, 1);
    assert_eq!(pr.wrm_header_version.as_deref(), Some("4.0.0.0"));
    assert_eq!(pr.key_ids, vec![hex::encode(&CENC_KID)]);
    assert_eq!(
        pr.la_url.as_deref(),
        Some("https://example.com/rightsmanager.asmx?a=1&b=2")
    );
    assert!(pr.xml.as_deref().unwrap().starts_with("<WRMHEADER"));
}

#[test]
fn playready_v42_kids_attribute_form_decodes() {
    let pro = playready_object(&wrmheader_v42(&playready_kid_b64()));
    let pr = parse_playready_pssh_data(&pro).expect("playready parse");
    assert_eq!(pr.wrm_header_version.as_deref(), Some("4.2.0.0"));
    assert_eq!(pr.key_ids, vec![hex::encode(&CENC_KID)]);
    assert_eq!(pr.la_url, None);
}

#[test]
fn playready_rejects_garbage() {
    assert!(parse_playready_pssh_data(&[]).is_none());
    assert!(parse_playready_pssh_data(&[0xFF; 32]).is_none());
    assert!(parse_playready_pssh_data(&widevine_payload(&[1; 16])).is_none());
}

// ---------- raw pssh box parsing ----------

#[test]
fn parse_pssh_boxes_decodes_concatenated_systems() {
    let kid = [0x11; 16];
    let mut data = pssh_box(&WIDEVINE_SYSTEM_ID, 0, &[], &widevine_payload(&kid));
    data.extend(pssh_box(
        &PLAYREADY_SYSTEM_ID,
        1,
        &[CENC_KID],
        &playready_object(&wrmheader_v40(&playready_kid_b64())),
    ));

    let boxes = parse_pssh_boxes(&data).expect("parse pssh boxes");
    assert_eq!(boxes.len(), 2);

    assert_eq!(boxes[0].system_name.as_deref(), Some("Widevine"));
    let wv = boxes[0].widevine.as_ref().expect("widevine decoded");
    assert_eq!(wv.key_ids, vec!["11".repeat(16)]);
    assert!(boxes[0].playready.is_none());

    assert_eq!(boxes[1].system_name.as_deref(), Some("PlayReady"));
    assert_eq!(boxes[1].key_ids, vec![hex::encode(&CENC_KID)]);
    let pr = boxes[1].playready.as_ref().expect("playready decoded");
    // The v1 box KIDs and the WRMHEADER KIDs must agree after the GUID
    // byte-order conversion.
    assert_eq!(pr.key_ids, boxes[1].key_ids);
}

#[test]
fn parse_pssh_boxes_rejects_non_pssh() {
    assert!(parse_pssh_boxes(b"").is_err());
    assert!(parse_pssh_boxes(&plain_box(b"ftyp", b"isom")).is_err());
    // Truncated box: declared size larger than the buffer.
    let mut data = pssh_box(&WIDEVINE_SYSTEM_ID, 0, &[], &widevine_payload(&[1; 16]));
    data.truncate(data.len() - 4);
    assert!(parse_pssh_boxes(&data).is_err());
}

#[test]
fn pssh_from_raw_widevine_wraps_payload() {
    let payload = widevine_payload(&[0x22; 16]);
    let pssh = pssh_from_raw_widevine(&payload).expect("raw widevine");
    assert_eq!(pssh.system_name.as_deref(), Some("Widevine"));
    assert_eq!(pssh.data_size as usize, payload.len());
    assert_eq!(
        pssh.widevine.as_ref().unwrap().key_ids,
        vec!["22".repeat(16)]
    );
    assert!(pssh_from_raw_widevine(&[0xFF; 8]).is_none());
}

// ---------- integration with the box decoder ----------

#[test]
fn pssh_box_decoder_populates_widevine_payload() {
    let data = pssh_box(&WIDEVINE_SYSTEM_ID, 0, &[], &widevine_payload(&[0x33; 16]));
    let boxes = parse(&data);
    let Some(StructuredData::ProtectionSystemHeader(d)) = &boxes[0].structured_data else {
        panic!("expected structured pssh");
    };
    assert_eq!(d.system_name.as_deref(), Some("Widevine"));
    let wv = d.widevine.as_ref().expect("widevine decoded");
    assert_eq!(wv.provider.as_deref(), Some("widevine_test"));
    // The one-line summary should advertise the decoded payload.
    let summary = boxes[0].decoded.as_deref().unwrap();
    assert!(
        summary.contains("provider=widevine_test"),
        "summary: {summary}"
    );
    assert!(summary.contains("scheme=cbcs"), "summary: {summary}");
}

#[test]
fn pssh_box_decoder_populates_playready_payload() {
    let data = pssh_box(
        &PLAYREADY_SYSTEM_ID,
        0,
        &[],
        &playready_object(&wrmheader_v40(&playready_kid_b64())),
    );
    let boxes = parse(&data);
    let Some(StructuredData::ProtectionSystemHeader(d)) = &boxes[0].structured_data else {
        panic!("expected structured pssh");
    };
    let pr = d.playready.as_ref().expect("playready decoded");
    assert_eq!(pr.wrm_header_version.as_deref(), Some("4.0.0.0"));
    let summary = boxes[0].decoded.as_deref().unwrap();
    assert!(
        summary.contains("wrm_version=4.0.0.0"),
        "summary: {summary}"
    );
    assert!(summary.contains("la_url="), "summary: {summary}");
}

#[test]
fn pssh_box_with_unparseable_data_still_yields_generic_fields() {
    // Widevine system ID but garbage payload: generic pssh fields survive,
    // the widevine section is absent.
    let data = pssh_box(&WIDEVINE_SYSTEM_ID, 0, &[], &[0xFF; 16]);
    let boxes = parse(&data);
    let Some(StructuredData::ProtectionSystemHeader(d)) = &boxes[0].structured_data else {
        panic!("expected structured pssh");
    };
    assert_eq!(d.system_name.as_deref(), Some("Widevine"));
    assert_eq!(d.data_size, 16);
    assert!(d.widevine.is_none());
}
