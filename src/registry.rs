use crate::boxes::{BoxHeader, BoxKey, FourCC};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// A value returned from a box decoder.
///
/// Decoders may return either a human-readable text summary, raw bytes, or structured data.
#[derive(Debug, Clone)]
pub enum BoxValue {
    Text(String),
    Bytes(Vec<u8>),
    Structured(StructuredData),
}

/// Structured data for sample table boxes
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StructuredData {
    /// Sample Description Box (stsd)
    SampleDescription(StsdData),
    /// Decoding Time-to-Sample Box (stts)
    DecodingTimeToSample(SttsData),
    /// Composition Time-to-Sample Box (ctts)
    CompositionTimeToSample(CttsData),
    /// Sample-to-Chunk Box (stsc)
    SampleToChunk(StscData),
    /// Sample Size Box (stsz)
    SampleSize(StszData),
    /// Sync Sample Box (stss)
    SyncSample(StssData),
    /// Chunk Offset Box (stco)
    ChunkOffset(StcoData),
    /// 64-bit Chunk Offset Box (co64)
    ChunkOffset64(Co64Data),
    /// Media Header Box (mdhd)
    MediaHeader(MdhdData),
    /// Handler Reference Box (hdlr)
    HandlerReference(HdlrData),
    /// Track Header Box (tkhd)
    TrackHeader(TkhdData),
    /// Track Fragment Run Box (trun)
    TrackFragmentRun(TrunData),
}

/// Sample Description Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StsdData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub entries: Vec<SampleEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SampleEntry {
    pub size: u32,
    pub codec: String,
    pub data_reference_index: u16,
    pub width: Option<u16>,
    pub height: Option<u16>,
}

/// Decoding Time-to-Sample Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SttsData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub entries: Vec<SttsEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SttsEntry {
    pub sample_count: u32,
    pub sample_delta: u32,
}

/// Composition Time-to-Sample Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CttsData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub entries: Vec<CttsEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CttsEntry {
    pub sample_count: u32,
    pub sample_offset: i32, // Can be negative in version 1
}

/// Sample-to-Chunk Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StscData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub entries: Vec<StscEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StscEntry {
    pub first_chunk: u32,
    pub samples_per_chunk: u32,
    pub sample_description_index: u32,
}

/// Sample Size Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StszData {
    pub version: u8,
    pub flags: u32,
    pub sample_size: u32,
    pub sample_count: u32,
    pub sample_sizes: Vec<u32>, // Empty if sample_size > 0
}

/// Sync Sample Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StssData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub sample_numbers: Vec<u32>,
}

/// Chunk Offset Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StcoData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub chunk_offsets: Vec<u32>,
}

/// 64-bit Chunk Offset Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Co64Data {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub chunk_offsets: Vec<u64>,
}

/// Media Header Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MdhdData {
    pub version: u8,
    pub flags: u32,
    pub creation_time: u32,
    pub modification_time: u32,
    pub timescale: u32,
    pub duration: u32,
    pub language: String,
}

/// Handler Reference Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HdlrData {
    pub version: u8,
    pub flags: u32,
    pub handler_type: String,
    pub name: String,
}

/// Track Header Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TkhdData {
    pub version: u8,
    pub flags: u32,
    pub track_id: u32,
    pub duration: u64,
    pub width: f32,
    pub height: f32,
}

/// Track Fragment Run Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrunData {
    pub version: u8,
    pub flags: u32,
    pub sample_count: u32,
    pub data_offset: Option<i32>,
    pub first_sample_flags: Option<u32>,
    pub samples: Vec<TrunSample>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrunSample {
    pub duration: Option<u32>,
    pub size: Option<u32>,
    pub flags: Option<u32>,
    pub composition_time_offset: Option<i32>,
}

