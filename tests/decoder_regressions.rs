//! Regression tests for decoder and parser bugs fixed in 0.9:
//! - FullBox decoders (mvhd/tkhd/elst/sidx/mdhd) double-parsing version/flags
//! - tkhd v0 reading 8-byte timestamps
//! - `meta` parsed as a plain container (version/flags leaked into children)
//! - stsd sample entries and their codec-config children not exposed
//! - mdhd version 1, stz2, iTunes tag coverage
//!
//! All fixtures are built synthetically so the tests are self-contained.

use mp4box::registry::StructuredData;
use mp4box::{Box, get_boxes, get_itunes_tags};
use std::io::Cursor;

// ---------- fixture builders ----------

/// Plain box: 4-byte size + fourcc + payload.
fn plain_box(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(payload);
    v
}

/// FullBox: adds version + 24-bit flags before the payload.
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

fn find<'a>(boxes: &'a [Box], typ: &str) -> &'a Box {
    boxes
        .iter()
        .find(|b| b.typ == typ)
        .unwrap_or_else(|| panic!("box {typ} not found"))
}

// ---------- mvhd ----------

fn mvhd_v0_payload(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    p.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    p.extend_from_slice(&timescale.to_be_bytes());
    p.extend_from_slice(&duration.to_be_bytes());
    p.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate
    p.extend_from_slice(&[0u8; 2 + 10 + 36 + 24 + 4]); // volume..next_track_ID
    p
}

#[test]
fn mvhd_v0_decodes_timescale_and_duration() {
    let data = full_box(b"mvhd", 0, 0, &mvhd_v0_payload(600, 357884));
    let boxes = parse(&data);
    let decoded = find(&boxes, "mvhd").decoded.as_deref().unwrap();
    assert_eq!(decoded, "timescale=600 duration=357884");
}

#[test]
fn mvhd_v1_decodes_timescale_and_duration() {
    let mut p = Vec::new();
    p.extend_from_slice(&0u64.to_be_bytes()); // creation_time
    p.extend_from_slice(&0u64.to_be_bytes()); // modification_time
    p.extend_from_slice(&90000u32.to_be_bytes());
    p.extend_from_slice(&8_589_934_592u64.to_be_bytes()); // > u32::MAX
    p.extend_from_slice(&[0u8; 80]);
    let data = full_box(b"mvhd", 1, 0, &p);
    let boxes = parse(&data);
    let decoded = find(&boxes, "mvhd").decoded.as_deref().unwrap();
    assert_eq!(decoded, "timescale=90000 duration=8589934592");
}

// ---------- tkhd ----------

fn tkhd_v0_payload(track_id: u32, duration: u32, width: u16, height: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes()); // creation_time (4 bytes in v0!)
    p.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    p.extend_from_slice(&track_id.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(&duration.to_be_bytes());
    p.extend_from_slice(&[0u8; 8]); // reserved[2]
    p.extend_from_slice(&[0u8; 8]); // layer/alternate_group/volume/reserved
    p.extend_from_slice(&[0u8; 36]); // matrix
    p.extend_from_slice(&((width as u32) << 16).to_be_bytes());
    p.extend_from_slice(&((height as u32) << 16).to_be_bytes());
    p
}

#[test]
fn tkhd_v0_structured_fields() {
    let data = full_box(b"tkhd", 0, 3, &tkhd_v0_payload(7, 48000, 1280, 720));
    let boxes = parse(&data);
    let tkhd = find(&boxes, "tkhd");
    let Some(StructuredData::TrackHeader(d)) = &tkhd.structured_data else {
        panic!("expected structured tkhd, got {:?}", tkhd.decoded);
    };
    assert_eq!(d.track_id, 7);
    assert_eq!(d.duration, 48000);
    assert_eq!(d.width, 1280.0);
    assert_eq!(d.height, 720.0);
    assert_eq!(d.flags, 3);
}

