//! Box payload decoders. Each `*Decoder` implements
//! [`BoxDecoder`](super::BoxDecoder) for one box type.

use super::cicp;
use super::codec_config::{parse_audio_specific_config, read_descriptor_length};
use super::data::*;
use super::{BoxDecoder, BoxValue};
use crate::boxes::BoxHeader;
use crate::util::ReadExt;
use std::io::{Cursor, Read};

// ---------- Helpers ----------

fn read_all(r: &mut dyn Read) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Appends a payload-relative field span of `len` bytes at `*pos`, then
/// advances `*pos` past it. Used by `field_spans` impls to lay out fields in
/// declaration order.
fn span(name: &str, len: u64, pos: &mut u64) -> FieldSpan {
    let s = FieldSpan {
        name: name.to_string(),
        start: *pos,
        length: len,
    };
    *pos += len;
    s
}

fn lang_from_u16(code: u16) -> String {
    if code == 0 {
        return "und".to_string();
    }
    let c1 = ((code >> 10) & 0x1F) as u8 + 0x60;
    let c2 = ((code >> 5) & 0x1F) as u8 + 0x60;
    let c3 = (code & 0x1F) as u8 + 0x60;
    format!("{}{}{}", c1 as char, c2 as char, c3 as char,)
}

// ---------- Decoders ----------

// ftyp: major + minor + compatible brands
pub struct FtypDecoder;

impl BoxDecoder for FtypDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 8 {
            return Ok(BoxValue::Text(format!(
                "ftyp: payload too short ({} bytes)",
                buf.len()
            )));
        }

        let major = &buf[0..4];
        let mut minor_bytes = [0u8; 4];
        minor_bytes.copy_from_slice(&buf[4..8]);
        let minor = u32::from_be_bytes(minor_bytes);

        let mut brands = Vec::new();
        for chunk in buf[8..].chunks(4) {
            if chunk.len() == 4 {
                brands.push(String::from_utf8_lossy(chunk).to_string());
            }
        }

        Ok(BoxValue::Text(format!(
            "major={} minor={} compatible={:?}",
            String::from_utf8_lossy(major),
            minor,
            brands
        )))
    }
}

// mvhd: timescale + duration
pub struct MvhdDecoder;

impl BoxDecoder for MvhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // mvhd is a FullBox: version/flags are stripped by the parser and
        // passed in, so the payload starts at creation_time.
        let version = version.unwrap_or(0);
        let (creation_time, modification_time, timescale, duration) = if version == 1 {
            let creation = r.read_u64_be()?;
            let modification = r.read_u64_be()?;
            let ts = r.read_u32_be()?;
            let dur = r.read_u64_be()?;
            (creation, modification, ts, dur)
        } else {
            let creation = r.read_u32_be()? as u64;
            let modification = r.read_u32_be()? as u64;
            let ts = r.read_u32_be()?;
            let dur = r.read_u32_be()? as u64;
            (creation, modification, ts, dur)
        };

        let rate = r.read_i32_be()? as f32 / 65536.0;
        let volume = r.read_i16_be()? as f32 / 256.0;
        // reserved (10) + matrix (36) + pre_defined (24)
        let mut skip = [0u8; 10 + 36 + 24];
        r.read_exact(&mut skip)?;
        let next_track_id = r.read_u32_be()?;

        Ok(BoxValue::Structured(StructuredData::MovieHeader(
            MvhdData {
                version,
                flags: flags.unwrap_or(0),
                creation_time,
                modification_time,
                timescale,
                duration,
                rate,
                volume,
                next_track_id,
            },
        )))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        // Widths mirror the decode() layout above: version 1 stores the
        // times/duration as 64-bit, version 0 as 32-bit.
        let time_w: u64 = if version == Some(1) { 8 } else { 4 };
        let dur_w: u64 = if version == Some(1) { 8 } else { 4 };
        let mut pos = 0u64;
        let mut spans = vec![
            span("creation_time", time_w, &mut pos),
            span("modification_time", time_w, &mut pos),
            span("timescale", 4, &mut pos),
            span("duration", dur_w, &mut pos),
            span("rate", 4, &mut pos),
            span("volume", 2, &mut pos),
        ];
        // reserved (10) + matrix (36) + pre_defined (24) are unnamed padding.
        pos += 10 + 36 + 24;
        spans.push(span("next_track_id", 4, &mut pos));
        spans
    }
}

// tkhd: track id, duration, width, height
pub struct TkhdDecoder;

impl BoxDecoder for TkhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // tkhd is a FullBox: version/flags are stripped by the parser and
        // passed in, so the payload starts at creation_time.
        let buf = read_all(r)?;
        let version = version.unwrap_or(0);
        let mut pos = 0usize;

        let read_u32 = |pos: &mut usize| -> Option<u32> {
            let v = u32::from_be_bytes(buf.get(*pos..*pos + 4)?.try_into().unwrap());
            *pos += 4;
            Some(v)
        };
        let read_u64 = |pos: &mut usize| -> Option<u64> {
            let v = u64::from_be_bytes(buf.get(*pos..*pos + 8)?.try_into().unwrap());
            *pos += 8;
            Some(v)
        };

        let (track_id, duration) = if version == 1 {
            // creation_time (8), modification_time (8), track_id (4), reserved (4), duration (8)
            if read_u64(&mut pos).is_none() || read_u64(&mut pos).is_none() {
                return Ok(BoxValue::Text(
                    "tkhd: truncated creation/modification".into(),
                ));
            }
            let id = read_u32(&mut pos).unwrap_or(0);
            let _ = read_u32(&mut pos); // reserved
            (id, read_u64(&mut pos).unwrap_or(0))
        } else {
            // creation_time (4), modification_time (4), track_id (4), reserved (4), duration (4)
            if read_u32(&mut pos).is_none() || read_u32(&mut pos).is_none() {
                return Ok(BoxValue::Text(
                    "tkhd: truncated creation/modification".into(),
                ));
            }
            let id = read_u32(&mut pos).unwrap_or(0);
            let _ = read_u32(&mut pos); // reserved
            (id, read_u32(&mut pos).unwrap_or(0) as u64)
        };

        // reserved[2] (8) + layer/alternate_group/volume/reserved (8) + matrix (36)
        pos += 8 + 8 + 36;

        // width / height as 16.16 fixed point
        let (width, height) = {
            let w = read_u32(&mut pos);
            let h = read_u32(&mut pos);
            match (w, h) {
                (Some(w), Some(h)) => (w as f32 / 65536.0, h as f32 / 65536.0),
                _ => (0.0, 0.0),
            }
        };

        let data = TkhdData {
            version,
            flags: flags.unwrap_or(0),
            track_id,
            duration,
            width,
            height,
        };

        Ok(BoxValue::Structured(StructuredData::TrackHeader(data)))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        let (time_w, dur_w): (u64, u64) = if version == Some(1) { (8, 8) } else { (4, 4) };
        let mut pos = 0u64;
        let mut spans = vec![
            span("creation_time", time_w, &mut pos),
            span("modification_time", time_w, &mut pos),
            span("track_id", 4, &mut pos),
        ];
        pos += 4; // reserved
        spans.push(span("duration", dur_w, &mut pos));
        // reserved[2] (8) + layer/alternate_group/volume/reserved (8) + matrix (36)
        pos += 8 + 8 + 36;
        spans.push(span("width", 4, &mut pos));
        spans.push(span("height", 4, &mut pos));
        spans
    }
}