/// Trait for custom box decoders.
///
/// A decoder is responsible for interpreting the payload of a specific box
/// (identified by a [`BoxKey`]) and returning a [`BoxValue`].
pub trait BoxDecoder: Send + Sync {
    fn decode(
        &self,
        r: &mut dyn Read,
        hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue>;
}

/// Registry of decoders keyed by `BoxKey` (4CC or UUID).
///
/// The registry is immutable once constructed; use [`Registry::with_decoder`]
/// to build it fluently.
pub struct Registry {
    map: HashMap<BoxKey, BoxDecoderEntry>,
}

struct BoxDecoderEntry {
    inner: Box<dyn BoxDecoder>,
    _name: String,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Return a new registry with the given decoder added.
    ///
    /// `name` is human-readable and used only for debugging / logging.
    pub fn with_decoder(mut self, key: BoxKey, name: &str, dec: Box<dyn BoxDecoder>) -> Self {
        self.map.insert(
            key,
            BoxDecoderEntry {
                inner: dec,
                _name: name.to_string(),
            },
        );
        self
    }

    /// Try to decode the payload of a box using a registered decoder.
    ///
    /// Returns `None` if no decoder exists for the given key.
    pub fn decode(
        &self,
        key: &BoxKey,
        r: &mut dyn Read,
        hdr: &BoxHeader,
        version: Option<u8>,
        flags: Option<u32>,
    ) -> Option<anyhow::Result<BoxValue>> {
        self.map
            .get(key)
            .map(|d| d.inner.decode(r, hdr, version, flags))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Helpers ----------

fn read_all(r: &mut dyn Read) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
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
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        let version = cur.read_u8()?;
        let _flags = {
            let mut f = [0u8; 3];
            cur.read_exact(&mut f)?;
            ((f[0] as u32) << 16) | ((f[1] as u32) << 8) | (f[2] as u32)
        };

        let (timescale, duration) = if version == 1 {
            let _creation = cur.read_u64::<BigEndian>()?;
            let _mod = cur.read_u64::<BigEndian>()?;
            let ts = cur.read_u32::<BigEndian>()?;
            let dur = cur.read_u64::<BigEndian>()?;
            (ts, dur as u64)
        } else {
            let _creation = cur.read_u32::<BigEndian>()?;
            let _mod = cur.read_u32::<BigEndian>()?;
            let ts = cur.read_u32::<BigEndian>()?;
            let dur = cur.read_u32::<BigEndian>()? as u64;
            (ts, dur)
        };

        Ok(BoxValue::Text(format!(
            "timescale={} duration={}",
            timescale, duration
        )))
    }
}

// tkhd: track id, duration, width, height
pub struct TkhdDecoder;

impl BoxDecoder for TkhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        if buf.len() < 4 {
            return Ok(BoxValue::Text(format!(
                "tkhd: payload too short ({} bytes)",
                buf.len()
            )));
        }

        let mut pos = 0usize;
        let version = buf[pos];
        pos += 1;
        if pos + 3 > buf.len() {
            return Ok(BoxValue::Text("tkhd: truncated flags".into()));
        }

        // Extract flags as a 24-bit big-endian value
        let flags_bytes = [0, buf[pos], buf[pos + 1], buf[pos + 2]];
        let flags_value = u32::from_be_bytes(flags_bytes);
        pos += 3;