#[test]
fn tkhd_v1_structured_fields() {
    let mut p = Vec::new();
    p.extend_from_slice(&0u64.to_be_bytes()); // creation_time
    p.extend_from_slice(&0u64.to_be_bytes()); // modification_time
    p.extend_from_slice(&42u32.to_be_bytes()); // track_id
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(&6_000_000_000u64.to_be_bytes()); // duration > u32::MAX
    p.extend_from_slice(&[0u8; 8 + 8 + 36]);
    p.extend_from_slice(&(1920u32 << 16).to_be_bytes());
    p.extend_from_slice(&(1080u32 << 16).to_be_bytes());
    let data = full_box(b"tkhd", 1, 1, &p);
    let boxes = parse(&data);
    let Some(StructuredData::TrackHeader(d)) = &find(&boxes, "tkhd").structured_data else {
        panic!("expected structured tkhd");
    };
    assert_eq!(d.track_id, 42);
    assert_eq!(d.duration, 6_000_000_000);
    assert_eq!(d.width, 1920.0);
    assert_eq!(d.height, 1080.0);
}

// ---------- elst / sidx ----------

#[test]
fn elst_v0_decodes_first_entry() {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes()); // entry_count
    p.extend_from_slice(&1000u32.to_be_bytes()); // segment_duration
    p.extend_from_slice(&(-1i32).to_be_bytes()); // media_time
    p.extend_from_slice(&1i16.to_be_bytes()); // media_rate_integer
    p.extend_from_slice(&0i16.to_be_bytes()); // media_rate_fraction
    let data = full_box(b"elst", 0, 0, &p);
    let boxes = parse(&data);
    let decoded = find(&boxes, "elst").decoded.as_deref().unwrap();
    assert_eq!(
        decoded,
        "version=0 entries=1 first: duration=1000 media_time=-1 rate=1/0"
    );
}

#[test]
fn sidx_v0_decodes_summary_and_references() {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
    p.extend_from_slice(&48000u32.to_be_bytes()); // timescale
    p.extend_from_slice(&96000u32.to_be_bytes()); // earliest_presentation_time
    p.extend_from_slice(&0u32.to_be_bytes()); // first_offset
    p.extend_from_slice(&0u16.to_be_bytes()); // reserved
    p.extend_from_slice(&3u16.to_be_bytes()); // reference_count
    for i in 0..3u32 {
        p.extend_from_slice(&(1000 + i).to_be_bytes()); // type=0 + referenced_size
        p.extend_from_slice(&48000u32.to_be_bytes()); // subsegment_duration
        p.extend_from_slice(&0x9000_0000u32.to_be_bytes()); // starts_with_sap, sap_type=1
    }
    let data = full_box(b"sidx", 0, 0, &p);
    let boxes = parse(&data);
    let sidx = find(&boxes, "sidx");
    assert_eq!(
        sidx.decoded.as_deref(),
        Some("timescale=48000 earliest_presentation_time=96000 first_offset=0 references=3")
    );
    let Some(StructuredData::SegmentIndex(d)) = &sidx.structured_data else {
        panic!("expected structured sidx");
    };
    assert_eq!(d.reference_id, 1);
    assert_eq!(d.references.len(), 3);
    assert_eq!(d.references[0].referenced_size, 1000);
    assert_eq!(d.references[2].referenced_size, 1002);
    assert!(d.references[0].starts_with_sap);
    assert_eq!(d.references[0].sap_type, 1);
    assert_eq!(d.references[0].subsegment_duration, 48000);
}

// ---------- mdhd ----------

#[test]
fn mdhd_v1_structured_fields() {
    let mut p = Vec::new();
    p.extend_from_slice(&0u64.to_be_bytes()); // creation_time
    p.extend_from_slice(&0u64.to_be_bytes()); // modification_time
    p.extend_from_slice(&90000u32.to_be_bytes());
    p.extend_from_slice(&5_000_000_000u64.to_be_bytes()); // > u32::MAX
    p.extend_from_slice(&0x55C4u16.to_be_bytes()); // language "und"
    p.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    let data = full_box(b"mdhd", 1, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::MediaHeader(d)) = &find(&boxes, "mdhd").structured_data else {
        panic!("expected structured mdhd");
    };
    assert_eq!(d.timescale, 90000);
    assert_eq!(d.duration, 5_000_000_000);
    assert_eq!(d.language, "und");
}