// mdhd: timescale, duration, language
pub struct MdhdDecoder;

impl BoxDecoder for MdhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let version = version.unwrap_or(0);
        let (creation_time, modification_time, timescale, duration) = if version == 1 {
            let creation = r.read_u64_be()?;
            let modification = r.read_u64_be()?;
            let ts = r.read_u32_be()?;
            let dur = r.read_u64_be()?;
            (creation, modification, ts, dur)
        } else {
            let creation = r.read_u32_be()? as u64;
            let modification = r.read_u32_be()? as u64;
            let ts = r.read_u32_be()?;
            let dur = r.read_u32_be()? as u64;
            (creation, modification, ts, dur)
        };
        let language_code = r.read_u16_be()?;
        let _pre_defined = r.read_u16_be()?;

        let lang = lang_from_u16(language_code);

        let data = MdhdData {
            version,
            flags: flags.unwrap_or(0),
            creation_time,
            modification_time,
            timescale,
            duration,
            language: lang,
        };

        Ok(BoxValue::Structured(StructuredData::MediaHeader(data)))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        let (time_w, dur_w): (u64, u64) = if version == Some(1) { (8, 8) } else { (4, 4) };
        let mut pos = 0u64;
        vec![
            span("creation_time", time_w, &mut pos),
            span("modification_time", time_w, &mut pos),
            span("timescale", 4, &mut pos),
            span("duration", dur_w, &mut pos),
            span("language", 2, &mut pos),
        ]
    }
}

// hdlr: handler type + name
pub struct HdlrDecoder;

impl BoxDecoder for HdlrDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        use crate::util::ReadExt;

        // pre_defined (4 bytes) + handler_type (4 bytes)
        let _pre_defined = r.read_u32_be()?;
        let mut handler_type = [0u8; 4];
        r.read_exact(&mut handler_type)?;

        // reserved (3 * 4 bytes)
        let mut reserved = [0u8; 12];
        r.read_exact(&mut reserved)?;

        // name: null-terminated string (or just rest of box)
        let mut name_bytes = Vec::new();
        r.read_to_end(&mut name_bytes)?;
        // strip trailing nulls
        while name_bytes.last() == Some(&0) {
            name_bytes.pop();
        }
        let name = String::from_utf8_lossy(&name_bytes).to_string();

        let handler_str = std::str::from_utf8(&handler_type).unwrap_or("????");

        let data = HdlrData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            handler_type: handler_str.to_string(),
            name,
        };

        Ok(BoxValue::Structured(StructuredData::HandlerReference(data)))
    }

    fn field_spans(
        &self,
        _version: Option<u8>,
        _flags: Option<u32>,
        payload_len: u64,
    ) -> Vec<FieldSpan> {
        // pre_defined (4) + handler_type (4) + reserved (12) + name (rest).
        let mut spans = Vec::new();
        if payload_len >= 8 {
            spans.push(FieldSpan {
                name: "handler_type".into(),
                start: 4,
                length: 4,
            });
        }
        if payload_len > 20 {
            spans.push(FieldSpan {
                name: "name".into(),
                start: 20,
                length: payload_len - 20,
            });
        }
        spans
    }
}

// sidx: segment index summary
pub struct SidxDecoder;

impl BoxDecoder for SidxDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // sidx is a FullBox: version/flags are stripped by the parser and
        // passed in, so the payload starts at reference_ID.
        let version = version.unwrap_or(0);
        let reference_id = r.read_u32_be()?;
        let timescale = r.read_u32_be()?;

        let (earliest_presentation_time, first_offset) = if version == 1 {
            (r.read_u64_be()?, r.read_u64_be()?)
        } else {
            (r.read_u32_be()? as u64, r.read_u32_be()? as u64)
        };

        let _reserved = r.read_u16_be()?;
        let ref_count = r.read_u16_be()?;

        let mut references = Vec::with_capacity(ref_count as usize);
        for _ in 0..ref_count {
            let word = r.read_u32_be()?;
            let subsegment_duration = r.read_u32_be()?;
            let sap = r.read_u32_be()?;
            references.push(SidxReference {
                reference_type: ((word >> 31) & 1) as u8,
                referenced_size: word & 0x7FFF_FFFF,
                subsegment_duration,
                starts_with_sap: (sap >> 31) & 1 == 1,
                sap_type: ((sap >> 28) & 0x07) as u8,
                sap_delta_time: sap & 0x0FFF_FFFF,
            });
        }

        Ok(BoxValue::Structured(StructuredData::SegmentIndex(
            SidxData {
                version,
                flags: flags.unwrap_or(0),
                reference_id,
                timescale,
                earliest_presentation_time,
                first_offset,
                references,
            },
        )))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        // earliest_presentation_time and first_offset widen to 64-bit in v1;
        // the reference array (after reserved + reference_count) is omitted.
        let w: u64 = if version == Some(1) { 8 } else { 4 };
        let mut pos = 0u64;
        vec![
            span("reference_id", 4, &mut pos),
            span("timescale", 4, &mut pos),
            span("earliest_presentation_time", w, &mut pos),
            span("first_offset", w, &mut pos),
        ]
    }
}

// stsd: list sample entry formats, maybe WxH
// ---- stsd decoder: codec + width/height for first entry -----------------
pub struct StsdDecoder;

/// Codec families that share a fixed sample-entry field layout.
fn is_visual_sample_entry(codec: &[u8; 4]) -> bool {
    matches!(
        codec,
        b"avc1"
            | b"avc2"
            | b"avc3"
            | b"avc4"
            | b"hev1"
            | b"hvc1"
            | b"vvc1"
            | b"mp4v"
            | b"vp08"
            | b"vp09"
            | b"av01"
            | b"dvh1"
            | b"dvhe"
            | b"dav1"
            | b"encv"
    )
}

fn is_audio_sample_entry(codec: &[u8; 4]) -> bool {
    matches!(
        codec,
        b"mp4a"
            | b"ac-3"
            | b"ec-3"
            | b"Opus"
            | b"opus"
            | b"samr"
            | b"sawb"
            | b"alac"
            | b"fLaC"
            | b"enca"
            | b"ipcm"
            | b"fpcm"
    )
}

impl BoxDecoder for StsdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // stsd is a FullBox; the payload starts at entry_count, followed by
        // sample entry boxes.
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        let entry_count = cur.read_u32_be()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let entry_start = cur.position();
            if entry_start + 8 > buf.len() as u64 {
                break;
            }
            let entry_size = cur.read_u32_be()?;

            let mut codec_bytes = [0u8; 4];
            cur.read_exact(&mut codec_bytes)?;
            let codec = codec_bytes
                .iter()
                .map(|&c| {
                    if (0x20..=0x7e).contains(&c) {
                        c as char
                    } else {
                        '.'
                    }
                })
                .collect::<String>();

            // SampleEntry base: 6 reserved bytes + data_reference_index
            let mut reserved = [0u8; 6];
            cur.read_exact(&mut reserved)?;
            let data_reference_index = cur.read_u16_be()?;

            let mut entry = SampleEntry {
                size: entry_size,
                codec,
                data_reference_index,
                width: None,
                height: None,
                channel_count: None,
                sample_size: None,
                sample_rate: None,
            };

            if is_visual_sample_entry(&codec_bytes) {
                // pre_defined (2) + reserved (2) + pre_defined (12), then
                // width and height
                let mut skip = [0u8; 16];
                cur.read_exact(&mut skip)?;
                entry.width = Some(cur.read_u16_be()?);
                entry.height = Some(cur.read_u16_be()?);
            } else if is_audio_sample_entry(&codec_bytes) {
                // version (2) + revision (2) + vendor (4), then channelcount,
                // samplesize, pre_defined (2), reserved (2), samplerate (16.16)
                let mut skip = [0u8; 8];
                cur.read_exact(&mut skip)?;
                entry.channel_count = Some(cur.read_u16_be()?);
                entry.sample_size = Some(cur.read_u16_be()?);
                let mut skip = [0u8; 4];
                cur.read_exact(&mut skip)?;
                entry.sample_rate = Some(cur.read_u32_be()? >> 16);
            }

            entries.push(entry);

            // Jump to the next entry regardless of how much we understood.
            if entry_size >= 8 {
                cur.set_position(entry_start + entry_size as u64);
            } else {
                break; // size 0/invalid: cannot locate further entries
            }
        }

        let data = StsdData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            entries,
        };

        Ok(BoxValue::Structured(StructuredData::SampleDescription(
            data,
        )))
    }
}

