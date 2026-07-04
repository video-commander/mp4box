use crate::boxes::{BoxHeader, BoxKey, FourCC};
use crate::util::ReadExt;
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
    /// Movie Header Box (mvhd)
    MovieHeader(MvhdData),
    /// Edit List Box (elst)
    EditList(ElstData),
    /// Segment Index Box (sidx)
    SegmentIndex(SidxData),
    /// Track Fragment Run Box (trun)
    TrackFragmentRun(TrunData),
    /// Track Fragment Header Box (tfhd)
    TrackFragmentHeader(TfhdData),
    /// Track Fragment Decode Time Box (tfdt)
    TrackFragmentDecodeTime(TfdtData),
    /// Track Extends Box (trex)
    TrackExtends(TrexData),
    /// Protection System Specific Header Box (pssh)
    ProtectionSystemHeader(PsshData),
    /// Track Encryption Box (tenc)
    TrackEncryption(TencData),
    /// Event Message Box (emsg)
    EventMessage(EmsgData),
    /// Elementary Stream Descriptor (esds)
    ElementaryStream(EsdsData),
}

/// Elementary Stream Descriptor data (esds, ISO 14496-1/-3)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EsdsData {
    pub version: u8,
    pub flags: u32,
    /// MPEG-4 objectTypeIndication (0x40 = MPEG-4 Audio/AAC, 0x6B = MP3, ...)
    pub object_type: u8,
    /// Human-readable name for `object_type`
    pub object_type_name: String,
    pub max_bitrate: u32,
    pub avg_bitrate: u32,
    /// Parsed AudioSpecificConfig from DecoderSpecificInfo, when present
    /// and the stream is MPEG-4 audio.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_config: Option<AudioSpecificConfig>,
}

/// AudioSpecificConfig (ISO 14496-3 §1.6.2.1), the authoritative source of
/// AAC profile, sample rate, and channel layout. The sample-entry fields
/// are unreliable for HE-AAC, where SBR doubles the output rate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioSpecificConfig {
    /// audioObjectType (2 = AAC-LC, 5 = SBR, 29 = PS, ...)
    pub audio_object_type: u8,
    /// Profile name derived from the object type and SBR/PS signaling
    /// ("AAC-LC", "HE-AAC", "HE-AAC v2", ...)
    pub profile: String,
    /// Core sampling frequency in Hz
    pub sample_rate: u32,
    /// Output sampling frequency after SBR extension, when explicitly
    /// signaled (typically double the core rate)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub extension_sample_rate: Option<u32>,
    /// channelConfiguration (1 = mono, 2 = stereo, ..., 7 = 7.1)
    pub channel_configuration: u8,
    /// Explicit SBR signaling present (HE-AAC)
    pub sbr: bool,
    /// Explicit PS signaling present (HE-AAC v2)
    pub ps: bool,
}

/// Protection System Specific Header Box data (ISO 23001-7)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PsshData {
    pub version: u8,
    pub flags: u32,
    /// DRM system UUID, hyphenated lowercase hex
    pub system_id: String,
    /// Human-readable DRM system name, if recognized
    pub system_name: Option<String>,
    /// Key IDs (version 1 only), 32-char lowercase hex each
    pub key_ids: Vec<String>,
    /// Size of the system-specific data blob
    pub data_size: u32,
}

/// Track Encryption Box data (ISO 23001-7)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TencData {
    pub version: u8,
    pub flags: u32,
    /// Pattern encryption (cbcs): encrypted blocks per pattern (version 1+)
    pub default_crypt_byte_block: u8,
    /// Pattern encryption (cbcs): clear blocks per pattern (version 1+)
    pub default_skip_byte_block: u8,
    pub default_is_protected: bool,
    /// Per-sample IV size in bytes (0 = constant IV, see `default_constant_iv`)
    pub default_per_sample_iv_size: u8,
    /// Default key ID, 32-char lowercase hex
    pub default_kid: String,
    /// Constant IV (hex) when per-sample IV size is 0
    pub default_constant_iv: Option<String>,
}

/// Event Message Box data (DASH, ISO 23009-1)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmsgData {
    pub version: u8,
    pub flags: u32,
    pub scheme_id_uri: String,
    pub value: String,
    pub timescale: u32,
    /// Version 1: absolute presentation time
    pub presentation_time: Option<u64>,
    /// Version 0: delta from the segment start
    pub presentation_time_delta: Option<u32>,
    pub event_duration: u32,
    pub id: u32,
    /// Size of the message payload
    pub message_size: u64,
}