// ---------- stz2 ----------

#[test]
fn stz2_decodes_8bit_field_sizes() {
    let mut p = Vec::new();
    p.extend_from_slice(&[0, 0, 0]); // reserved
    p.push(8); // field_size
    p.extend_from_slice(&3u32.to_be_bytes()); // sample_count
    p.extend_from_slice(&[10, 20, 30]);
    let data = full_box(b"stz2", 0, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::SampleSize(d)) = &find(&boxes, "stz2").structured_data else {
        panic!("expected structured stz2");
    };
    assert_eq!(d.sample_count, 3);
    assert_eq!(d.sample_sizes, vec![10, 20, 30]);
}

#[test]
fn stz2_decodes_4bit_field_sizes() {
    let mut p = Vec::new();
    p.extend_from_slice(&[0, 0, 0]);
    p.push(4); // field_size: two samples per byte
    p.extend_from_slice(&3u32.to_be_bytes());
    p.extend_from_slice(&[0x12, 0x30]); // sizes 1, 2, 3 (last nibble padding)
    let data = full_box(b"stz2", 0, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::SampleSize(d)) = &find(&boxes, "stz2").structured_data else {
        panic!("expected structured stz2");
    };
    assert_eq!(d.sample_sizes, vec![1, 2, 3]);
}

// ---------- meta ----------

fn ilst_with_tags() -> Vec<u8> {
    // ©nam > data (type 1 = UTF-8): "Hello"
    let mut name_data = Vec::new();
    name_data.extend_from_slice(&1u32.to_be_bytes()); // type indicator
    name_data.extend_from_slice(&0u32.to_be_bytes()); // locale
    name_data.extend_from_slice(b"Hello");
    let nam = plain_box(b"\xa9nam", &plain_box(b"data", &name_data));

    // trkn > data (type 0 implicit): track 3 of 12
    let mut trkn_data = Vec::new();
    trkn_data.extend_from_slice(&0u32.to_be_bytes()); // type indicator
    trkn_data.extend_from_slice(&0u32.to_be_bytes()); // locale
    trkn_data.extend_from_slice(&[0, 0, 0, 3, 0, 12, 0, 0]);
    let trkn = plain_box(b"trkn", &plain_box(b"data", &trkn_data));

    let mut ilst_payload = nam;
    ilst_payload.extend_from_slice(&trkn);
    plain_box(b"ilst", &ilst_payload)
}

fn iso_meta() -> Vec<u8> {
    // hdlr (FullBox) + ilst inside an ISO-style meta (FullBox container)
    let mut hdlr_payload = Vec::new();
    hdlr_payload.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    hdlr_payload.extend_from_slice(b"mdir");
    hdlr_payload.extend_from_slice(&[0u8; 12]); // reserved
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr_payload);

    let mut meta_payload = hdlr;
    meta_payload.extend_from_slice(&ilst_with_tags());
    full_box(b"meta", 0, 0, &meta_payload)
}

#[test]
fn iso_meta_children_parse_correctly() {
    let data = iso_meta();
    let boxes = parse(&data);
    let meta = find(&boxes, "meta");
    assert_eq!(meta.kind, "container");
    assert_eq!(meta.version, Some(0));
    let children = meta.children.as_ref().unwrap();
    let types: Vec<&str> = children.iter().map(|c| c.typ.as_str()).collect();
    assert_eq!(types, vec!["hdlr", "ilst"]);
}

#[test]
fn quicktime_meta_without_version_flags_parses() {
    // QT-style meta: children start immediately, no version/flags.
    let mut hdlr_payload = Vec::new();
    hdlr_payload.extend_from_slice(&0u32.to_be_bytes());
    hdlr_payload.extend_from_slice(b"mdta");
    hdlr_payload.extend_from_slice(&[0u8; 12]);
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr_payload);
    let data = plain_box(b"meta", &hdlr);

    let boxes = parse(&data);
    let meta = find(&boxes, "meta");
    assert_eq!(meta.kind, "container");
    let children = meta.children.as_ref().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].typ, "hdlr");
}