// stts: time-to-sample
pub struct SttsDecoder;

impl BoxDecoder for SttsDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        // and stripped from the payload. We start directly with the box-specific data.
        let entry_count = cur.read_u32_be()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let sample_count = cur.read_u32_be()?;
            let sample_delta = cur.read_u32_be()?;
            entries.push(SttsEntry {
                sample_count,
                sample_delta,
            });
        }

        let data = SttsData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            entries,
        };

        Ok(BoxValue::Structured(StructuredData::DecodingTimeToSample(
            data,
        )))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// stss: sync sample table
pub struct StssDecoder;

impl BoxDecoder for StssDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let entry_count = cur.read_u32_be()?;
        let mut sample_numbers = Vec::new();

        for _ in 0..entry_count {
            sample_numbers.push(cur.read_u32_be()?);
        }

        let data = StssData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            sample_numbers,
        };

        Ok(BoxValue::Structured(StructuredData::SyncSample(data)))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// ctts: composition time to sample
pub struct CttsDecoder;

impl BoxDecoder for CttsDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let entry_count = cur.read_u32_be()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let sample_count = cur.read_u32_be()?;
            // Note: In version 1, sample_offset can be signed, but since we don't have access
            // to the parsed version here, we assume version 0 behavior (unsigned)
            let sample_offset = cur.read_u32_be()? as i32;
            entries.push(CttsEntry {
                sample_count,
                sample_offset,
            });
        }

        let data = CttsData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            entries,
        };

        Ok(BoxValue::Structured(
            StructuredData::CompositionTimeToSample(data),
        ))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// stsc: sample-to-chunk
pub struct StscDecoder;

impl BoxDecoder for StscDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let entry_count = cur.read_u32_be()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let first_chunk = cur.read_u32_be()?;
            let samples_per_chunk = cur.read_u32_be()?;
            let sample_description_index = cur.read_u32_be()?;
            entries.push(StscEntry {
                first_chunk,
                samples_per_chunk,
                sample_description_index,
            });
        }

        let data = StscData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            entries,
        };

        Ok(BoxValue::Structured(StructuredData::SampleToChunk(data)))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// stsz: sample sizes
pub struct StszDecoder;

impl BoxDecoder for StszDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let sample_size = cur.read_u32_be()?;
        let sample_count = cur.read_u32_be()?;
        let mut sample_sizes = Vec::new();

        // If sample_size is 0, each sample has its own size
        if sample_size == 0 {
            for _ in 0..sample_count {
                sample_sizes.push(cur.read_u32_be()?);
            }
        }

        let data = StszData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            sample_size,
            sample_count,
            sample_sizes,
        };

        Ok(BoxValue::Structured(StructuredData::SampleSize(data)))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        let mut pos = 0u64;
        vec![
            span("sample_size", 4, &mut pos),
            span("sample_count", 4, &mut pos),
        ]
    }
}

// stz2: compact sample sizes (4-, 8-, or 16-bit fields)
pub struct Stz2Decoder;

impl BoxDecoder for Stz2Decoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // reserved (3 bytes) + field_size (1 byte)
        let mut reserved = [0u8; 3];
        cur.read_exact(&mut reserved)?;
        let field_size = cur.read_u8()?;
        let sample_count = cur.read_u32_be()?;

        let mut sample_sizes = Vec::with_capacity(sample_count as usize);
        match field_size {
            4 => {
                let mut remaining = sample_count;
                while remaining > 0 {
                    let byte = cur.read_u8()?;
                    sample_sizes.push((byte >> 4) as u32);
                    remaining -= 1;
                    if remaining > 0 {
                        sample_sizes.push((byte & 0x0F) as u32);
                        remaining -= 1;
                    }
                }
            }
            8 => {
                for _ in 0..sample_count {
                    sample_sizes.push(cur.read_u8()? as u32);
                }
            }
            16 => {
                for _ in 0..sample_count {
                    sample_sizes.push(cur.read_u16_be()? as u32);
                }
            }
            other => anyhow::bail!("stz2: invalid field_size {}", other),
        }

        // Reuse the stsz shape: sample_size == 0 means per-sample sizes.
        let data = StszData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            sample_size: 0,
            sample_count,
            sample_sizes,
        };

        Ok(BoxValue::Structured(StructuredData::SampleSize(data)))
    }
}

// stco: 32-bit chunk offsets
pub struct StcoDecoder;

impl BoxDecoder for StcoDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let entry_count = cur.read_u32_be()?;
        let mut chunk_offsets = Vec::new();

        for _ in 0..entry_count {
            chunk_offsets.push(cur.read_u32_be()?);
        }

        let data = StcoData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            chunk_offsets,
        };

        Ok(BoxValue::Structured(StructuredData::ChunkOffset(data)))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// co64: 64-bit chunk offsets
pub struct Co64Decoder;

impl BoxDecoder for Co64Decoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        // For FullBox types, version and flags are already parsed by the main parser
        let entry_count = cur.read_u32_be()?;
        let mut chunk_offsets = Vec::new();

        for _ in 0..entry_count {
            chunk_offsets.push(cur.read_u64_be()?);
        }

        let data = Co64Data {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            chunk_offsets,
        };

        Ok(BoxValue::Structured(StructuredData::ChunkOffset64(data)))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// elst: edit list
pub struct ElstDecoder;

impl BoxDecoder for ElstDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // elst is a FullBox: version/flags are stripped by the parser and
        // passed in, so the payload starts at entry_count.
        let version = version.unwrap_or(0);
        let entry_count = r.read_u32_be()?;

        let mut entries = Vec::new();
        for _ in 0..entry_count {
            let (segment_duration, media_time) = if version == 1 {
                (r.read_u64_be()?, r.read_i64_be()?)
            } else {
                (r.read_u32_be()? as u64, r.read_i32_be()? as i64)
            };
            entries.push(ElstEntry {
                segment_duration,
                media_time,
                media_rate_integer: r.read_i16_be()?,
                media_rate_fraction: r.read_i16_be()?,
            });
        }

        Ok(BoxValue::Structured(StructuredData::EditList(ElstData {
            version,
            flags: flags.unwrap_or(0),
            entry_count,
            entries,
        })))
    }

    fn field_spans(&self, _: Option<u8>, _: Option<u32>, _: u64) -> Vec<FieldSpan> {
        vec![FieldSpan {
            name: "entry_count".into(),
            start: 0,
            length: 4,
        }]
    }
}

