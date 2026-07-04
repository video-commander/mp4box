//! Tests for structured fragment decoders and fragmented-MP4 sample
//! extraction (moof/traf/tfhd/tfdt/trun with trex defaults).
//!
//! Builds a minimal synthetic fragmented file: moov with empty sample tables
//! and mvex/trex defaults, then two moof+mdat pairs.

use mp4box::registry::StructuredData;
use mp4box::{get_boxes, track_samples_from_reader};
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

/// moov with one track (id 1, timescale 1000, empty stbl) and trex defaults.
fn fragmented_moov(default_duration: u32, default_flags: u32) -> Vec<u8> {
    let mvhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]); // creation + modification
        p.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        p.extend_from_slice(&0u32.to_be_bytes()); // duration (fragmented: 0)
        p.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate
        p.extend_from_slice(&[0u8; 2 + 10 + 36 + 24]);
        p.extend_from_slice(&2u32.to_be_bytes()); // next_track_ID
        full_box(b"mvhd", 0, 0, &p)
    };

    let tkhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]); // creation + modification
        p.extend_from_slice(&1u32.to_be_bytes()); // track_id
        p.extend_from_slice(&[0u8; 4]); // reserved
        p.extend_from_slice(&0u32.to_be_bytes()); // duration
        p.extend_from_slice(&[0u8; 8 + 8 + 36 + 8]);
        full_box(b"tkhd", 0, 3, &p)
    };

    let mdhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]); // creation + modification
        p.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        p.extend_from_slice(&0u32.to_be_bytes()); // duration
        p.extend_from_slice(&0x55C4u16.to_be_bytes()); // "und"
        p.extend_from_slice(&[0u8; 2]);
        full_box(b"mdhd", 0, 0, &p)
    };

    let hdlr = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(b"vide");
        p.extend_from_slice(&[0u8; 12]);
        p.push(0); // empty name
        full_box(b"hdlr", 0, 0, &p)
    };

    // Empty sample tables (typical fragmented init data)
    let stbl = plain_box(
        b"stbl",
        &concat(&[
            full_box(b"stsd", 0, 0, &0u32.to_be_bytes()),
            full_box(b"stts", 0, 0, &0u32.to_be_bytes()),
            full_box(b"stsc", 0, 0, &0u32.to_be_bytes()),
            full_box(b"stsz", 0, 0, &[0u8; 8]),
            full_box(b"stco", 0, 0, &0u32.to_be_bytes()),
        ]),
    );
    let minf = plain_box(b"minf", &stbl);
    let mdia = plain_box(b"mdia", &concat(&[mdhd, hdlr, minf]));
    let trak = plain_box(b"trak", &concat(&[tkhd, mdia]));

    let trex = {
        let mut p = Vec::new();
        p.extend_from_slice(&1u32.to_be_bytes()); // track_id
        p.extend_from_slice(&1u32.to_be_bytes()); // default_sample_description_index
        p.extend_from_slice(&default_duration.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes()); // default_sample_size
        p.extend_from_slice(&default_flags.to_be_bytes());
        full_box(b"trex", 0, 0, &p)
    };
    let mvex = plain_box(b"mvex", &trex);

    plain_box(b"moov", &concat(&[mvhd, trak, mvex]))
}

/// moof with one traf: tfhd (default-base-is-moof), tfdt, and one trun with
/// per-sample sizes. `data_offset_to_mdat` points at the mdat payload
/// relative to moof start.
fn moof(
    sequence: u32,
    base_decode_time: u64,
    sizes: &[u32],
    first_sample_flags: u32,
    data_offset_to_mdat: i32,
) -> Vec<u8> {
    let mfhd = full_box(b"mfhd", 0, 0, &sequence.to_be_bytes());

    // flags: 0x020000 default-base-is-moof (no other tfhd fields)
    let tfhd = full_box(b"tfhd", 0, 0x020000, &1u32.to_be_bytes());
    let tfdt = full_box(b"tfdt", 1, 0, &base_decode_time.to_be_bytes());

    // trun flags: 0x000001 data-offset, 0x000004 first-sample-flags,
    // 0x000200 sample-size
    let trun = {
        let mut p = Vec::new();
        p.extend_from_slice(&(sizes.len() as u32).to_be_bytes());
        p.extend_from_slice(&data_offset_to_mdat.to_be_bytes());
        p.extend_from_slice(&first_sample_flags.to_be_bytes());
        for s in sizes {
            p.extend_from_slice(&s.to_be_bytes());
        }
        full_box(b"trun", 0, 0x000205, &p)
    };

    let traf = plain_box(b"traf", &concat(&[tfhd, tfdt, trun]));
    plain_box(b"moof", &concat(&[mfhd, traf]))
}