#[test]
fn itunes_tags_from_synthetic_file() {
    let udta = plain_box(b"udta", &iso_meta());
    let moov = plain_box(b"moov", &udta);

    let len = moov.len() as u64;
    let mut cur = Cursor::new(moov);
    let tags = get_itunes_tags(&mut cur, len).unwrap();
    assert_eq!(tags.get("title").map(String::as_str), Some("Hello"));
    assert_eq!(tags.get("track").map(String::as_str), Some("3/12"));
}

// ---------- stsd ----------

fn avc1_sample_entry(width: u16, height: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    p.extend_from_slice(&[0u8; 16]); // pre_defined/reserved
    p.extend_from_slice(&width.to_be_bytes());
    p.extend_from_slice(&height.to_be_bytes());
    p.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // horizresolution
    p.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // vertresolution
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(&1u16.to_be_bytes()); // frame_count
    p.extend_from_slice(&[0u8; 32]); // compressorname
    p.extend_from_slice(&0x0018u16.to_be_bytes()); // depth
    p.extend_from_slice(&(-1i16).to_be_bytes()); // pre_defined
    // avcC child: configurationVersion=1, profile=100, compat=0, level=31
    p.extend_from_slice(&plain_box(b"avcC", &[1, 100, 0, 31, 0xFF, 0xE1]));
    plain_box(b"avc1", &p)
}

fn mp4a_sample_entry(channels: u16, sample_rate: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 6]); // reserved
    p.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    p.extend_from_slice(&[0u8; 8]); // version/revision/vendor
    p.extend_from_slice(&channels.to_be_bytes());
    p.extend_from_slice(&16u16.to_be_bytes()); // samplesize
    p.extend_from_slice(&[0u8; 4]); // pre_defined + reserved
    p.extend_from_slice(&(sample_rate << 16).to_be_bytes());
    plain_box(b"mp4a", &p)
}

fn stsd_with_entries(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for e in entries {
        p.extend_from_slice(e);
    }
    full_box(b"stsd", 0, 0, &p)
}

#[test]
fn stsd_exposes_sample_entry_and_codec_children() {
    let data = stsd_with_entries(&[avc1_sample_entry(1920, 1080)]);
    let boxes = parse(&data);
    let stsd = find(&boxes, "stsd");

    // Tree: stsd > avc1 > avcC
    assert_eq!(stsd.kind, "container");
    let entries = stsd.children.as_ref().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].typ, "avc1");
    let codec_children = entries[0].children.as_ref().unwrap();
    assert_eq!(codec_children[0].typ, "avcC");
    let avcc = codec_children[0].decoded.as_deref().unwrap();
    assert!(avcc.contains("profile=100"), "avcC decoded: {avcc}");
    assert!(avcc.contains("level=3.1"), "avcC decoded: {avcc}");

    // Structured data: width/height and real data_reference_index
    let Some(StructuredData::SampleDescription(d)) = &stsd.structured_data else {
        panic!("expected structured stsd");
    };
    assert_eq!(d.entries.len(), 1);
    assert_eq!(d.entries[0].codec, "avc1");
    assert_eq!(d.entries[0].data_reference_index, 1);
    assert_eq!(d.entries[0].width, Some(1920));
    assert_eq!(d.entries[0].height, Some(1080));
}

#[test]
fn stsd_decodes_audio_fields_and_multiple_entries() {
    let data = stsd_with_entries(&[mp4a_sample_entry(2, 44100), avc1_sample_entry(640, 480)]);
    let boxes = parse(&data);
    let Some(StructuredData::SampleDescription(d)) = &find(&boxes, "stsd").structured_data else {
        panic!("expected structured stsd");
    };
    assert_eq!(d.entry_count, 2);
    assert_eq!(d.entries.len(), 2);
    assert_eq!(d.entries[0].codec, "mp4a");
    assert_eq!(d.entries[0].channel_count, Some(2));
    assert_eq!(d.entries[0].sample_rate, Some(44100));
    assert_eq!(d.entries[0].sample_size, Some(16));
    assert_eq!(d.entries[1].codec, "avc1");
    assert_eq!(d.entries[1].width, Some(640));
}

