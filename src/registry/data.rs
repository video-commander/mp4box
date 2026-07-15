//! Structured, serializable representations of decoded box payloads,
//! plus [`StructuredData::summary`] for one-line rendering.

/// Byte range of a single named field within a box's payload.
///
/// Offsets are payload-relative: `start = 0` is the first payload byte — i.e.
/// the byte after the box header, and after the version+flags word for full
/// boxes (the parser strips those before a decoder sees the payload). Consumers
/// that highlight fields in a hex view over the whole box add the box's payload
/// offset to map these into file/box coordinates. Spans need not be contiguous
/// or cover the whole payload; reserved/matrix regions are simply omitted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldSpan {
    pub name: String,
    pub start: u64,
    pub length: u64,
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
    /// Object Descriptor Box (iods)
    ObjectDescriptor(IodsData),
    /// Colour Information Box (colr)
    ColourInformation(ColrData),
    /// Dolby Vision Configuration Box (dvcC / dvvC)
    DolbyVisionConfig(DoviConfigData),
    /// Mastering Display Colour Volume Box (mdcv)
    MasteringDisplayColourVolume(MdcvData),
    /// Content Light Level Information Box (clli)
    ContentLightLevel(ClliData),
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

/// Object Descriptor Box data (iods, ISO/IEC 14496-14 §5.1).
///
/// Wraps an MPEG-4 `InitialObjectDescriptor` (ISO/IEC 14496-1 §7.2.6.4).
/// Either an inline URL is present, or the five profile-level indications
/// describe the profiles required to render the scene.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IodsData {
    pub version: u8,
    pub flags: u32,
    /// ObjectDescriptorID (10-bit)
    pub od_id: u16,
    /// includeInlineProfileLevelFlag
    pub include_inline_profiles: bool,
    /// Inline URL, present when URL_Flag is set (mutually exclusive with the
    /// profile-level fields below, which are then 0xFF / "no capability").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// ODProfileLevelIndication (0xFF = none required, 0xFE = no capability)
    pub od_profile_level: u8,
    /// sceneProfileLevelIndication
    pub scene_profile_level: u8,
    /// audioProfileLevelIndication
    pub audio_profile_level: u8,
    /// visualProfileLevelIndication
    pub visual_profile_level: u8,
    /// graphicsProfileLevelIndication
    pub graphics_profile_level: u8,
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
    /// Decoded Widevine payload, when the system is Widevine and the data
    /// blob parses as its protobuf. Boxed to keep the enum variants close
    /// in size.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub widevine: Option<Box<crate::drm::WidevinePsshData>>,
    /// Decoded PlayReady payload, when the system is PlayReady and the data
    /// blob parses as a PlayReady Object.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub playready: Option<Box<crate::drm::PlayReadyPsshData>>,
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
                if let Some(wv) = &d.widevine {
                    if let Some(provider) = &wv.provider {
                        s.push_str(&format!(" provider={provider}"));
                    }
                    if let Some(scheme) = &wv.protection_scheme {
                        s.push_str(&format!(" scheme={scheme}"));
                    }
                    if !wv.key_ids.is_empty() && d.key_ids.is_empty() {
                        s.push_str(&format!(" wv_kids={}", wv.key_ids.len()));
                    }
                }
                if let Some(pr) = &d.playready {
                    if let Some(version) = &pr.wrm_header_version {
                        s.push_str(&format!(" wrm_version={version}"));
                    }
                    if let Some(la_url) = &pr.la_url {
                        s.push_str(&format!(" la_url=\"{la_url}\""));
                    }
                }
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
            StructuredData::ObjectDescriptor(d) => {
                let mut s = format!("od_id={}", d.od_id);
                if let Some(url) = &d.url {
                    s.push_str(&format!(" url={:?}", url));
                } else {
                    s.push_str(&format!(
                        " od_profile=0x{:02X} scene=0x{:02X} audio=0x{:02X} visual=0x{:02X} graphics=0x{:02X}",
                        d.od_profile_level,
                        d.scene_profile_level,
                        d.audio_profile_level,
                        d.visual_profile_level,
                        d.graphics_profile_level
                    ));
                }
                s
            }
            StructuredData::ColourInformation(d) => {
                if d.colour_type == "nclx" {
                    let lab = |v: Option<u16>, n: &Option<String>| match (v, n) {
                        (Some(v), Some(n)) => format!("{v} ({n})"),
                        (Some(v), None) => v.to_string(),
                        _ => "?".to_string(),
                    };
                    format!(
                        "type=nclx primaries={} transfer={} matrix={} full_range={}",
                        lab(d.primaries, &d.primaries_name),
                        lab(d.transfer, &d.transfer_name),
                        lab(d.matrix, &d.matrix_name),
                        d.full_range.map(u8::from).unwrap_or(0)
                    )
                } else {
                    format!("type={}", d.colour_type)
                }
            }
            StructuredData::DolbyVisionConfig(d) => format!(
                "version={}.{} profile={} level={} rpu_present={} el_present={} bl_present={} bl_compatibility={} ({})",
                d.dv_version_major,
                d.dv_version_minor,
                d.dv_profile,
                d.dv_level,
                u8::from(d.rpu_present),
                u8::from(d.el_present),
                u8::from(d.bl_present),
                d.bl_signal_compatibility_id,
                d.bl_signal_compatibility,
            ),
            StructuredData::MasteringDisplayColourVolume(d) => format!(
                "R({},{}) G({},{}) B({},{}) W({},{}) max_luminance={:.4} min_luminance={:.4} cd/m2",
                d.red_x,
                d.red_y,
                d.green_x,
                d.green_y,
                d.blue_x,
                d.blue_y,
                d.white_x,
                d.white_y,
                d.max_display_mastering_luminance,
                d.min_display_mastering_luminance,
            ),
            StructuredData::ContentLightLevel(d) => format!(
                "max_cll={} max_fall={}",
                d.max_content_light_level, d.max_pic_average_light_level
            ),
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