// ---------- New decoders ----------

// btrt: buffer size, max bitrate, avg bitrate
pub struct BtrtDecoder;

impl BoxDecoder for BtrtDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buffer_size = r.read_u32_be()?;
        let max_bitrate = r.read_u32_be()?;
        let avg_bitrate = r.read_u32_be()?;
        Ok(BoxValue::Text(format!(
            "buffer_size={} max_bitrate={} avg_bitrate={}",
            buffer_size, max_bitrate, avg_bitrate
        )))
    }
}

// esds: elementary stream descriptor
pub struct EsdsDecoder;

impl BoxDecoder for EsdsDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut pos = 0;

        // ES_Descriptor tag = 0x03
        if pos >= buf.len() || buf[pos] != 0x03 {
            return Ok(BoxValue::Text("esds: no ES_Descriptor".into()));
        }
        pos += 1;
        if read_descriptor_length(&buf, &mut pos).is_none() {
            return Ok(BoxValue::Text("esds: truncated".into()));
        }
        // ES_ID (2) + flags (1)
        if pos + 3 > buf.len() {
            return Ok(BoxValue::Text("esds: truncated ES_ID".into()));
        }
        pos += 2;
        let stream_flags = buf[pos];
        pos += 1;
        if stream_flags & 0x80 != 0 {
            pos += 2;
        } // streamDependenceFlag
        if stream_flags & 0x40 != 0 {
            if pos >= buf.len() {
                return Ok(BoxValue::Text("esds: truncated URL".into()));
            }
            let url_len = buf[pos] as usize;
            pos += 1 + url_len;
        }
        if stream_flags & 0x20 != 0 {
            pos += 2;
        } // OCRstreamFlag

        // DecoderConfigDescriptor tag = 0x04
        if pos >= buf.len() || buf[pos] != 0x04 {
            return Ok(BoxValue::Text("esds: no DecoderConfigDescriptor".into()));
        }
        pos += 1;
        if read_descriptor_length(&buf, &mut pos).is_none() {
            return Ok(BoxValue::Text("esds: truncated DecoderConfig".into()));
        }
        if pos >= buf.len() {
            return Ok(BoxValue::Text(
                "esds: truncated objectTypeIndication".into(),
            ));
        }
        let object_type = buf[pos];
        pos += 1;

        // streamType(1) + bufferSizeDB(3) + maxBitrate(4) + avgBitrate(4)
        if pos + 12 > buf.len() {
            return Ok(BoxValue::Text(format!(
                "esds: objectType=0x{:02X}",
                object_type
            )));
        }
        pos += 4; // skip streamType byte + bufferSizeDB
        let max_bitrate = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let avg_bitrate = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap());
        pos += 4;

        let type_name = match object_type {
            0x40 => "MPEG-4 Audio",
            0x66..=0x68 => "MPEG-2 AAC",
            0x69 => "MPEG-2 Audio",
            0x6B => "MP3",
            0x20 => "MPEG-4 Visual",
            0x21 => "H.264/AVC",
            0x60..=0x65 => "MPEG-2 Visual",
            _ => "unknown",
        };

        // DecoderSpecificInfo tag = 0x05: for MPEG-4 audio this holds the
        // AudioSpecificConfig — the authoritative profile/rate/channels.
        let mut audio_config = None;
        if pos < buf.len() && buf[pos] == 0x05 {
            pos += 1;
            if let Some(len) = read_descriptor_length(&buf, &mut pos) {
                let end = (pos + len as usize).min(buf.len());
                let is_mpeg4_audio = object_type == 0x40 || (0x66..=0x68).contains(&object_type);
                if is_mpeg4_audio {
                    audio_config = parse_audio_specific_config(&buf[pos..end]);
                }
            }
        }

        Ok(BoxValue::Structured(StructuredData::ElementaryStream(
            EsdsData {
                version: _version.unwrap_or(0),
                flags: _flags.unwrap_or(0),
                object_type,
                object_type_name: type_name.to_string(),
                max_bitrate,
                avg_bitrate,
                audio_config,
            },
        )))
    }
}

// iods: Object Descriptor Box (ISO/IEC 14496-14 §5.1), wrapping an MPEG-4
// InitialObjectDescriptor (ISO/IEC 14496-1 §7.2.6.4).
pub struct IodsDecoder;

impl BoxDecoder for IodsDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // iods is a FullBox: version/flags are stripped by the parser, so the
        // payload starts at the descriptor tag.
        let buf = read_all(r)?;
        let mut pos = 0usize;

        // Descriptor tag: InitialObjectDescrTag (0x02) or the MP4 IOD tag
        // (0x10); both carry the profile-level layout below. A plain
        // ObjectDescrTag (0x01 / 0x11) omits the inline-profile flag and the
        // five profile-level bytes.
        let tag = match buf.first() {
            Some(&t) => t,
            None => return Ok(BoxValue::Bytes(buf)),
        };
        pos += 1;
        // Consume the expandable descriptor length (value itself unused: we
        // parse against the box bounds, and streams in the wild disagree on
        // whether it counts the sub-descriptors).
        let _len = read_descriptor_length(&buf, &mut pos);
        let is_iod = matches!(tag, 0x02 | 0x10);

        // ObjectDescriptorID (10 bits) + URL_Flag (1) + includeInline (1, IOD
        // only) + reserved.
        if pos + 2 > buf.len() {
            return Ok(BoxValue::Bytes(buf));
        }
        let b0 = buf[pos];
        let b1 = buf[pos + 1];
        pos += 2;
        let od_id = ((b0 as u16) << 2) | ((b1 as u16) >> 6);
        let url_flag = (b1 >> 5) & 0x01 != 0;
        let include_inline_profiles = is_iod && (b1 >> 4) & 0x01 != 0;

        let mut url = None;
        // 0xFF = "no profile required"; used as the default when the field is
        // absent (URL form) or truncated.
        let (mut od_p, mut scene_p, mut audio_p, mut visual_p, mut graphics_p) =
            (0xFFu8, 0xFFu8, 0xFFu8, 0xFFu8, 0xFFu8);

        if url_flag {
            if pos < buf.len() {
                let url_len = buf[pos] as usize;
                pos += 1;
                let end = (pos + url_len).min(buf.len());
                url = Some(String::from_utf8_lossy(&buf[pos..end]).to_string());
            }
        } else if is_iod && pos + 5 <= buf.len() {
            od_p = buf[pos];
            scene_p = buf[pos + 1];
            audio_p = buf[pos + 2];
            visual_p = buf[pos + 3];
            graphics_p = buf[pos + 4];
        }

        Ok(BoxValue::Structured(StructuredData::ObjectDescriptor(
            IodsData {
                version: version.unwrap_or(0),
                flags: flags.unwrap_or(0),
                od_id,
                include_inline_profiles,
                url,
                od_profile_level: od_p,
                scene_profile_level: scene_p,
                audio_profile_level: audio_p,
                visual_profile_level: visual_p,
                graphics_profile_level: graphics_p,
            },
        )))
    }
}

// avcC: AVC decoder configuration record
pub struct AvccDecoder;

