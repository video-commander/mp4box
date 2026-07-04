//! Tests for the edit module.
//!
//! The load-bearing invariant: serializing an *unedited* tree reproduces the
//! source byte for byte. On top of that, structural edits must keep every
//! ancestor size correct and remap chunk offsets so samples still point at
//! the same media bytes.

#![cfg(feature = "edit")]

use mp4box::edit::{Command, Editor};
use mp4box::registry::StructuredData;
use mp4box::{get_boxes, get_itunes_tags, track_samples_from_reader};
use std::io::Cursor;

// ---------- fixture: a tiny but complete progressive MP4 ----------

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

/// Sample media payloads, distinct so tests can verify content by offset.
const SAMPLES: [&[u8]; 3] = [b"AAAAAAAAAA", b"BBBBBBBBBBBBBBB", b"CCCCC"];

/// Build `ftyp | free | mdat | moov` with a real sample table: 3 samples in
/// one chunk whose stco entry points into the mdat payload.
///
/// Returns (file_bytes, mdat_payload_offset).
fn build_progressive_file() -> (Vec<u8>, u64) {
    let ftyp = plain_box(b"ftyp", &concat(&[b"isom".to_vec(), vec![0u8; 4]]));
    let free = plain_box(b"free", &[0u8; 16]);

    let media: Vec<u8> = SAMPLES.concat();
    let mdat = plain_box(b"mdat", &media);
    let mdat_payload_offset = (ftyp.len() + free.len() + 8) as u64;

    // stbl tables describing the 3 samples
    let stsd = {
        let mut p = Vec::new();
        p.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        // Minimal mp4a audio sample entry (no codec children needed)
        let mut e = Vec::new();
        e.extend_from_slice(&[0u8; 6]);
        e.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
        e.extend_from_slice(&[0u8; 8]);
        e.extend_from_slice(&1u16.to_be_bytes()); // channels
        e.extend_from_slice(&16u16.to_be_bytes()); // sample size
        e.extend_from_slice(&[0u8; 4]);
        e.extend_from_slice(&(48000u32 << 16).to_be_bytes());
        p.extend_from_slice(&plain_box(b"mp4a", &e));
        full_box(b"stsd", 0, 0, &p)
    };
    let stts = {
        let mut p = Vec::new();
        p.extend_from_slice(&1u32.to_be_bytes());
        p.extend_from_slice(&3u32.to_be_bytes()); // 3 samples
        p.extend_from_slice(&100u32.to_be_bytes()); // delta 100
        full_box(b"stts", 0, 0, &p)
    };
    let stsc = {
        let mut p = Vec::new();
        p.extend_from_slice(&1u32.to_be_bytes());
        p.extend_from_slice(&1u32.to_be_bytes()); // first_chunk
        p.extend_from_slice(&3u32.to_be_bytes()); // samples_per_chunk
        p.extend_from_slice(&1u32.to_be_bytes()); // sdi
        full_box(b"stsc", 0, 0, &p)
    };
    let stsz = {
        let mut p = Vec::new();
        p.extend_from_slice(&0u32.to_be_bytes()); // per-sample sizes
        p.extend_from_slice(&3u32.to_be_bytes());
        for s in SAMPLES {
            p.extend_from_slice(&(s.len() as u32).to_be_bytes());
        }
        full_box(b"stsz", 0, 0, &p)
    };
    let stco = {
        let mut p = Vec::new();
        p.extend_from_slice(&1u32.to_be_bytes());
        p.extend_from_slice(&(mdat_payload_offset as u32).to_be_bytes());
        full_box(b"stco", 0, 0, &p)
    };
    let stbl = plain_box(b"stbl", &concat(&[stsd, stts, stsc, stsz, stco]));
    let minf = plain_box(b"minf", &stbl);

    let mdhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]);
        p.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        p.extend_from_slice(&300u32.to_be_bytes()); // duration
        p.extend_from_slice(&0x55C4u16.to_be_bytes()); // "und"
        p.extend_from_slice(&[0u8; 2]);
        full_box(b"mdhd", 0, 0, &p)
    };
    let hdlr = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(b"soun");
        p.extend_from_slice(&[0u8; 12]);
        p.push(0);
        full_box(b"hdlr", 0, 0, &p)
    };
    let mdia = plain_box(b"mdia", &concat(&[mdhd, hdlr, minf]));

    let tkhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]);
        p.extend_from_slice(&1u32.to_be_bytes()); // track_id
        p.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(&300u32.to_be_bytes()); // duration
        p.extend_from_slice(&[0u8; 8 + 8 + 36 + 8]);
        full_box(b"tkhd", 0, 3, &p)
    };
    let trak = plain_box(b"trak", &concat(&[tkhd, mdia]));

    let mvhd = {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 8]);
        p.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        p.extend_from_slice(&300u32.to_be_bytes()); // duration
        p.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate
        p.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
        p.extend_from_slice(&[0u8; 10]);
        // a deliberately non-identity matrix, to catch encoders that reset it
        let mut matrix = [0u8; 36];
        matrix[0] = 0xDE;
        matrix[35] = 0xAD;
        p.extend_from_slice(&matrix);
        p.extend_from_slice(&[0u8; 24]);
        p.extend_from_slice(&2u32.to_be_bytes()); // next_track_id
        full_box(b"mvhd", 0, 0, &p)
    };
    let udta = plain_box(b"udta", &plain_box(b"cust", b"custom-user-data"));

    let moov = plain_box(b"moov", &concat(&[mvhd, trak, udta]));

    (concat(&[ftyp, free, mdat, moov]), mdat_payload_offset)
}