impl StructuredData {
    /// One-line human-readable summary, used as the `decoded` string in the
    /// high-level API and by the CLI tools.
    pub fn summary(&self) -> String {
        match self {
            StructuredData::MovieHeader(d) => {
                format!("timescale={} duration={}", d.timescale, d.duration)
            }
            StructuredData::TrackHeader(d) => format!(
                "track_id={} duration={} width={} height={} flags=0x{:06X}",
                d.track_id, d.duration, d.width, d.height, d.flags
            ),
            StructuredData::MediaHeader(d) => format!(
                "timescale={} duration={} language={}",
                d.timescale, d.duration, d.language
            ),
            StructuredData::HandlerReference(d) => {
                format!("handler_type={} name={:?}", d.handler_type, d.name)
            }
            StructuredData::EditList(d) => match d.entries.first() {
                Some(e) => format!(
                    "version={} entries={} first: duration={} media_time={} rate={}/{}",
                    d.version,
                    d.entry_count,
                    e.segment_duration,
                    e.media_time,
                    e.media_rate_integer,
                    e.media_rate_fraction
                ),
                None => format!("version={} entries=0", d.version),
            },
            StructuredData::SegmentIndex(d) => format!(
                "timescale={} earliest_presentation_time={} first_offset={} references={}",
                d.timescale,
                d.earliest_presentation_time,
                d.first_offset,
                d.references.len()
            ),
            StructuredData::SampleDescription(d) => {
                let e = d.entries.first();
                match e {
                    Some(e) => {
                        let mut s = format!("codec={}", e.codec);
                        if let (Some(w), Some(h)) = (e.width, e.height) {
                            s.push_str(&format!(" {}x{}", w, h));
                        }
                        if let Some(ch) = e.channel_count {
                            s.push_str(&format!(" channels={}", ch));
                        }
                        if let Some(sr) = e.sample_rate {
                            s.push_str(&format!(" sample_rate={}", sr));
                        }
                        if let Some(bits) = e.sample_size {
                            s.push_str(&format!(" bits={}", bits));
                        }
                        s.push_str(&format!(" entries={}", d.entry_count));
                        s
                    }
                    None => "entries=0".to_string(),
                }
            }
            StructuredData::DecodingTimeToSample(d) => {
                let summary: Vec<String> = d
                    .entries
                    .iter()
                    .take(4)
                    .map(|e| format!("{}×{}", e.sample_count, e.sample_delta))
                    .collect();
                let ellipsis = if d.entry_count > 4 { ", …" } else { "" };
                format!(
                    "entries={} [{}{}]",
                    d.entry_count,
                    summary.join(", "),
                    ellipsis
                )
            }
            StructuredData::CompositionTimeToSample(d) => format!("entries={}", d.entry_count),
            StructuredData::SampleToChunk(d) => format!("entries={}", d.entry_count),
            StructuredData::SampleSize(d) => {
                if d.sample_size > 0 {
                    format!("fixed_size={} count={}", d.sample_size, d.sample_count)
                } else {
                    format!("variable count={}", d.sample_count)
                }
            }
            StructuredData::SyncSample(d) => format!("keyframes={}", d.entry_count),
            StructuredData::ChunkOffset(d) => format!("chunks={}", d.entry_count),
            StructuredData::ChunkOffset64(d) => format!("chunks={}", d.entry_count),
            StructuredData::TrackFragmentRun(d) => {
                let mut parts = vec![format!("samples={}", d.sample_count)];
                if let Some(off) = d.data_offset {
                    parts.push(format!("data_offset={}", off));
                }
                let has_dur = d.flags & 0x100 != 0;
                let has_size = d.flags & 0x200 != 0;
                if (has_dur || has_size)
                    && let Some(first) = d.samples.first()
                {
                    if let Some(dur) = first.duration {
                        parts.push(format!("first_dur={}", dur));
                    }
                    if let Some(sz) = first.size {
                        parts.push(format!("first_size={}", sz));
                    }
                }
                parts.join(" ")
            }
            StructuredData::TrackFragmentHeader(d) => {
                let mut parts = vec![format!("track_id={}", d.track_id)];
                if let Some(v) = d.base_data_offset {
                    parts.push(format!("base_data_offset={}", v));
                }
                if let Some(v) = d.sample_description_index {
                    parts.push(format!("sample_description_index={}", v));
                }
                if let Some(v) = d.default_sample_duration {
                    parts.push(format!("default_duration={}", v));
                }
                if let Some(v) = d.default_sample_size {
                    parts.push(format!("default_size={}", v));
                }
                if let Some(v) = d.default_sample_flags {
                    parts.push(format!("default_flags=0x{:08X}", v));
                }
                if d.default_base_is_moof {
                    parts.push("default_base_is_moof".into());
                }
                parts.join(" ")
            }
            StructuredData::TrackFragmentDecodeTime(d) => {
                format!("base_media_decode_time={}", d.base_media_decode_time)
            }
            StructuredData::TrackExtends(d) => format!(
                "track_id={} default_sample_description_index={} default_sample_duration={} default_sample_size={} default_sample_flags=0x{:08X}",
                d.track_id,
                d.default_sample_description_index,
                d.default_sample_duration,
                d.default_sample_size,
                d.default_sample_flags
            ),
            StructuredData::ProtectionSystemHeader(d) => {
                let mut s = match &d.system_name {
                    Some(name) => format!("system={} ({})", name, d.system_id),
                    None => format!("system={}", d.system_id),
                };
                if !d.key_ids.is_empty() {
                    s.push_str(&format!(" kids={}", d.key_ids.len()));
                    for kid in d.key_ids.iter().take(2) {
                        s.push_str(&format!(" {}", kid));
                    }
                    if d.key_ids.len() > 2 {
                        s.push('…');
                    }
                }
                s.push_str(&format!(" data_size={}", d.data_size));
                s
            }
            StructuredData::TrackEncryption(d) => {
                let mut s = format!(
                    "protected={} iv_size={} kid={}",
                    d.default_is_protected, d.default_per_sample_iv_size, d.default_kid
                );
                if let Some(iv) = &d.default_constant_iv {
                    s.push_str(&format!(" constant_iv={}", iv));
                }
                if d.default_crypt_byte_block != 0 || d.default_skip_byte_block != 0 {
                    s.push_str(&format!(
                        " pattern={}:{}",
                        d.default_crypt_byte_block, d.default_skip_byte_block
                    ));
                }
                s
            }
            StructuredData::ElementaryStream(d) => {
                let mut s = format!(
                    "objectType=0x{:02X} ({})",
                    d.object_type, d.object_type_name
                );
                if let Some(a) = &d.audio_config {
                    s.push_str(&format!(
                        " profile={} sample_rate={}",
                        a.profile, a.sample_rate
                    ));
                    if let Some(ext) = a.extension_sample_rate {
                        s.push_str(&format!(" output_rate={}", ext));
                    }
                    s.push_str(&format!(" channels={}", a.channel_configuration));
                }
                s.push_str(&format!(
                    " max_bitrate={} avg_bitrate={}",
                    d.max_bitrate, d.avg_bitrate
                ));
                s
            }
            StructuredData::EventMessage(d) => {
                let time = match (d.presentation_time, d.presentation_time_delta) {
                    (Some(t), _) => format!("presentation_time={}", t),
                    (None, Some(dt)) => format!("presentation_time_delta={}", dt),
                    _ => String::new(),
                };
                format!(
                    "scheme={:?} value={:?} timescale={} {} duration={} id={} message_size={}",
                    d.scheme_id_uri,
                    d.value,
                    d.timescale,
                    time,
                    d.event_duration,
                    d.id,
                    d.message_size
                )
            }
        }
    }
}