impl BoxDecoder for AvccDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 5 {
            return Ok(BoxValue::Bytes(buf));
        }
        let config_version = buf[0];
        let profile = buf[1];
        let profile_compat = buf[2];
        let level = buf[3];
        let nal_length_size = (buf[4] & 0x03) + 1;
        Ok(BoxValue::Text(format!(
            "configurationVersion={} profile={} compat=0x{:02X} level={}.{} nal_length_size={}",
            config_version,
            profile,
            profile_compat,
            level / 10,
            level % 10,
            nal_length_size
        )))
    }
}

// hvcC: HEVC decoder configuration record
pub struct HvccDecoder;

impl BoxDecoder for HvccDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 13 {
            return Ok(BoxValue::Bytes(buf));
        }
        let config_version = buf[0];
        let profile_space = (buf[1] >> 6) & 0x03;
        let tier = (buf[1] >> 5) & 0x01;
        let profile_idc = buf[1] & 0x1F;
        let level_idc = buf[12];
        Ok(BoxValue::Text(format!(
            "configurationVersion={} profile_space={} tier={} profile_idc={} level_idc={} ({}.{})",
            config_version,
            profile_space,
            if tier == 1 { "high" } else { "main" },
            profile_idc,
            level_idc,
            level_idc / 30,
            (level_idc % 30) / 3
        )))
    }
}

// av1C: AV1 codec configuration
pub struct Av1cDecoder;

impl BoxDecoder for Av1cDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 4 {
            return Ok(BoxValue::Bytes(buf));
        }
        let marker_version = buf[0];
        let marker = (marker_version >> 7) & 1;
        let version = marker_version & 0x7F;
        let seq_profile = (buf[1] >> 5) & 0x07;
        let seq_level_idx_0 = buf[1] & 0x1F;
        let seq_tier_0 = (buf[2] >> 7) & 1;
        let high_bitdepth = (buf[2] >> 6) & 1;
        let twelve_bit = (buf[2] >> 5) & 1;
        let monochrome = (buf[2] >> 4) & 1;
        Ok(BoxValue::Text(format!(
            "marker={} version={} profile={} level_idx={} tier={} high_bitdepth={} twelve_bit={} monochrome={}",
            marker,
            version,
            seq_profile,
            seq_level_idx_0,
            seq_tier_0,
            high_bitdepth,
            twelve_bit,
            monochrome
        )))
    }
}

// dvcC / dvvC: Dolby Vision configuration record.
//
// DOVIDecoderConfigurationRecord (Dolby Vision streams spec, section 3.2):
//   u8  dv_version_major
//   u8  dv_version_minor
//   u7  dv_profile
//   u6  dv_level
//   u1  rpu_present_flag
//   u1  el_present_flag
//   u1  bl_present_flag
//   u4  dv_bl_signal_compatibility_id
//   (reserved padding to 24 bytes)
pub struct DvccDecoder;

/// Cross-compatibility of the base layer with non-Dolby-Vision decoders,
/// per `dv_bl_signal_compatibility_id`.
fn dv_compatibility_name(id: u8) -> &'static str {
    match id {
        0 => "none",
        1 => "HDR10 (BT.2020 PQ)",
        2 => "SDR (BT.709)",
        4 => "HLG (BT.2020)",
        6 => "BT.2100 (HDR10/HLG)",
        _ => "reserved",
    }
}

impl BoxDecoder for DvccDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 5 {
            return Ok(BoxValue::Bytes(buf));
        }
        let compat_id = buf[4] >> 4;
        Ok(BoxValue::Structured(StructuredData::DolbyVisionConfig(
            DoviConfigData {
                dv_version_major: buf[0],
                dv_version_minor: buf[1],
                dv_profile: buf[2] >> 1,
                dv_level: ((buf[2] & 0x01) << 5) | (buf[3] >> 3),
                rpu_present: (buf[3] >> 2) & 1 == 1,
                el_present: (buf[3] >> 1) & 1 == 1,
                bl_present: buf[3] & 1 == 1,
                bl_signal_compatibility_id: compat_id,
                bl_signal_compatibility: dv_compatibility_name(compat_id).to_string(),
            },
        )))
    }
}

// vpcC: VP codec configuration (FullBox version=1)
pub struct VpccDecoder;

impl BoxDecoder for VpccDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let profile = r.read_u8()?;
        let level = r.read_u8()?;
        let byte3 = r.read_u8()?;
        let bit_depth = (byte3 >> 4) & 0x0F;
        let chroma_subsampling = (byte3 >> 1) & 0x07;
        let full_range = byte3 & 0x01;
        let colour_primaries = r.read_u8()? as u16;
        let transfer = r.read_u8()? as u16;
        let matrix = r.read_u8()? as u16;
        Ok(BoxValue::Text(format!(
            "profile={} level={} bit_depth={} chroma={} full_range={} primaries={} transfer={} matrix={}",
            profile,
            level,
            bit_depth,
            chroma_subsampling,
            full_range,
            cicp::labeled(colour_primaries, cicp::primaries_name(colour_primaries)),
            cicp::labeled(transfer, cicp::transfer_name(transfer)),
            cicp::labeled(matrix, cicp::matrix_name(matrix))
        )))
    }
}

// dOps: Opus specific box
pub struct DopsDecoder;

impl BoxDecoder for DopsDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let version = r.read_u8()?;
        let channels = r.read_u8()?;
        let pre_skip = r.read_u16_be()?;
        let input_sample_rate = r.read_u32_le()?; // native Ogg byte order
        let output_gain = r.read_i16_be()?;
        let mapping_family = r.read_u8()?;
        Ok(BoxValue::Text(format!(
            "version={} channels={} pre_skip={} input_sample_rate={} output_gain={} mapping_family={}",
            version, channels, pre_skip, input_sample_rate, output_gain, mapping_family
        )))
    }
}

// dac3: AC-3 bitstream information
pub struct Dac3Decoder;

impl BoxDecoder for Dac3Decoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 3 {
            return Ok(BoxValue::Bytes(buf));
        }
        let b0 = buf[0] as u32;
        let b1 = buf[1] as u32;
        let b2 = buf[2] as u32;
        let fscod = (b0 >> 6) & 0x03;
        let bsid = (b0 >> 1) & 0x1F;
        let bsmod = ((b0 & 0x01) << 2) | ((b1 >> 6) & 0x03);
        let acmod = (b1 >> 3) & 0x07;
        let lfeon = (b1 >> 2) & 0x01;
        let bit_rate_code = ((b1 & 0x03) << 3) | ((b2 >> 5) & 0x07);
        let sample_rates = [48000u32, 44100, 32000, 0];
        let sample_rate = sample_rates[fscod as usize];
        Ok(BoxValue::Text(format!(
            "fscod={} bsid={} bsmod={} acmod={} lfeon={} bit_rate_code={} sample_rate={}",
            fscod, bsid, bsmod, acmod, lfeon, bit_rate_code, sample_rate
        )))
    }
}

// dec3: Enhanced AC-3 bitstream information
pub struct Dec3Decoder;

impl BoxDecoder for Dec3Decoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 2 {
            return Ok(BoxValue::Bytes(buf));
        }
        let word = ((buf[0] as u16) << 8) | buf[1] as u16;
        let data_rate = (word >> 3) & 0x1FFF;
        let num_ind_sub = (word & 0x07) + 1;
        Ok(BoxValue::Text(format!(
            "data_rate={}kbps num_independent_substreams={}",
            data_rate, num_ind_sub
        )))
    }
}

// dfLa: FLAC specific box (FullBox, then METADATA_BLOCK_HEADER + STREAMINFO)
pub struct DflaDecoder;