        let read_u32 = |pos: &mut usize| -> Option<u32> {
            if *pos + 4 > buf.len() {
                return None;
            }
            let v = u32::from_be_bytes(buf[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Some(v)
        };
        let read_u64 = |pos: &mut usize| -> Option<u64> {
            if *pos + 8 > buf.len() {
                return None;
            }
            let v = u64::from_be_bytes(buf[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Some(v)
        };

        let track_id;
        let duration;

        if version == 1 {
            // creation_time (8), modification_time (8), track_id (4), reserved (4), duration (8)
            if read_u64(&mut pos).is_none() || read_u64(&mut pos).is_none() {
                return Ok(BoxValue::Text(
                    "tkhd: truncated creation/modification".into(),
                ));
            }
            track_id = read_u32(&mut pos).unwrap_or(0);
            let _ = read_u32(&mut pos); // reserved
            duration = read_u64(&mut pos).unwrap_or(0);
            eprintln!(
                "DEBUG tkhd v1: track_id={}, duration={}",
                track_id, duration
            );
        } else {
            // For version 0, read two 8-byte timestamps then the fields
            let _creation_time = read_u64(&mut pos).unwrap_or(0);
            let _modification_time = read_u64(&mut pos).unwrap_or(0);

            track_id = read_u32(&mut pos).unwrap_or(0);
            let _reserved = read_u32(&mut pos).unwrap_or(0);
            duration = read_u32(&mut pos).unwrap_or(0) as u64;
        }

        // reserved[2]
        for _ in 0..2 {
            let _ = read_u32(&mut pos);
        }

        // layer/alt_group/volume/reserved (8 bytes)
        if pos + 8 <= buf.len() {
            pos += 8;
        } else {
            // we still have track/duration, just don't try width/height
            return Ok(BoxValue::Text(format!(
                "track_id={} duration={} (no width/height, short payload)",
                track_id, duration
            )));
        }

        // matrix (36 bytes)
        if pos + 36 <= buf.len() {
            pos += 36;
        } else {
            return Ok(BoxValue::Text(format!(
                "track_id={} duration={} (no width/height, short payload)",
                track_id, duration
            )));
        }

        // width / height
        let (width, height) = if pos + 8 <= buf.len() {
            let width = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap());
            let height = u32::from_be_bytes(buf[pos + 4..pos + 8].try_into().unwrap());
            (width as f32 / 65536.0, height as f32 / 65536.0)
        } else {
            (0.0, 0.0)
        };

        let data = TkhdData {
            version,
            flags: flags_value,
            track_id,
            duration,
            width,
            height,
        };

        Ok(BoxValue::Structured(StructuredData::TrackHeader(data)))
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
        let creation_time = r.read_u32::<BigEndian>()?;
        let modification_time = r.read_u32::<BigEndian>()?;
        let timescale = r.read_u32::<BigEndian>()?;
        let duration = r.read_u32::<BigEndian>()?;
        let language_code = r.read_u16::<BigEndian>()?;
        let _pre_defined = r.read_u16::<BigEndian>()?;

        let lang = lang_from_u16(language_code);

        let data = MdhdData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            creation_time,
            modification_time,
            timescale,
            duration,
            language: lang,
        };

        Ok(BoxValue::Structured(StructuredData::MediaHeader(data)))
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
        use byteorder::{BigEndian, ReadBytesExt};

        // pre_defined (4 bytes) + handler_type (4 bytes)
        let _pre_defined = r.read_u32::<BigEndian>()?;
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
}

// sidx: segment index summary
pub struct SidxDecoder;

impl BoxDecoder for SidxDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let buf = read_all(r)?;
        let mut cur = Cursor::new(&buf);

        let version = cur.read_u8()?;
        let _flags = {
            let mut f = [0u8; 3];
            cur.read_exact(&mut f)?;
            ((f[0] as u32) << 16) | ((f[1] as u32) << 8) | (f[2] as u32)
        };

        let _ref_id = cur.read_u32::<BigEndian>()?;
        let timescale = cur.read_u32::<BigEndian>()?;

        let (earliest, first_offset) = if version == 1 {
            let earliest = cur.read_u64::<BigEndian>()?;
            let first = cur.read_u64::<BigEndian>()?;
            (earliest, first)
        } else {
            let earliest = cur.read_u32::<BigEndian>()? as u64;
            let first = cur.read_u32::<BigEndian>()? as u64;
            (earliest, first)
        };

        let _reserved = cur.read_u16::<BigEndian>()?;
        let ref_count = cur.read_u16::<BigEndian>()?;

        Ok(BoxValue::Text(format!(
            "timescale={} earliest_presentation_time={} first_offset={} references={}",
            timescale, earliest, first_offset, ref_count
        )))
    }
}

// stsd: list sample entry formats, maybe WxH
// ---- stsd decoder: codec + width/height for first entry -----------------
pub struct StsdDecoder;