/// Movie Header Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MvhdData {
    pub version: u8,
    pub flags: u32,
    pub creation_time: u64,
    pub modification_time: u64,
    pub timescale: u32,
    pub duration: u64,
    /// Playback rate (1.0 = normal), from the 16.16 fixed-point field
    pub rate: f32,
    /// Playback volume (1.0 = full), from the 8.8 fixed-point field
    pub volume: f32,
    pub next_track_id: u32,
}

/// Edit List Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElstData {
    pub version: u8,
    pub flags: u32,
    pub entry_count: u32,
    pub entries: Vec<ElstEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElstEntry {
    /// Edit duration in movie timescale units
    pub segment_duration: u64,
    /// Start time within the media (-1 = empty edit)
    pub media_time: i64,
    pub media_rate_integer: i16,
    pub media_rate_fraction: i16,
}

/// Segment Index Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SidxData {
    pub version: u8,
    pub flags: u32,
    pub reference_id: u32,
    pub timescale: u32,
    pub earliest_presentation_time: u64,
    pub first_offset: u64,
    pub references: Vec<SidxReference>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SidxReference {
    /// 1 = reference to another sidx, 0 = reference to media
    pub reference_type: u8,
    pub referenced_size: u32,
    pub subsegment_duration: u32,
    pub starts_with_sap: bool,
    pub sap_type: u8,
    pub sap_delta_time: u32,
}