// ---------- classification ----------

#[test]
fn dinf_is_container_and_dref_visible() {
    use mp4box::known_boxes::KnownBox;
    use std::str::FromStr;
    let dinf = KnownBox::from(mp4box::FourCC::from_str("dinf").unwrap());
    assert!(dinf.is_container());

    // dref (FullBox) inside dinf must appear as a child.
    let dref = full_box(b"dref", 0, 0, &0u32.to_be_bytes());
    let data = plain_box(b"dinf", &dref);
    let boxes = parse(&data);
    let dinf = find(&boxes, "dinf");
    assert_eq!(dinf.children.as_ref().unwrap()[0].typ, "dref");
}

#[test]
fn trailing_padding_is_tolerated() {
    // A valid box followed by 4 bytes of zero padding must not error.
    let mut data = full_box(b"mvhd", 0, 0, &mvhd_v0_payload(600, 100));
    data.extend_from_slice(&[0u8; 4]);
    let len = data.len() as u64;
    let mut cur = Cursor::new(data);
    let boxes = get_boxes(&mut cur, len, false).expect("padding should be tolerated");
    assert_eq!(boxes.len(), 1);
}

// ---------- real-file integration (skipped when media files are absent) ----------

#[test]
fn real_file_ground_truth() {
    let path = "BigBuckBunny.mp4";
    if !std::path::Path::new(path).exists() {
        eprintln!("skipping: {path} not present");
        return;
    }

    let mut file = std::fs::File::open(path).unwrap();
    let size = file.metadata().unwrap().len();
    let boxes = get_boxes(&mut file, size, true).unwrap();

    // Ground truth read manually from the file bytes.
    let moov = find(&boxes, "moov");
    let mvhd = find(moov.children.as_ref().unwrap(), "mvhd");
    assert_eq!(
        mvhd.decoded.as_deref(),
        Some("timescale=600 duration=357884")
    );

    let tracks = mp4box::track_samples_from_path(path).unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].track_id, 1);
    assert_eq!(tracks[0].handler_type, "soun");
    assert_eq!(tracks[0].timescale, 44100);
    assert_eq!(tracks[0].samples[0].duration, 1024);
    assert_eq!(tracks[1].track_id, 2);
    assert_eq!(tracks[1].handler_type, "vide");
    // First video sample must be a keyframe located inside the file.
    assert!(tracks[1].samples[0].is_sync);
    assert!(tracks[1].samples[0].file_offset > 0);
    assert!(tracks[1].samples[0].file_offset < size);
}

// ---------- DRM / DASH boxes ----------

const WIDEVINE_ID: [u8; 16] = [
    0xED, 0xEF, 0x8B, 0xA9, 0x79, 0xD6, 0x4A, 0xCE, 0xA3, 0xC8, 0x27, 0xDC, 0xD5, 0x1D, 0x21, 0xED,
];

#[test]
fn pssh_v0_recognizes_widevine() {
    let mut p = Vec::new();
    p.extend_from_slice(&WIDEVINE_ID);
    p.extend_from_slice(&5u32.to_be_bytes()); // data_size
    p.extend_from_slice(&[1, 2, 3, 4, 5]);
    let data = full_box(b"pssh", 0, 0, &p);
    let boxes = parse(&data);
    let pssh = find(&boxes, "pssh");
    let Some(StructuredData::ProtectionSystemHeader(d)) = &pssh.structured_data else {
        panic!("expected structured pssh");
    };
    assert_eq!(d.system_id, "edef8ba9-79d6-4ace-a3c8-27dcd51d21ed");
    assert_eq!(d.system_name.as_deref(), Some("Widevine"));
    assert!(d.key_ids.is_empty());
    assert_eq!(d.data_size, 5);
    assert!(pssh.decoded.as_deref().unwrap().contains("Widevine"));
}