impl BoxDecoder for DflaDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 4 {
            return Ok(BoxValue::Bytes(buf));
        }
        let block_type = buf[0] & 0x7F;
        if block_type != 0 || buf.len() < 4 + 34 {
            return Ok(BoxValue::Text(format!(
                "FLAC block_type={} ({} bytes)",
                block_type,
                buf.len().saturating_sub(4)
            )));
        }
        // STREAMINFO starts at buf[4]
        let s = &buf[4..];
        let sample_rate = ((s[10] as u32) << 12) | ((s[11] as u32) << 4) | ((s[12] as u32) >> 4);
        let channels = ((s[12] >> 1) & 0x07) + 1;
        let bits_per_sample = (((s[12] & 0x01) << 4) | (s[13] >> 4)) + 1;
        let total_samples = (((s[13] & 0x0F) as u64) << 32)
            | ((s[14] as u64) << 24)
            | ((s[15] as u64) << 16)
            | ((s[16] as u64) << 8)
            | (s[17] as u64);
        Ok(BoxValue::Text(format!(
            "sample_rate={} channels={} bits_per_sample={} total_samples={}",
            sample_rate, channels, bits_per_sample, total_samples
        )))
    }
}

// colr: colour information
pub struct ColrDecoder;

impl BoxDecoder for ColrDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 4 {
            return Ok(BoxValue::Bytes(buf));
        }
        let colour_type = &buf[0..4];
        let type_str = String::from_utf8_lossy(colour_type).to_string();
        if colour_type == b"nclx" && buf.len() >= 11 {
            let primaries = u16::from_be_bytes(buf[4..6].try_into().unwrap());
            let transfer = u16::from_be_bytes(buf[6..8].try_into().unwrap());
            let matrix = u16::from_be_bytes(buf[8..10].try_into().unwrap());
            let full_range = (buf[10] >> 7) & 1 == 1;
            Ok(BoxValue::Structured(StructuredData::ColourInformation(
                ColrData {
                    colour_type: type_str,
                    primaries: Some(primaries),
                    primaries_name: cicp::primaries_name(primaries).map(str::to_string),
                    transfer: Some(transfer),
                    transfer_name: cicp::transfer_name(transfer).map(str::to_string),
                    matrix: Some(matrix),
                    matrix_name: cicp::matrix_name(matrix).map(str::to_string),
                    full_range: Some(full_range),
                },
            )))
        } else {
            Ok(BoxValue::Structured(StructuredData::ColourInformation(
                ColrData {
                    colour_type: type_str,
                    primaries: None,
                    primaries_name: None,
                    transfer: None,
                    transfer_name: None,
                    matrix: None,
                    matrix_name: None,
                    full_range: None,
                },
            )))
        }
    }
}

// pasp: pixel aspect ratio
pub struct PaspDecoder;

impl BoxDecoder for PaspDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let h = r.read_u32_be()?;
        let v = r.read_u32_be()?;
        Ok(BoxValue::Text(format!("h_spacing={} v_spacing={}", h, v)))
    }
}

// mdcv: mastering display color volume
pub struct MdcvDecoder;

impl BoxDecoder for MdcvDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 24 {
            return Ok(BoxValue::Bytes(buf));
        }
        let rx = u16::from_be_bytes(buf[0..2].try_into().unwrap());
        let ry = u16::from_be_bytes(buf[2..4].try_into().unwrap());
        let gx = u16::from_be_bytes(buf[4..6].try_into().unwrap());
        let gy = u16::from_be_bytes(buf[6..8].try_into().unwrap());
        let bx = u16::from_be_bytes(buf[8..10].try_into().unwrap());
        let by_ = u16::from_be_bytes(buf[10..12].try_into().unwrap());
        let wx = u16::from_be_bytes(buf[12..14].try_into().unwrap());
        let wy = u16::from_be_bytes(buf[14..16].try_into().unwrap());
        let max_lum = u32::from_be_bytes(buf[16..20].try_into().unwrap());
        let min_lum = u32::from_be_bytes(buf[20..24].try_into().unwrap());
        Ok(BoxValue::Structured(
            StructuredData::MasteringDisplayColourVolume(MdcvData {
                red_x: rx,
                red_y: ry,
                green_x: gx,
                green_y: gy,
                blue_x: bx,
                blue_y: by_,
                white_x: wx,
                white_y: wy,
                max_display_mastering_luminance: max_lum as f64 / 10000.0,
                min_display_mastering_luminance: min_lum as f64 / 10000.0,
            }),
        ))
    }
}

// clli: content light level information
pub struct ClliDecoder;

impl BoxDecoder for ClliDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let max_cll = r.read_u16_be()?;
        let max_fall = r.read_u16_be()?;
        Ok(BoxValue::Structured(StructuredData::ContentLightLevel(
            ClliData {
                max_content_light_level: max_cll,
                max_pic_average_light_level: max_fall,
            },
        )))
    }
}

// kind: track kind (FullBox; version/flags pre-stripped)
pub struct KindDecoder;

impl BoxDecoder for KindDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut parts = buf.splitn(3, |&b| b == 0);
        let scheme = parts
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_default();
        let value = parts
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_default();
        Ok(BoxValue::Text(format!(
            "scheme_uri={:?} value={:?}",
            scheme, value
        )))
    }
}

// irot: image rotation
pub struct IrotDecoder;

impl BoxDecoder for IrotDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let byte = r.read_u8()?;
        let degrees = (byte & 0x03) * 90;
        Ok(BoxValue::Text(format!("angle={}°", degrees)))
    }
}

// imir: image mirror
pub struct ImirDecoder;

impl BoxDecoder for ImirDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let byte = r.read_u8()?;
        let axis = if byte & 0x01 == 0 {
            "vertical (top-bottom)"
        } else {
            "horizontal (left-right)"
        };
        Ok(BoxValue::Text(format!("axis={}", axis)))
    }
}

// data: iTunes metadata value (FullBox; flags = type_indicator)
pub struct IlstDataDecoder;

impl BoxDecoder for IlstDataDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let type_code = flags.unwrap_or(0) & 0x00FFFFFF;
        let _locale = r.read_u32_be()?;
        let buf = read_all(r)?;
        match type_code {
            1 => Ok(BoxValue::Text(format!(
                "type=utf8 value={:?}",
                String::from_utf8_lossy(&buf)
            ))),
            13 => Ok(BoxValue::Text(format!("type=jpeg ({} bytes)", buf.len()))),
            14 => Ok(BoxValue::Text(format!("type=png ({} bytes)", buf.len()))),
            21 => {
                let v: i64 = match buf.len() {
                    1 => buf[0] as i8 as i64,
                    2 => i16::from_be_bytes([buf[0], buf[1]]) as i64,
                    4 => i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as i64,
                    8 => i64::from_be_bytes(buf[..8].try_into().unwrap_or([0; 8])),
                    _ => 0,
                };
                Ok(BoxValue::Text(format!("type=int value={}", v)))
            }
            22 => {
                let v: u64 = match buf.len() {
                    1 => buf[0] as u64,
                    2 => u16::from_be_bytes([buf[0], buf[1]]) as u64,
                    4 => u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64,
                    8 => u64::from_be_bytes(buf[..8].try_into().unwrap_or([0; 8])),
                    _ => 0,
                };
                Ok(BoxValue::Text(format!("type=uint value={}", v)))
            }
            _ => Ok(BoxValue::Text(format!(
                "type_code={} ({} bytes)",
                type_code,
                buf.len()
            ))),
        }
    }
}