/// Track Fragment Header Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TfhdData {
    pub version: u8,
    pub flags: u32,
    pub track_id: u32,
    pub base_data_offset: Option<u64>,
    pub sample_description_index: Option<u32>,
    pub default_sample_duration: Option<u32>,
    pub default_sample_size: Option<u32>,
    pub default_sample_flags: Option<u32>,
    /// Flag 0x010000: the fragment's samples have no duration
    pub duration_is_empty: bool,
    /// Flag 0x020000: offsets are relative to the enclosing moof
    pub default_base_is_moof: bool,
}

/// Track Fragment Decode Time Box data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TfdtData {
    pub version: u8,
    pub flags: u32,
    pub base_media_decode_time: u64,
}

/// Track Extends Box data (per-track defaults for fragments)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrexData {
    pub track_id: u32,
    pub default_sample_description_index: u32,
    pub default_sample_duration: u32,
    pub default_sample_size: u32,
    pub default_sample_flags: u32,
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
    /// Video: frame width in pixels
    pub width: Option<u16>,
    /// Video: frame height in pixels
    pub height: Option<u16>,
    /// Audio: channel count
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_count: Option<u16>,
    /// Audio: bits per sample
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sample_size: Option<u16>,
    /// Audio: sample rate in Hz (integer part of the 16.16 fixed-point field)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sample_rate: Option<u32>,
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
    pub creation_time: u64,
    pub modification_time: u64,
    pub timescale: u32,
    pub duration: u64,
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

// ---------- AudioSpecificConfig (ISO 14496-3) ----------

/// MSB-first bit reader over a byte slice.
struct BitReader<'a> {
    buf: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, bit: 0 }
    }

    fn read(&mut self, n: usize) -> Option<u32> {
        debug_assert!(n <= 32);
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.buf.get(self.bit / 8)?;
            v = (v << 1) | ((byte >> (7 - self.bit % 8)) & 1) as u32;
            self.bit += 1;
        }
        Some(v)
    }
}

fn aac_sample_rate(index: u32) -> Option<u32> {
    [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ]
    .get(index as usize)
    .copied()
}

fn audio_object_type_name(aot: u8) -> &'static str {
    match aot {
        1 => "AAC Main",
        2 => "AAC-LC",
        3 => "AAC SSR",
        4 => "AAC LTP",
        5 => "SBR",
        6 => "AAC Scalable",
        23 => "AAC-LD",
        29 => "PS",
        39 => "AAC-ELD",
        42 => "xHE-AAC (USAC)",
        _ => "MPEG-4 Audio",
    }
}