#[test]
fn pssh_v1_lists_key_ids() {
    let mut p = Vec::new();
    p.extend_from_slice(&WIDEVINE_ID);
    p.extend_from_slice(&2u32.to_be_bytes()); // KID_count
    p.extend_from_slice(&[0xAA; 16]);
    p.extend_from_slice(&[0xBB; 16]);
    p.extend_from_slice(&0u32.to_be_bytes()); // data_size
    let data = full_box(b"pssh", 1, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::ProtectionSystemHeader(d)) = &find(&boxes, "pssh").structured_data
    else {
        panic!("expected structured pssh");
    };
    assert_eq!(d.key_ids.len(), 2);
    assert_eq!(d.key_ids[0], "aa".repeat(16));
    assert_eq!(d.key_ids[1], "bb".repeat(16));
}

#[test]
fn tenc_v1_pattern_encryption() {
    // reserved, pattern (crypt=1, skip=9), is_protected, iv_size=0
    let mut p = vec![0u8, 0x19, 1, 0];
    p.extend_from_slice(&[0xCC; 16]); // default_KID
    p.push(16); // constant IV size
    p.extend_from_slice(&[0xDD; 16]);
    let data = full_box(b"tenc", 1, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::TrackEncryption(d)) = &find(&boxes, "tenc").structured_data else {
        panic!("expected structured tenc");
    };
    assert!(d.default_is_protected);
    assert_eq!(d.default_crypt_byte_block, 1);
    assert_eq!(d.default_skip_byte_block, 9);
    assert_eq!(d.default_per_sample_iv_size, 0);
    assert_eq!(d.default_kid, "cc".repeat(16));
    assert_eq!(
        d.default_constant_iv.as_deref(),
        Some("dd".repeat(16).as_str())
    );
}

#[test]
fn emsg_v0_and_v1_decode() {
    // v0: scheme/value strings first, then delta timing.
    let mut p = Vec::new();
    p.extend_from_slice(b"urn:scte:scte35:2013:xml\0");
    p.extend_from_slice(b"1\0");
    p.extend_from_slice(&90000u32.to_be_bytes()); // timescale
    p.extend_from_slice(&180000u32.to_be_bytes()); // presentation_time_delta
    p.extend_from_slice(&270000u32.to_be_bytes()); // event_duration
    p.extend_from_slice(&7u32.to_be_bytes()); // id
    p.extend_from_slice(b"<payload/>");
    let data = full_box(b"emsg", 0, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::EventMessage(d)) = &find(&boxes, "emsg").structured_data else {
        panic!("expected structured emsg");
    };
    assert_eq!(d.scheme_id_uri, "urn:scte:scte35:2013:xml");
    assert_eq!(d.value, "1");
    assert_eq!(d.timescale, 90000);
    assert_eq!(d.presentation_time_delta, Some(180000));
    assert_eq!(d.presentation_time, None);
    assert_eq!(d.id, 7);
    assert_eq!(d.message_size, 10);

    // v1: absolute 64-bit presentation time first, strings after.
    let mut p = Vec::new();
    p.extend_from_slice(&90000u32.to_be_bytes());
    p.extend_from_slice(&8_589_934_592u64.to_be_bytes()); // > u32::MAX
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&9u32.to_be_bytes());
    p.extend_from_slice(b"urn:example\0");
    p.extend_from_slice(b"\0");
    let data = full_box(b"emsg", 1, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::EventMessage(d)) = &find(&boxes, "emsg").structured_data else {
        panic!("expected structured emsg");
    };
    assert_eq!(d.scheme_id_uri, "urn:example");
    assert_eq!(d.presentation_time, Some(8_589_934_592));
    assert_eq!(d.presentation_time_delta, None);
    assert_eq!(d.message_size, 0);
}

// ---------- esds AudioSpecificConfig ----------

/// Build an esds payload with the given DecoderSpecificInfo bytes.
fn esds_payload(object_type: u8, dsi: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    // ES_Descriptor (tag 3): ES_ID(2) + flags(1) + DecoderConfigDescriptor
    let dcd_len = 13 + 2 + dsi.len();
    p.push(0x03);
    p.push((3 + 2 + dcd_len) as u8);
    p.extend_from_slice(&[0, 1, 0]); // ES_ID=1, no flags
    // DecoderConfigDescriptor (tag 4)
    p.push(0x04);
    p.push((13 + 2 + dsi.len()) as u8);
    p.push(object_type);
    p.extend_from_slice(&[0x15, 0, 0, 0]); // streamType + bufferSizeDB
    p.extend_from_slice(&128_000u32.to_be_bytes()); // maxBitrate
    p.extend_from_slice(&96_000u32.to_be_bytes()); // avgBitrate
    // DecoderSpecificInfo (tag 5)
    p.push(0x05);
    p.push(dsi.len() as u8);
    p.extend_from_slice(dsi);
    p
}