/// Two-fragment file: 3 samples then 2 samples, trex default duration 100,
/// default flags mark samples non-sync, first_sample_flags marks keyframes.
fn build_fragmented_file() -> Vec<u8> {
    let moov = fragmented_moov(100, 0x0101_0000);

    let sizes1: &[u32] = &[10, 20, 30];
    let sizes2: &[u32] = &[40, 50];

    // moof sizes are needed to compute the trun data offsets (mdat payload
    // starts at moof_len + 8-byte mdat header). Build once to measure.
    let m1_len = moof(1, 0, sizes1, 0x0200_0000, 0).len() as i32;
    let m2_len = moof(2, 300, sizes2, 0x0200_0000, 0).len() as i32;

    let moof1 = moof(1, 0, sizes1, 0x0200_0000, m1_len + 8);
    let mdat1 = plain_box(b"mdat", &[0xAA; 60]); // 10+20+30
    let moof2 = moof(2, 300, sizes2, 0x0200_0000, m2_len + 8);
    let mdat2 = plain_box(b"mdat", &[0xBB; 90]); // 40+50

    concat(&[
        plain_box(b"ftyp", &concat(&[b"iso5".to_vec(), vec![0u8; 4]])),
        moov,
        moof1,
        mdat1,
        moof2,
        mdat2,
    ])
}

#[test]
fn fragmented_file_samples_extracted() {
    let data = build_fragmented_file();
    let tracks = track_samples_from_reader(Cursor::new(&data)).unwrap();

    assert_eq!(tracks.len(), 1);
    let t = &tracks[0];
    assert_eq!(t.track_id, 1);
    assert_eq!(t.sample_count, 5);
    assert_eq!(t.samples.len(), 5);

    // Durations from trex default
    assert!(t.samples.iter().all(|s| s.duration == 100));

    // DTS: fragment 1 from tfdt=0, fragment 2 from tfdt=300
    let dts: Vec<u64> = t.samples.iter().map(|s| s.dts).collect();
    assert_eq!(dts, vec![0, 100, 200, 300, 400]);

    // Track duration filled from final DTS despite mdhd duration 0
    assert_eq!(t.duration, 500);

    // Sizes from trun
    let sizes: Vec<u32> = t.samples.iter().map(|s| s.size).collect();
    assert_eq!(sizes, vec![10, 20, 30, 40, 50]);

    // Sync: first sample of each fragment via first_sample_flags
    // (0x02000000 = depends-on-none, non-sync bit clear); others inherit the
    // trex default 0x01010000 which has the non-sync bit set.
    let sync: Vec<bool> = t.samples.iter().map(|s| s.is_sync).collect();
    assert_eq!(sync, vec![true, false, false, true, false]);

    // File offsets: sample data starts right after each moof's mdat header
    // and advances by sample size.
    let ftyp_len = 16u64;
    let moov_len = fragmented_moov(100, 0x0101_0000).len() as u64;
    let m1_start = ftyp_len + moov_len;
    let m1_len = moof(1, 0, &[10, 20, 30], 0x0200_0000, 0).len() as u64;
    let mdat1_payload = m1_start + m1_len + 8;
    let offsets: Vec<u64> = t.samples.iter().map(|s| s.file_offset).collect();
    assert_eq!(offsets[0], mdat1_payload);
    assert_eq!(offsets[1], mdat1_payload + 10);
    assert_eq!(offsets[2], mdat1_payload + 30);

    // mdat1 total = 8 + 60 bytes; moof2 starts right after it.
    let m2_start = m1_start + m1_len + 8 + 60;
    let m2_len = moof(2, 300, &[40, 50], 0x0200_0000, 0).len() as u64;
    let mdat2_payload = m2_start + m2_len + 8;
    assert_eq!(offsets[3], mdat2_payload);
    assert_eq!(offsets[4], mdat2_payload + 40);

    // PTS == DTS (no composition offsets in this fixture)
    assert!(t.samples.iter().all(|s| s.pts == s.dts));
}