fn run_editor(editor: &Editor, input: &[u8]) -> Vec<u8> {
    let mut src = Cursor::new(input.to_vec());
    let mut dst = Vec::new();
    editor.process(&mut src, &mut dst).expect("edit failed");
    dst
}

/// Read back the stco entry and sample bytes of the (single-track) file.
fn first_chunk_offset(data: &[u8]) -> u64 {
    let len = data.len() as u64;
    let mut cur = Cursor::new(data);
    let boxes = get_boxes(&mut cur, len, true).unwrap();
    fn find_stco(boxes: &[mp4box::Box]) -> Option<u64> {
        for b in boxes {
            if let Some(StructuredData::ChunkOffset(d)) = &b.structured_data {
                return d.chunk_offsets.first().map(|&v| v as u64);
            }
            if let Some(kids) = &b.children
                && let Some(v) = find_stco(kids)
            {
                return Some(v);
            }
        }
        None
    }
    find_stco(&boxes).expect("no stco found")
}

/// Assert all samples in `data` still resolve to the expected media bytes.
fn assert_samples_intact(data: &[u8]) {
    let tracks = track_samples_from_reader(Cursor::new(data.to_vec())).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].samples.len(), SAMPLES.len());
    for (s, expected) in tracks[0].samples.iter().zip(SAMPLES) {
        let start = s.file_offset as usize;
        let end = start + s.size as usize;
        assert_eq!(
            &data[start..end],
            expected,
            "sample {} does not point at its media bytes",
            s.index
        );
    }
}

// ---------- identity ----------

#[test]
fn identity_roundtrip_is_byte_exact() {
    let (input, _) = build_progressive_file();
    let output = run_editor(&Editor::new(), &input);
    assert_eq!(input, output, "unedited round-trip must be byte-identical");
}

#[test]
fn identity_roundtrip_with_trailing_padding() {
    let (mut input, _) = build_progressive_file();
    input.extend_from_slice(&[0x00, 0x01, 0x02]); // < 8 bytes of junk at EOF
    let output = run_editor(&Editor::new(), &input);
    assert_eq!(input, output);
}

#[test]
fn identity_roundtrip_real_files() {
    // Byte-identical round-trip on any local media files present.
    for path in [
        "video_counter_10min_unfragmented_avc.mp4",
        "BigBuckBunny.mp4",
        "tears-of-steel-360p_encoded_1773818279309.mp4",
    ] {
        if !std::path::Path::new(path).exists() {
            eprintln!("skipping: {path} not present");
            continue;
        }
        let input = std::fs::read(path).unwrap();
        let mut src = Cursor::new(&input);
        let mut dst = Vec::with_capacity(input.len());
        Editor::new().process(&mut src, &mut dst).unwrap();
        assert_eq!(input.len(), dst.len(), "{path}: length changed");
        assert_eq!(input, dst, "{path}: bytes changed");
    }
}

// ---------- structural edits ----------