// mean: iTunes reverse DNS domain (FullBox; version/flags pre-stripped)
pub struct MeanDecoder;

impl BoxDecoder for MeanDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        Ok(BoxValue::Text(format!(
            "domain={:?}",
            String::from_utf8_lossy(&buf)
        )))
    }
}

// name: iTunes reverse DNS name (FullBox; version/flags pre-stripped)
pub struct IlstNameDecoder;

impl BoxDecoder for IlstNameDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        Ok(BoxValue::Text(format!(
            "name={:?}",
            String::from_utf8_lossy(&buf)
        )))
    }
}

// trun: track fragment run (FullBox)
pub struct TrunDecoder;

impl BoxDecoder for TrunDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let ver = version.unwrap_or(0);
        let fl = flags.unwrap_or(0);

        let sample_count = r.read_u32_be()?;
        let data_offset = if fl & 0x000001 != 0 {
            Some(r.read_i32_be()?)
        } else {
            None
        };
        let first_sample_flags = if fl & 0x000004 != 0 {
            Some(r.read_u32_be()?)
        } else {
            None
        };

        let mut samples = Vec::new();
        for _ in 0..sample_count {
            let duration = if fl & 0x000100 != 0 {
                Some(r.read_u32_be()?)
            } else {
                None
            };
            let size = if fl & 0x000200 != 0 {
                Some(r.read_u32_be()?)
            } else {
                None
            };
            let sflags = if fl & 0x000400 != 0 {
                Some(r.read_u32_be()?)
            } else {
                None
            };
            let cto = if fl & 0x000800 != 0 {
                if ver == 1 {
                    Some(r.read_i32_be()?)
                } else {
                    Some(r.read_u32_be()? as i32)
                }
            } else {
                None
            };
            samples.push(TrunSample {
                duration,
                size,
                flags: sflags,
                composition_time_offset: cto,
            });
        }

        Ok(BoxValue::Structured(StructuredData::TrackFragmentRun(
            TrunData {
                version: ver,
                flags: fl,
                sample_count,
                data_offset,
                first_sample_flags,
                samples,
            },
        )))
    }

    fn field_spans(
        &self,
        _version: Option<u8>,
        flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        // The per-sample array is omitted; only the header fields, which are
        // flag-gated like decode() above.
        let fl = flags.unwrap_or(0);
        let mut pos = 0u64;
        let mut spans = vec![span("sample_count", 4, &mut pos)];
        if fl & 0x000001 != 0 {
            spans.push(span("data_offset", 4, &mut pos));
        }
        if fl & 0x000004 != 0 {
            spans.push(span("first_sample_flags", 4, &mut pos));
        }
        spans
    }
}

// tfhd: track fragment header (FullBox)
pub struct TfhdDecoder;

impl BoxDecoder for TfhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let fl = flags.unwrap_or(0);
        let track_id = r.read_u32_be()?;
        let base_data_offset = if fl & 0x000001 != 0 {
            Some(r.read_u64_be()?)
        } else {
            None
        };
        let sample_description_index = if fl & 0x000002 != 0 {
            Some(r.read_u32_be()?)
        } else {
            None
        };
        let default_sample_duration = if fl & 0x000008 != 0 {
            Some(r.read_u32_be()?)
        } else {
            None
        };
        let default_sample_size = if fl & 0x000010 != 0 {
            Some(r.read_u32_be()?)
        } else {
            None
        };
        let default_sample_flags = if fl & 0x000020 != 0 {
            Some(r.read_u32_be()?)
        } else {
            None
        };

        Ok(BoxValue::Structured(StructuredData::TrackFragmentHeader(
            TfhdData {
                version: version.unwrap_or(0),
                flags: fl,
                track_id,
                base_data_offset,
                sample_description_index,
                default_sample_duration,
                default_sample_size,
                default_sample_flags,
                duration_is_empty: fl & 0x010000 != 0,
                default_base_is_moof: fl & 0x020000 != 0,
            },
        )))
    }

    fn field_spans(
        &self,
        _version: Option<u8>,
        flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        // Optional fields are present only when their flag bit is set; widths
        // and order mirror decode() above.
        let fl = flags.unwrap_or(0);
        let mut pos = 0u64;
        let mut spans = vec![span("track_id", 4, &mut pos)];
        if fl & 0x000001 != 0 {
            spans.push(span("base_data_offset", 8, &mut pos));
        }
        if fl & 0x000002 != 0 {
            spans.push(span("sample_description_index", 4, &mut pos));
        }
        if fl & 0x000008 != 0 {
            spans.push(span("default_sample_duration", 4, &mut pos));
        }
        if fl & 0x000010 != 0 {
            spans.push(span("default_sample_size", 4, &mut pos));
        }
        if fl & 0x000020 != 0 {
            spans.push(span("default_sample_flags", 4, &mut pos));
        }
        spans
    }
}

// tfdt: track fragment decode time (FullBox)
pub struct TfdtDecoder;

impl BoxDecoder for TfdtDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let version = version.unwrap_or(0);
        let base_media_decode_time = if version == 1 {
            r.read_u64_be()?
        } else {
            r.read_u32_be()? as u64
        };
        Ok(BoxValue::Structured(
            StructuredData::TrackFragmentDecodeTime(TfdtData {
                version,
                flags: flags.unwrap_or(0),
                base_media_decode_time,
            }),
        ))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        let width: u64 = if version == Some(1) { 8 } else { 4 };
        let mut pos = 0u64;
        vec![span("base_media_decode_time", width, &mut pos)]
    }
}

// trex: track extends (FullBox)
pub struct TrexDecoder;

impl BoxDecoder for TrexDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        Ok(BoxValue::Structured(StructuredData::TrackExtends(
            TrexData {
                track_id: r.read_u32_be()?,
                default_sample_description_index: r.read_u32_be()?,
                default_sample_duration: r.read_u32_be()?,
                default_sample_size: r.read_u32_be()?,
                default_sample_flags: r.read_u32_be()?,
            },
        )))
    }

    fn field_spans(
        &self,
        _version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        let mut pos = 0u64;
        vec![
            span("track_id", 4, &mut pos),
            span("default_sample_description_index", 4, &mut pos),
            span("default_sample_duration", 4, &mut pos),
            span("default_sample_size", 4, &mut pos),
            span("default_sample_flags", 4, &mut pos),
        ]
    }
}

// ---------- DRM / DASH decoders ----------

pub(crate) fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn uuid_string(bytes: &[u8; 16]) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        hex_string(&bytes[0..4]),
        hex_string(&bytes[4..6]),
        hex_string(&bytes[6..8]),
        hex_string(&bytes[8..10]),
        hex_string(&bytes[10..16])
    )
}