impl BoxDecoder for StsdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        use byteorder::{BigEndian, ReadBytesExt};

        // stsd is a FullBox; our reader is already positioned at payload:
        // u32 entry_count
        // [ SampleEntry entries... ]

        let entry_count = r.read_u32::<BigEndian>()?;
        if entry_count == 0 {
            return Ok(BoxValue::Text("entry_count=0".to_string()));
        }

        // First sample entry only (good enough for mp4info-like summary)
        let entry_size = r.read_u32::<BigEndian>()?;

        let mut codec_bytes = [0u8; 4];
        r.read_exact(&mut codec_bytes)?;
        let codec = std::str::from_utf8(&codec_bytes)
            .unwrap_or("????")
            .to_string();

        // Now we’re at SampleEntry fields.
        // For visual sample entries (avc1/hvc1/etc.), layout is:
        //
        // 6 reserved bytes
        // u16 data_reference_index
        // 16 bytes pre_defined / reserved
        // u16 width
        // u16 height
        //
        // For audio sample entries, this layout is different, so we only
        // try to read width/height for known video codecs.
        let visual_codecs = ["avc1", "hvc1", "hev1", "vp09", "av01"];

        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;

        if visual_codecs.contains(&codec.as_str()) {
            // Skip reserved + data_reference_index
            let mut skip = [0u8; 6 + 2 + 16];
            r.read_exact(&mut skip)?;

            let w = r.read_u16::<BigEndian>()?;
            let h = r.read_u16::<BigEndian>()?;
            width = Some(w as u32);
            height = Some(h as u32);
        }

        let mut parts = Vec::new();
        parts.push(format!("entry_count={}", entry_count));
        parts.push(format!("codec={}", codec));
        if let Some(w) = width {
            parts.push(format!("width={}", w));
        }
        if let Some(h) = height {
            parts.push(format!("height={}", h));
        }

        // Create structured data
        let data = StsdData {
            version: _version.unwrap_or(0),
            flags: _flags.unwrap_or(0),
            entry_count,
            entries: vec![SampleEntry {
                size: entry_size,
                codec,
                data_reference_index: 1, // Default value
                width: width.map(|w| w as u16),
                height: height.map(|h| h as u16),
            }],
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let sample_count = cur.read_u32::<BigEndian>()?;
            let sample_delta = cur.read_u32::<BigEndian>()?;
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut sample_numbers = Vec::new();

        for _ in 0..entry_count {
            sample_numbers.push(cur.read_u32::<BigEndian>()?);
        }

        let data = StssData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            sample_numbers,
        };

        Ok(BoxValue::Structured(StructuredData::SyncSample(data)))
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let sample_count = cur.read_u32::<BigEndian>()?;
            // Note: In version 1, sample_offset can be signed, but since we don't have access
            // to the parsed version here, we assume version 0 behavior (unsigned)
            let sample_offset = cur.read_u32::<BigEndian>()? as i32;
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let first_chunk = cur.read_u32::<BigEndian>()?;
            let samples_per_chunk = cur.read_u32::<BigEndian>()?;
            let sample_description_index = cur.read_u32::<BigEndian>()?;
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
        let sample_size = cur.read_u32::<BigEndian>()?;
        let sample_count = cur.read_u32::<BigEndian>()?;
        let mut sample_sizes = Vec::new();

        // If sample_size is 0, each sample has its own size
        if sample_size == 0 {
            for _ in 0..sample_count {
                sample_sizes.push(cur.read_u32::<BigEndian>()?);
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut chunk_offsets = Vec::new();

        for _ in 0..entry_count {
            chunk_offsets.push(cur.read_u32::<BigEndian>()?);
        }

        let data = StcoData {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            chunk_offsets,
        };

        Ok(BoxValue::Structured(StructuredData::ChunkOffset(data)))
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
        let entry_count = cur.read_u32::<BigEndian>()?;
        let mut chunk_offsets = Vec::new();

        for _ in 0..entry_count {
            chunk_offsets.push(cur.read_u64::<BigEndian>()?);
        }

        let data = Co64Data {
            version: version.unwrap_or(0),
            flags: flags.unwrap_or(0),
            entry_count,
            chunk_offsets,
        };

        Ok(BoxValue::Structured(StructuredData::ChunkOffset64(data)))
    }
}

// elst: edit list
pub struct ElstDecoder;