#[test]
fn remove_before_mdat_shifts_chunk_offsets() {
    let (input, old_offset) = build_progressive_file();
    assert_eq!(first_chunk_offset(&input), old_offset);

    // Removing the 24-byte `free` box moves mdat 24 bytes earlier.
    let mut editor = Editor::new();
    editor.remove("free");
    let output = run_editor(&editor, &input);

    assert_eq!(output.len(), input.len() - 24);
    assert_eq!(first_chunk_offset(&output), old_offset - 24);
    assert_samples_intact(&output);
}

#[test]
fn remove_after_mdat_keeps_chunk_offsets() {
    let (input, old_offset) = build_progressive_file();

    // udta lives inside moov, after mdat: nothing moves for the media data.
    let mut editor = Editor::new();
    editor.remove("moov/udta");
    let output = run_editor(&editor, &input);

    assert!(output.len() < input.len());
    assert_eq!(
        first_chunk_offset(&output),
        old_offset,
        "chunk offsets must be untouched when mdat did not move"
    );
    assert_samples_intact(&output);

    // moov (the ancestor) must have shrunk by exactly the udta size and the
    // tree must reparse cleanly with no udta.
    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
    let boxes = get_boxes(&mut cur, len, false).unwrap();
    let moov = boxes.iter().find(|b| b.typ == "moov").unwrap();
    assert!(
        moov.children
            .as_ref()
            .unwrap()
            .iter()
            .all(|c| c.typ != "udta")
    );
    // No gaps: boxes must tile the file exactly.
    assert_eq!(boxes.iter().map(|b| b.size).sum::<u64>(), len);
}

#[test]
fn grow_box_before_mdat_shifts_chunk_offsets() {
    let (input, old_offset) = build_progressive_file();

    // Replace the 24-byte `free` box with a 48-byte one: mdat moves +24.
    let mut editor = Editor::new();
    editor.add_command(Command::Replace {
        path: "free".into(),
        bytes: plain_box(b"free", &[0u8; 40]),
    });
    let output = run_editor(&editor, &input);

    assert_eq!(output.len(), input.len() + 24);
    assert_eq!(first_chunk_offset(&output), old_offset + 24);
    assert_samples_intact(&output);
}

#[test]
fn insert_into_container_updates_ancestors() {
    let (input, _) = build_progressive_file();

    let extra = plain_box(b"cprt", b"\x00\x00\x00\x00(c) test");
    let mut editor = Editor::new();
    editor.add_command(Command::Insert {
        parent: "moov/udta".into(),
        bytes: extra.clone(),
        position: None,
    });
    let output = run_editor(&editor, &input);

    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
    let boxes = get_boxes(&mut cur, len, false).unwrap();
    let moov = boxes.iter().find(|b| b.typ == "moov").unwrap();
    let udta = moov
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|b| b.typ == "udta")
        .unwrap();
    let kids = udta.children.as_ref().unwrap();
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[1].typ, "cprt");
    assert_eq!(boxes.iter().map(|b| b.size).sum::<u64>(), len);
    assert_samples_intact(&output);
}

#[test]
fn remove_all_strips_every_match() {
    let (input, _) = build_progressive_file();
    let mut editor = Editor::new();
    editor.remove_all("free");
    editor.remove_all("cust");
    let output = run_editor(&editor, &input);

    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
    let boxes = get_boxes(&mut cur, len, false).unwrap();
    fn count(boxes: &[mp4box::Box], typ: &str) -> usize {
        boxes
            .iter()
            .map(|b| {
                (b.typ == typ) as usize + b.children.as_deref().map(|k| count(k, typ)).unwrap_or(0)
            })
            .sum()
    }
    assert_eq!(count(&boxes, "free"), 0);
    assert_eq!(count(&boxes, "cust"), 0);
    assert_samples_intact(&output);
}

// ---------- field patching ----------

#[test]
fn set_mvhd_field_preserves_all_other_bytes() {
    let (input, _) = build_progressive_file();

    let mut editor = Editor::new();
    editor.set_field("moov/mvhd", "creation_time", "3600");
    editor.set_field("moov/mvhd", "modification_time", "7200");
    let output = run_editor(&editor, &input);

    // Same length; only the timestamp bytes differ. Both fields were zero;
    // 3600 = 0x00000E10 and 7200 = 0x00001C20 change 2 bytes each.
    assert_eq!(input.len(), output.len());
    let diff: Vec<usize> = (0..input.len())
        .filter(|&i| input[i] != output[i])
        .collect();
    assert_eq!(diff.len(), 4, "expected only timestamp bytes to change");

    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
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
    assert_eq!(d.creation_time, 3600);
    assert_eq!(d.modification_time, 7200);
    assert_eq!(d.timescale, 1000, "timescale must be untouched");
    assert_eq!(d.next_track_id, 2, "next_track_id must be untouched");
}