/// Colour Information Box data (colr).
///
/// For `nclx` the CICP code points are captured with their human-readable
/// names (see [`crate::registry::cicp`]); `transfer` in particular signals HDR
/// (16 = PQ, 18 = HLG). Other colour types (`nclc`, `prof`, `rICC`) carry only
/// the `colour_type` tag here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColrData {
    /// "nclx", "nclc", "prof", or "rICC".
    pub colour_type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primaries: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primaries_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub transfer: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub transfer_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matrix: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matrix_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_range: Option<bool>,
}

/// Dolby Vision Configuration Box data (dvcC / dvvC).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DoviConfigData {
    pub dv_version_major: u8,
    pub dv_version_minor: u8,
    pub dv_profile: u8,
    pub dv_level: u8,
    pub rpu_present: bool,
    pub el_present: bool,
    pub bl_present: bool,
    /// Base-layer cross-compatibility id (`dv_bl_signal_compatibility_id`).
    pub bl_signal_compatibility_id: u8,
    /// Human-readable meaning of `bl_signal_compatibility_id`.
    pub bl_signal_compatibility: String,
}

/// Mastering Display Colour Volume Box data (mdcv, SMPTE ST 2086). Chromaticity
/// coordinates are the raw 0.00002-step integers; luminances are in cd/m².
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MdcvData {
    pub red_x: u16,
    pub red_y: u16,
    pub green_x: u16,
    pub green_y: u16,
    pub blue_x: u16,
    pub blue_y: u16,
    pub white_x: u16,
    pub white_y: u16,
    pub max_display_mastering_luminance: f64,
    pub min_display_mastering_luminance: f64,
}

/// Content Light Level Information Box data (clli).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClliData {
    /// MaxCLL: maximum content light level (cd/m²).
    pub max_content_light_level: u16,
    /// MaxFALL: maximum frame-average light level (cd/m²).
    pub max_pic_average_light_level: u16,
}