#[test]
fn structured_fragment_boxes_decode() {
    let data = build_fragmented_file();
    let len = data.len() as u64;
    let mut cur = Cursor::new(&data);
    let boxes = get_boxes(&mut cur, len, true).unwrap();

    // trex under moov/mvex
    let moov = boxes.iter().find(|b| b.typ == "moov").unwrap();
    let mvex = moov
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|b| b.typ == "mvex")
        .unwrap();
    let trex = &mvex.children.as_ref().unwrap()[0];
    let Some(StructuredData::TrackExtends(d)) = &trex.structured_data else {
        panic!("expected structured trex");
    };
    assert_eq!(d.track_id, 1);
    assert_eq!(d.default_sample_duration, 100);
    assert_eq!(d.default_sample_flags, 0x0101_0000);

    // tfhd/tfdt under first moof/traf
    let moof = boxes.iter().find(|b| b.typ == "moof").unwrap();
    let traf = moof
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|b| b.typ == "traf")
        .unwrap();
    let tkids = traf.children.as_ref().unwrap();

    let Some(StructuredData::TrackFragmentHeader(tfhd)) = &tkids
        .iter()
        .find(|b| b.typ == "tfhd")
        .and_then(|b| b.structured_data.as_ref())
    else {
        panic!("expected structured tfhd");
    };
    assert_eq!(tfhd.track_id, 1);
    assert!(tfhd.default_base_is_moof);
    assert!(!tfhd.duration_is_empty);
    assert_eq!(tfhd.base_data_offset, None);

    let Some(StructuredData::TrackFragmentDecodeTime(tfdt)) = &tkids
        .iter()
        .find(|b| b.typ == "tfdt")
        .and_then(|b| b.structured_data.as_ref())
    else {
        panic!("expected structured tfdt");
    };
    assert_eq!(tfdt.base_media_decode_time, 0);
}

#[test]
fn structured_mvhd_fields() {
    let data = build_fragmented_file();
    let len = data.len() as u64;
    let mut cur = Cursor::new(&data);
    let boxes = get_boxes(&mut cur, len, true).unwrap();

    let moov = boxes.iter().find(|b| b.typ == "moov").unwrap();
    let mvhd = moov
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|b| b.typ == "mvhd")
        .unwrap();
    let Some(StructuredData::MovieHeader(d)) = &mvhd.structured_data else {
        panic!("expected structured mvhd");
    };
    assert_eq!(d.timescale, 1000);
    assert_eq!(d.duration, 0);
    assert_eq!(d.rate, 1.0);
    assert_eq!(d.next_track_id, 2);
    // decoded summary keeps the text-parsable format
    assert_eq!(mvhd.decoded.as_deref(), Some("timescale=1000 duration=0"));
}

/// Real-file integration, skipped when the fixture isn't present.
#[test]
fn real_fragmented_file_ground_truth() {
    let path = concat_home("Source/mp4-fixtures/output/fragmented_fmp4.mp4");
    if !std::path::Path::new(&path).exists() {
        eprintln!("skipping: {path} not present");
        return;
    }

    let tracks = mp4box::track_samples_from_path(&path).unwrap();
    assert_eq!(tracks.len(), 2);

    // Ground truth from ffprobe: 300 video packets, 432 audio packets;
    // first video packet at pos=6311, size=41665, keyframe.
    let video = tracks.iter().find(|t| t.handler_type == "vide").unwrap();
    assert_eq!(video.sample_count, 300);
    assert_eq!(video.samples[0].file_offset, 6311);
    assert_eq!(video.samples[0].size, 41665);
    assert!(video.samples[0].is_sync);
    assert!(!video.samples[1].is_sync);

    let audio = tracks.iter().find(|t| t.handler_type == "soun").unwrap();
    assert_eq!(audio.sample_count, 432);
    assert_eq!(audio.samples[0].file_offset, 6667518);
}

fn concat_home(rel: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/{rel}")
}