/// Well-known DRM system IDs (ISO 23001-7 pssh SystemID registry).
pub(crate) fn drm_system_name(system_id: &[u8; 16]) -> Option<&'static str> {
    match *system_id {
        crate::drm::WIDEVINE_SYSTEM_ID => Some("Widevine"),
        crate::drm::PLAYREADY_SYSTEM_ID => Some("PlayReady"),
        [
            0x94,
            0xCE,
            0x86,
            0xFB,
            0x07,
            0xFF,
            0x4F,
            0x43,
            0xAD,
            0xB8,
            0x93,
            0xD2,
            0xFA,
            0x96,
            0x8C,
            0xA2,
        ] => Some("FairPlay"),
        [
            0x10,
            0x77,
            0xEF,
            0xEC,
            0xC0,
            0xB2,
            0x4D,
            0x02,
            0xAC,
            0xE3,
            0x3C,
            0x1E,
            0x52,
            0xE2,
            0xFB,
            0x4B,
        ] => Some("Common PSSH (ClearKey)"),
        [
            0x5E,
            0x62,
            0x9A,
            0xF5,
            0x38,
            0xDA,
            0x40,
            0x63,
            0x89,
            0x77,
            0x97,
            0xFF,
            0xBD,
            0x99,
            0x02,
            0xD4,
        ] => Some("Marlin"),
        _ => None,
    }
}

// pssh: protection system specific header (FullBox)
pub struct PsshDecoder;

impl BoxDecoder for PsshDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        // The reader is bounded to the box payload, so this also picks up the
        // system-specific data blob for crate::drm to decode.
        let body = read_all(r)?;
        Ok(BoxValue::Structured(
            StructuredData::ProtectionSystemHeader(crate::drm::parse_pssh_body(
                version.unwrap_or(0),
                flags.unwrap_or(0),
                &body,
            )?),
        ))
    }
}

// tenc: track encryption (FullBox)
pub struct TencDecoder;

impl BoxDecoder for TencDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let version = version.unwrap_or(0);
        let _reserved = r.read_u8()?;
        // In version 0 this byte is reserved; in version 1+ it packs the
        // cbcs pattern (crypt:skip blocks).
        let pattern = r.read_u8()?;
        let (crypt, skip) = if version >= 1 {
            (pattern >> 4, pattern & 0x0F)
        } else {
            (0, 0)
        };
        let is_protected = r.read_u8()? != 0;
        let per_sample_iv_size = r.read_u8()?;
        let mut kid = [0u8; 16];
        r.read_exact(&mut kid)?;

        let constant_iv = if is_protected && per_sample_iv_size == 0 {
            let iv_size = r.read_u8()? as usize;
            let mut iv = vec![0u8; iv_size];
            r.read_exact(&mut iv)?;
            Some(hex_string(&iv))
        } else {
            None
        };

        Ok(BoxValue::Structured(StructuredData::TrackEncryption(
            TencData {
                version,
                flags: flags.unwrap_or(0),
                default_crypt_byte_block: crypt,
                default_skip_byte_block: skip,
                default_is_protected: is_protected,
                default_per_sample_iv_size: per_sample_iv_size,
                default_kid: hex_string(&kid),
                default_constant_iv: constant_iv,
            },
        )))
    }

    fn field_spans(
        &self,
        _version: Option<u8>,
        _flags: Option<u32>,
        payload_len: u64,
    ) -> Vec<FieldSpan> {
        // reserved (1) + crypt/skip pattern byte (1) then the fixed fields. The
        // trailing constant_iv is conditional on values only known by reading,
        // so it is omitted.
        if payload_len < 20 {
            return Vec::new();
        }
        vec![
            FieldSpan {
                name: "default_is_protected".into(),
                start: 2,
                length: 1,
            },
            FieldSpan {
                name: "default_per_sample_iv_size".into(),
                start: 3,
                length: 1,
            },
            FieldSpan {
                name: "default_kid".into(),
                start: 4,
                length: 16,
            },
        ]
    }
}

// senc: sample encryption (FullBox). Per-sample IVs cannot be parsed without
// the IV size from tenc, so only the summary fields are decoded.
pub struct SencDecoder;

impl BoxDecoder for SencDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let fl = flags.unwrap_or(0);
        let sample_count = r.read_u32_be()?;
        Ok(BoxValue::Text(format!(
            "sample_count={} subsamples={} (per-sample IVs sized by tenc)",
            sample_count,
            fl & 0x2 != 0
        )))
    }
}

// schm: scheme type (FullBox) — which protection scheme applies (cenc/cbcs/...)
pub struct SchmDecoder;

impl BoxDecoder for SchmDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let mut scheme = [0u8; 4];
        r.read_exact(&mut scheme)?;
        let version = r.read_u32_be()?;
        Ok(BoxValue::Text(format!(
            "scheme={} version={}.{}",
            String::from_utf8_lossy(&scheme),
            version >> 16,
            version & 0xFFFF
        )))
    }
}

// frma: original (pre-encryption) sample entry format
pub struct FrmaDecoder;

impl BoxDecoder for FrmaDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let mut fourcc = [0u8; 4];
        r.read_exact(&mut fourcc)?;
        Ok(BoxValue::Text(format!(
            "original_format={}",
            String::from_utf8_lossy(&fourcc)
        )))
    }
}

// emsg: DASH event message (FullBox)
pub struct EmsgDecoder;

fn read_cstring(r: &mut dyn Read) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    loop {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        if b[0] == 0 {
            break;
        }
        bytes.push(b[0]);
        anyhow::ensure!(bytes.len() <= 4096, "unterminated string in emsg");
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

impl BoxDecoder for EmsgDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let version = version.unwrap_or(0);

        let data = if version == 0 {
            let scheme_id_uri = read_cstring(r)?;
            let value = read_cstring(r)?;
            let timescale = r.read_u32_be()?;
            let presentation_time_delta = r.read_u32_be()?;
            let event_duration = r.read_u32_be()?;
            let id = r.read_u32_be()?;
            let consumed = scheme_id_uri.len() + value.len() + 2 + 16;
            EmsgData {
                version,
                flags: flags.unwrap_or(0),
                scheme_id_uri,
                value,
                timescale,
                presentation_time: None,
                presentation_time_delta: Some(presentation_time_delta),
                event_duration,
                id,
                message_size: payload_len_after(hdr, consumed as u64),
            }
        } else {
            let timescale = r.read_u32_be()?;
            let presentation_time = r.read_u64_be()?;
            let event_duration = r.read_u32_be()?;
            let id = r.read_u32_be()?;
            let scheme_id_uri = read_cstring(r)?;
            let value = read_cstring(r)?;
            let consumed = 20 + scheme_id_uri.len() + value.len() + 2;
            EmsgData {
                version,
                flags: flags.unwrap_or(0),
                scheme_id_uri,
                value,
                timescale,
                presentation_time: Some(presentation_time),
                presentation_time_delta: None,
                event_duration,
                id,
                message_size: payload_len_after(hdr, consumed as u64),
            }
        };

        Ok(BoxValue::Structured(StructuredData::EventMessage(data)))
    }

    fn field_spans(
        &self,
        version: Option<u8>,
        _flags: Option<u32>,
        _payload_len: u64,
    ) -> Vec<FieldSpan> {
        // Only version 1 has its fixed fields first; version 0 leads with
        // variable-length strings, so its field offsets aren't known here.
        if version != Some(1) {
            return Vec::new();
        }
        let mut pos = 0u64;
        vec![
            span("timescale", 4, &mut pos),
            span("presentation_time", 8, &mut pos),
            span("event_duration", 4, &mut pos),
            span("id", 4, &mut pos),
        ]
    }
}

/// Remaining message bytes after `consumed` bytes of fixed fields, given
/// the box header (payload excludes header and version/flags).
fn payload_len_after(hdr: &BoxHeader, consumed: u64) -> u64 {
    hdr.size
        .saturating_sub(hdr.header_size)
        .saturating_sub(4) // version/flags
        .saturating_sub(consumed)
}