fn parse_esds(dsi: &[u8]) -> mp4box::registry::EsdsData {
    let data = full_box(b"esds", 0, 0, &esds_payload(0x40, dsi));
    let boxes = parse(&data);
    let Some(StructuredData::ElementaryStream(d)) = &find(&boxes, "esds").structured_data else {
        panic!("expected structured esds");
    };
    d.clone()
}

#[test]
fn esds_aac_lc() {
    // AOT=2 (00010), freq idx=4/44100 (0100), channels=2 (0010), GA=000
    // -> 00010 0100 0010 000x = 0x12 0x10
    let d = parse_esds(&[0x12, 0x10]);
    let a = d.audio_config.expect("audio config");
    assert_eq!(a.profile, "AAC-LC");
    assert_eq!(a.audio_object_type, 2);
    assert_eq!(a.sample_rate, 44100);
    assert_eq!(a.channel_configuration, 2);
    assert!(!a.sbr);
    assert_eq!(a.extension_sample_rate, None);
    assert_eq!(d.max_bitrate, 128_000);
    assert_eq!(d.avg_bitrate, 96_000);
}

#[test]
fn esds_he_aac_hierarchical() {
    // AOT=5/SBR (00101), ext freq idx... wait: hierarchical layout is
    // aot=5, core freq idx=7/22050 (0111), channels=2 (0010),
    // ext freq idx=4/44100 (0100), inner aot=2 (00010)
    // bits: 00101 0111 0010 0100 00010 -> 0x2B 0x92 0x08 (last 2 bits pad)
    let d = parse_esds(&[0x2B, 0x92, 0x08]);
    let a = d.audio_config.expect("audio config");
    assert_eq!(a.profile, "HE-AAC");
    assert!(a.sbr);
    assert!(!a.ps);
    assert_eq!(a.sample_rate, 22050);
    assert_eq!(a.extension_sample_rate, Some(44100));
    assert_eq!(a.audio_object_type, 2); // inner codec is AAC-LC
}

#[test]
fn esds_he_aac_backward_compatible() {
    // Real-world bytes from an AudioToolbox HE-AAC encode:
    // AAC-LC@22050 stereo core + 0x2B7 sync extension, SBR, ext rate 44100.
    let d = parse_esds(&[0x13, 0x90, 0x56, 0xE5, 0xA0]);
    let a = d.audio_config.expect("audio config");
    assert_eq!(a.profile, "HE-AAC");
    assert!(a.sbr);
    assert!(!a.ps);
    assert_eq!(a.sample_rate, 22050);
    assert_eq!(a.extension_sample_rate, Some(44100));
    assert_eq!(a.channel_configuration, 2);
}

#[test]
fn esds_plain_aac_without_dsi_still_decodes() {
    // esds with no DecoderSpecificInfo at all.
    let mut p = Vec::new();
    p.push(0x03);
    p.push(18);
    p.extend_from_slice(&[0, 1, 0]);
    p.push(0x04);
    p.push(13);
    p.push(0x6B); // MP3
    p.extend_from_slice(&[0x15, 0, 0, 0]);
    p.extend_from_slice(&320_000u32.to_be_bytes());
    p.extend_from_slice(&320_000u32.to_be_bytes());
    let data = full_box(b"esds", 0, 0, &p);
    let boxes = parse(&data);
    let Some(StructuredData::ElementaryStream(d)) = &find(&boxes, "esds").structured_data else {
        panic!("expected structured esds");
    };
    assert_eq!(d.object_type, 0x6B);
    assert_eq!(d.object_type_name, "MP3");
    assert!(d.audio_config.is_none());
}