/// Parse the leading fields of an AudioSpecificConfig: object type,
/// sampling frequency, channel configuration, and explicit (hierarchical)
/// SBR/PS signaling. Implicit HE-AAC signaling lives in the bitstream and
/// cannot be detected here.
fn parse_audio_specific_config(buf: &[u8]) -> Option<AudioSpecificConfig> {
    let mut r = BitReader::new(buf);

    let read_aot = |r: &mut BitReader| -> Option<u8> {
        let aot = r.read(5)?;
        if aot == 31 {
            Some((32 + r.read(6)?) as u8)
        } else {
            Some(aot as u8)
        }
    };
    let read_rate = |r: &mut BitReader| -> Option<u32> {
        let idx = r.read(4)?;
        if idx == 15 {
            r.read(24)
        } else {
            aac_sample_rate(idx)
        }
    };

    let mut aot = read_aot(&mut r)?;
    let sample_rate = read_rate(&mut r)?;
    let channel_configuration = r.read(4)? as u8;

    // Explicit hierarchical signaling: AOT 5 (SBR) / 29 (PS) wrap the real
    // codec — the extension rate and true object type follow.
    let mut sbr = false;
    let mut ps = false;
    let mut extension_sample_rate = None;
    if aot == 5 || aot == 29 {
        sbr = true;
        ps = aot == 29;
        extension_sample_rate = Some(read_rate(&mut r)?);
        aot = read_aot(&mut r)?;
    } else if matches!(aot, 1..=4 | 6 | 7 | 17 | 19..=23) {
        // Explicit backward-compatible signaling: a GASpecificConfig
        // followed by a 0x2B7 sync extension carrying SBR (and possibly PS)
        // at the end of the config. Skip the GASpecificConfig fields first.
        let ga_ok = (|| -> Option<bool> {
            let _frame_length_flag = r.read(1)?;
            if r.read(1)? == 1 {
                r.read(14)?; // coreCoderDelay
            }
            // extensionFlag adds AOT-specific fields we don't model; give
            // up on sync-extension detection rather than misread bits.
            Some(r.read(1)? == 0)
        })();

        if ga_ok == Some(true)
            && r.read(11) == Some(0x2B7)
            && read_aot(&mut r) == Some(5)
            && r.read(1) == Some(1)
        {
            sbr = true;
            extension_sample_rate = read_rate(&mut r);
            // HE-AAC v2: a second sync extension signals parametric stereo.
            if r.read(11) == Some(0x548) && r.read(1) == Some(1) {
                ps = true;
            }
        }
    }

    let profile = if ps {
        "HE-AAC v2".to_string()
    } else if sbr {
        "HE-AAC".to_string()
    } else {
        audio_object_type_name(aot).to_string()
    };

    Some(AudioSpecificConfig {
        audio_object_type: aot,
        profile,
        sample_rate,
        extension_sample_rate,
        channel_configuration,
        sbr,
        ps,
    })
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
        let max_cll = r.read_u16_be()?;
        let max_fall = r.read_u16_be()?;
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
}

// ---------- DRM / DASH decoders ----------

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn uuid_string(bytes: &[u8; 16]) -> String {
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
fn drm_system_name(system_id: &[u8; 16]) -> Option<&'static str> {
    match system_id {
        [
            0xED,
            0xEF,
            0x8B,
            0xA9,
            0x79,
            0xD6,
            0x4A,
            0xCE,
            0xA3,
            0xC8,
            0x27,
            0xDC,
            0xD5,
            0x1D,
            0x21,
            0xED,
        ] => Some("Widevine"),
        [
            0x9A,
            0x04,
            0xF0,
            0x79,
            0x98,
            0x40,
            0x42,
            0x86,
            0xAB,
            0x92,
            0xE6,
            0x5B,
            0xE0,
            0x88,
            0x5F,
            0x95,
        ] => Some("PlayReady"),
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
        let version = version.unwrap_or(0);
        let mut system_id = [0u8; 16];
        r.read_exact(&mut system_id)?;

        let mut key_ids = Vec::new();
        if version >= 1 {
            let kid_count = r.read_u32_be()?;
            for _ in 0..kid_count {
                let mut kid = [0u8; 16];
                r.read_exact(&mut kid)?;
                key_ids.push(hex_string(&kid));
            }
        }
        let data_size = r.read_u32_be()?;

        Ok(BoxValue::Structured(
            StructuredData::ProtectionSystemHeader(PsshData {
                version,
                flags: flags.unwrap_or(0),
                system_id: uuid_string(&system_id),
                system_name: drm_system_name(&system_id).map(str::to_string),
                key_ids,
                data_size,
            }),
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
}

/// Remaining message bytes after `consumed` bytes of fixed fields, given
/// the box header (payload excludes header and version/flags).
fn payload_len_after(hdr: &BoxHeader, consumed: u64) -> u64 {
    hdr.size
        .saturating_sub(hdr.header_size)
        .saturating_sub(4) // version/flags
        .saturating_sub(consumed)
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
            BoxKey::FourCC(FourCC(*b"stz2")),
            "stz2",
            Box::new(Stz2Decoder),
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
            BoxKey::FourCC(FourCC(*b"avcC")),
            "avcC",
            Box::new(AvccDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"hvcC")),
            "hvcC",
            Box::new(HvccDecoder),
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
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"pssh")),
            "pssh",
            Box::new(PsshDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"tenc")),
            "tenc",
            Box::new(TencDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"senc")),
            "senc",
            Box::new(SencDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"emsg")),
            "emsg",
            Box::new(EmsgDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"schm")),
            "schm",
            Box::new(SchmDecoder),
        )
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"frma")),
            "frma",
            Box::new(FrmaDecoder),
        )
}