impl BoxDecoder for ElstDecoder {
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
                "elst: payload too short ({} bytes)",
                buf.len()
            )));
        }

        let mut pos = 0usize;
        let version = buf[pos];
        pos += 1;
        if pos + 3 > buf.len() {
            return Ok(BoxValue::Text("elst: truncated flags".into()));
        }
        pos += 3;

        let read_u32 = |pos: &mut usize| -> Option<u32> {
            if *pos + 4 > buf.len() {
                return None;
            }
            let v = u32::from_be_bytes(buf[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Some(v)
        };
        let read_u64 = |pos: &mut usize| -> Option<u64> {
            if *pos + 8 > buf.len() {
                return None;
            }
            let v = u64::from_be_bytes(buf[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Some(v)
        };
        let read_i32 = |pos: &mut usize| -> Option<i32> {
            if *pos + 4 > buf.len() {
                return None;
            }
            let v = i32::from_be_bytes(buf[*pos..*pos + 4].try_into().unwrap());
            *pos += 4;
            Some(v)
        };
        let read_i64 = |pos: &mut usize| -> Option<i64> {
            if *pos + 8 > buf.len() {
                return None;
            }
            let v = i64::from_be_bytes(buf[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Some(v)
        };
        let read_i16 = |pos: &mut usize| -> Option<i16> {
            if *pos + 2 > buf.len() {
                return None;
            }
            let v = i16::from_be_bytes(buf[*pos..*pos + 2].try_into().unwrap());
            *pos += 2;
            Some(v)
        };

        let entry_count = read_u32(&mut pos).unwrap_or(0);

        if entry_count == 0 {
            return Ok(BoxValue::Text(format!("version={} entries=0", version)));
        }

        let (seg_duration, media_time) = if version == 1 {
            let dur = read_u64(&mut pos).unwrap_or(0);
            let mt = read_i64(&mut pos).unwrap_or(0);
            (dur, mt)
        } else {
            let dur = read_u32(&mut pos).unwrap_or(0) as u64;
            let mt = read_i32(&mut pos).unwrap_or(0) as i64;
            (dur, mt)
        };

        let rate_int = read_i16(&mut pos);
        let rate_frac = read_i16(&mut pos);

        match (rate_int, rate_frac) {
            (Some(ri), Some(rf)) => Ok(BoxValue::Text(format!(
                "version={} entries={} first: duration={} media_time={} rate={}/{}",
                version, entry_count, seg_duration, media_time, ri, rf
            ))),
            _ => Ok(BoxValue::Text(format!(
                "version={} entries={} first: duration={} media_time={} (no rate, short payload)",
                version, entry_count, seg_duration, media_time
            ))),
        }
    }
}

// ---------- Helpers (continued) ----------

fn read_descriptor_length(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let mut length = 0u32;
    for _ in 0..4 {
        if *pos >= buf.len() {
            return None;
        }
        let b = buf[*pos];
        *pos += 1;
        length = (length << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            break;
        }
    }
    Some(length)
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
        let buffer_size = r.read_u32::<BigEndian>()?;
        let max_bitrate = r.read_u32::<BigEndian>()?;
        let avg_bitrate = r.read_u32::<BigEndian>()?;
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
        if pos + 8 > buf.len() {
            return Ok(BoxValue::Text(format!(
                "esds: objectType=0x{:02X}",
                object_type
            )));
        }
        pos += 4; // skip streamType byte + bufferSizeDB
        let max_bitrate = u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap());
        pos += 4;
        let avg_bitrate = if pos + 4 <= buf.len() {
            u32::from_be_bytes(buf[pos..pos + 4].try_into().unwrap())
        } else {
            0
        };

        let type_name = match object_type {
            0x40 => "AAC",
            0x66..=0x68 => "MPEG-4 Audio",
            0x69 => "MPEG-2 Audio",
            0x6B => "MP3",
            0x20 => "MPEG-4 Visual",
            0x21 => "H.264/AVC",
            0x60..=0x65 => "MPEG-2 Visual",
            _ => "unknown",
        };

        Ok(BoxValue::Text(format!(
            "objectType=0x{:02X} ({}) max_bitrate={} avg_bitrate={}",
            object_type, type_name, max_bitrate, avg_bitrate
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
        let colour_primaries = r.read_u8()?;
        let transfer = r.read_u8()?;
        let matrix = r.read_u8()?;
        Ok(BoxValue::Text(format!(
            "profile={} level={} bit_depth={} chroma={} full_range={} primaries={} transfer={} matrix={}",
            profile,
            level,
            bit_depth,
            chroma_subsampling,
            full_range,
            colour_primaries,
            transfer,
            matrix
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
        let pre_skip = r.read_u16::<BigEndian>()?;
        let input_sample_rate = r.read_u32::<LittleEndian>()?; // native Ogg byte order
        let output_gain = r.read_i16::<BigEndian>()?;
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
            let full_range = (buf[10] >> 7) & 1;
            Ok(BoxValue::Text(format!(
                "type=nclx primaries={} transfer={} matrix={} full_range={}",
                primaries, transfer, matrix, full_range
            )))
        } else {
            Ok(BoxValue::Text(format!(
                "type={} ({} bytes)",
                type_str,
                buf.len().saturating_sub(4)
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
        let h = r.read_u32::<BigEndian>()?;
        let v = r.read_u32::<BigEndian>()?;
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
        Ok(BoxValue::Text(format!(
            "R({},{}) G({},{}) B({},{}) W({},{}) max_luminance={:.4} min_luminance={:.4} cd/m2",
            rx,
            ry,
            gx,
            gy,
            bx,
            by_,
            wx,
            wy,
            max_lum as f64 / 10000.0,
            min_lum as f64 / 10000.0
        )))
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
        let max_cll = r.read_u16::<BigEndian>()?;
        let max_fall = r.read_u16::<BigEndian>()?;
        Ok(BoxValue::Text(format!(
            "max_cll={} max_fall={}",
            max_cll, max_fall
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
        let _locale = r.read_u32::<BigEndian>()?;
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

        let sample_count = r.read_u32::<BigEndian>()?;
        let data_offset = if fl & 0x000001 != 0 {
            Some(r.read_i32::<BigEndian>()?)
        } else {
            None
        };
        let first_sample_flags = if fl & 0x000004 != 0 {
            Some(r.read_u32::<BigEndian>()?)
        } else {
            None
        };

        let mut samples = Vec::new();
        for _ in 0..sample_count {
            let duration = if fl & 0x000100 != 0 {
                Some(r.read_u32::<BigEndian>()?)
            } else {
                None
            };
            let size = if fl & 0x000200 != 0 {
                Some(r.read_u32::<BigEndian>()?)
            } else {
                None
            };
            let sflags = if fl & 0x000400 != 0 {
                Some(r.read_u32::<BigEndian>()?)
            } else {
                None
            };
            let cto = if fl & 0x000800 != 0 {
                if ver == 1 {
                    Some(r.read_i32::<BigEndian>()?)
                } else {
                    Some(r.read_u32::<BigEndian>()? as i32)
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
}

// tfhd: track fragment header (FullBox)
pub struct TfhdDecoder;

impl BoxDecoder for TfhdDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let fl = flags.unwrap_or(0);
        let track_id = r.read_u32::<BigEndian>()?;
        let base_data_offset = if fl & 0x000001 != 0 {
            Some(r.read_u64::<BigEndian>()?)
        } else {
            None
        };
        let sample_description_index = if fl & 0x000002 != 0 {
            Some(r.read_u32::<BigEndian>()?)
        } else {
            None
        };
        let default_duration = if fl & 0x000008 != 0 {
            Some(r.read_u32::<BigEndian>()?)
        } else {
            None
        };
        let default_size = if fl & 0x000010 != 0 {
            Some(r.read_u32::<BigEndian>()?)
        } else {
            None
        };
        let default_flags = if fl & 0x000020 != 0 {
            Some(r.read_u32::<BigEndian>()?)
        } else {
            None
        };

        let mut parts = vec![format!("track_id={}", track_id)];
        if let Some(v) = base_data_offset {
            parts.push(format!("base_data_offset={}", v));
        }
        if let Some(v) = sample_description_index {
            parts.push(format!("sample_description_index={}", v));
        }
        if let Some(v) = default_duration {
            parts.push(format!("default_duration={}", v));
        }
        if let Some(v) = default_size {
            parts.push(format!("default_size={}", v));
        }
        if let Some(v) = default_flags {
            parts.push(format!("default_flags=0x{:08X}", v));
        }
        if fl & 0x010000 != 0 {
            parts.push("default_base_is_moof".into());
        }
        Ok(BoxValue::Text(parts.join(" ")))
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
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let base_decode_time = if version.unwrap_or(0) == 1 {
            r.read_u64::<BigEndian>()?
        } else {
            r.read_u32::<BigEndian>()? as u64
        };
        Ok(BoxValue::Text(format!(
            "base_media_decode_time={}",
            base_decode_time
        )))
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
        let track_id = r.read_u32::<BigEndian>()?;
        let default_sample_description_index = r.read_u32::<BigEndian>()?;
        let default_sample_duration = r.read_u32::<BigEndian>()?;
        let default_sample_size = r.read_u32::<BigEndian>()?;
        let default_sample_flags = r.read_u32::<BigEndian>()?;
        Ok(BoxValue::Text(format!(
            "track_id={} default_sample_description_index={} default_sample_duration={} default_sample_size={} default_sample_flags=0x{:08X}",
            track_id,
            default_sample_description_index,
            default_sample_duration,
            default_sample_size,
            default_sample_flags
        )))
    }
}

// ---------- Default registry ----------
pub fn default_registry() -> Registry {
    use crate::boxes::BoxKey;

    Registry::new()
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"ftyp")),
            "ftyp",
            Box::new(FtypDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"mvhd")),
            "mvhd",
            Box::new(MvhdDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"tkhd")),
            "tkhd",
            Box::new(TkhdDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"mdhd")),
            "mdhd",
            Box::new(MdhdDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"hdlr")),
            "hdlr",
            Box::new(HdlrDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"sidx")),
            "sidx",
            Box::new(SidxDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stsd")),
            "stsd",
            Box::new(StsdDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stts")),
            "stts",
            Box::new(SttsDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stss")),
            "stss",
            Box::new(StssDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"ctts")),
            "ctts",
            Box::new(CttsDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stsc")),
            "stsc",
            Box::new(StscDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stsz")),
            "stsz",
            Box::new(StszDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"stco")),
            "stco",
            Box::new(StcoDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"co64")),
            "co64",
            Box::new(Co64Decoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"elst")),
            "elst",
            Box::new(ElstDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"btrt")),
            "btrt",
            Box::new(BtrtDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"esds")),
            "esds",
            Box::new(EsdsDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"av1C")),
            "av1C",
            Box::new(Av1cDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"vpcC")),
            "vpcC",
            Box::new(VpccDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"dOps")),
            "dOps",
            Box::new(DopsDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"dac3")),
            "dac3",
            Box::new(Dac3Decoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"dec3")),
            "dec3",
            Box::new(Dec3Decoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"dfLa")),
            "dfLa",
            Box::new(DflaDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"colr")),
            "colr",
            Box::new(ColrDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"pasp")),
            "pasp",
            Box::new(PaspDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"mdcv")),
            "mdcv",
            Box::new(MdcvDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"clli")),
            "clli",
            Box::new(ClliDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"kind")),
            "kind",
            Box::new(KindDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"irot")),
            "irot",
            Box::new(IrotDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"imir")),
            "imir",
            Box::new(ImirDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"data")),
            "data",
            Box::new(IlstDataDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"mean")),
            "mean",
            Box::new(MeanDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"name")),
            "name",
            Box::new(IlstNameDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"trun")),
            "trun",
            Box::new(TrunDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"tfhd")),
            "tfhd",
            Box::new(TfhdDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"tfdt")),
            "tfdt",
            Box::new(TfdtDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"trex")),
            "trex",
            Box::new(TrexDecoder),
        )
}