#[test]
fn set_mdhd_language() {
    let (input, _) = build_progressive_file();
    let mut editor = Editor::new();
    editor.set_field("moov/trak/mdia/mdhd", "language", "eng");
    let output = run_editor(&editor, &input);

    let mut cur = Cursor::new(output.clone());
    let tracks = track_samples_from_reader(&mut cur).unwrap();
    assert_eq!(tracks.len(), 1);
    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
    let boxes = get_boxes(&mut cur, len, true).unwrap();
    fn find_mdhd_lang(boxes: &[mp4box::Box]) -> Option<String> {
        for b in boxes {
            if let Some(StructuredData::MediaHeader(d)) = &b.structured_data {
                return Some(d.language.clone());
            }
            if let Some(kids) = &b.children
                && let Some(l) = find_mdhd_lang(kids)
            {
                return Some(l);
            }
        }
        None
    }
    assert_eq!(find_mdhd_lang(&boxes).as_deref(), Some("eng"));
}

#[test]
fn set_unknown_field_errors() {
    let (input, _) = build_progressive_file();
    let mut editor = Editor::new();
    editor.set_field("moov/mvhd", "bogus_field", "1");
    let mut src = Cursor::new(input);
    let mut dst = Vec::new();
    let err = editor.process(&mut src, &mut dst).unwrap_err();
    assert!(err.to_string().contains("bogus_field"), "err: {err}");
}

// ---------- tags ----------

#[test]
fn set_tag_creates_udta_meta_ilst_chain() {
    let (input, _) = build_progressive_file();

    let mut editor = Editor::new();
    editor.set_tag("title", "Test Movie").unwrap();
    editor.set_tag("artist", "Test Artist").unwrap();
    let output = run_editor(&editor, &input);

    let len = output.len() as u64;
    let mut cur = Cursor::new(&output);
    let tags = get_itunes_tags(&mut cur, len).unwrap();
    assert_eq!(tags.get("title").map(String::as_str), Some("Test Movie"));
    assert_eq!(tags.get("artist").map(String::as_str), Some("Test Artist"));
    assert_samples_intact(&output);
}

#[test]
fn set_tag_replaces_existing_value() {
    let (input, _) = build_progressive_file();

    let mut editor = Editor::new();
    editor.set_tag("title", "First").unwrap();
    let output1 = run_editor(&editor, &input);

    let mut editor = Editor::new();
    editor.set_tag("title", "Second").unwrap();
    let output2 = run_editor(&editor, &output1);

    let len = output2.len() as u64;
    let mut cur = Cursor::new(&output2);
    let tags = get_itunes_tags(&mut cur, len).unwrap();
    assert_eq!(tags.get("title").map(String::as_str), Some("Second"));
    // Replacing must not grow the file by another atom: the two outputs
    // differ only in the value ("First" vs "Second" = +1 byte).
    assert_eq!(output2.len(), output1.len() + 1);
    assert_samples_intact(&output2);
}

// ---------- guards ----------

#[test]
fn fragmented_files_are_refused() {
    // Minimal file with a moof present.
    let ftyp = plain_box(b"ftyp", &concat(&[b"iso5".to_vec(), vec![0u8; 4]]));
    let moof = plain_box(b"moof", &full_box(b"mfhd", 0, 0, &1u32.to_be_bytes()));
    let input = concat(&[ftyp, moof]);

    let mut editor = Editor::new();
    editor.remove("ftyp");
    let mut src = Cursor::new(input.clone());
    let mut dst = Vec::new();
    let err = editor.process(&mut src, &mut dst).unwrap_err();
    assert!(err.to_string().contains("fragmented"), "err: {err}");

    // But a no-op pass-through still works (nothing moves).
    let output = run_editor(&Editor::new(), &input);
    assert_eq!(input, output);
}

#[test]
fn missing_path_errors_cleanly() {
    let (input, _) = build_progressive_file();
    let mut editor = Editor::new();
    editor.remove("moov/trak[3]");
    let mut src = Cursor::new(input);
    let mut dst = Vec::new();
    let err = editor.process(&mut src, &mut dst).unwrap_err();
    assert!(err.to_string().contains("not found"), "err: {err}");
}
