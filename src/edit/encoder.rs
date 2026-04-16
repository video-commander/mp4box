use crate::{
    boxes::{BoxHeader, BoxKey, FourCC},
    registry::BoxValue,
};
use byteorder::{BigEndian, WriteBytesExt};
use std::collections::HashMap;

/// Trait for encoding a [`BoxValue`] back to raw box-body bytes (no box header).
///
/// The encoded bytes cover only the body of the box — the calling layer wraps
/// them with the appropriate 4-byte size + 4-byte type header (plus version/flags
/// prefix for FullBox types).
pub trait BoxEncoder: Send + Sync {
    fn encode(&self, value: &BoxValue) -> anyhow::Result<Vec<u8>>;
}

// ---- Registry integration -----------------------------------------------

struct BoxEncoderEntry {
    inner: Box<dyn BoxEncoder>,
    _name: String,
}

/// A registry of box encoders keyed by [`BoxKey`].
pub struct EncoderRegistry {
    map: HashMap<BoxKey, BoxEncoderEntry>,
}

impl EncoderRegistry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn with_encoder(mut self, key: BoxKey, name: &str, enc: Box<dyn BoxEncoder>) -> Self {
        self.map.insert(
            key,
            BoxEncoderEntry {
                inner: enc,
                _name: name.to_string(),
            },
        );
        self
    }

    pub fn encode(&self, key: &BoxKey, value: &BoxValue) -> Option<anyhow::Result<Vec<u8>>> {
        self.map.get(key).map(|e| e.inner.encode(value))
    }
}

impl Default for EncoderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Helper: write a complete box (header + body) -----------------------

/// Wrap `body` bytes with an 8-byte box header (`size u32 BE` + `fourcc`).
/// Uses a 64-bit extended header automatically if the total size ≥ 2^32.
pub fn wrap_box_header(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let total = 8u64 + body.len() as u64;
    let mut out = Vec::with_capacity(total as usize);
    if total < u32::MAX as u64 {
        out.write_u32::<BigEndian>(total as u32).unwrap();
        out.extend_from_slice(fourcc);
    } else {
        // Extended size box
        out.write_u32::<BigEndian>(1u32).unwrap();
        out.extend_from_slice(fourcc);
        out.write_u64::<BigEndian>(total + 8).unwrap(); // +8 for the 8-byte ext size
    }
    out.extend_from_slice(body);
    out
}

/// Wrap body with FullBox prefix (version byte + 3 flags bytes) then box header.
pub fn wrap_full_box_header(fourcc: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let full_body_len = 4 + body.len(); // version(1) + flags(3) + body
    let total = 8u64 + full_body_len as u64;
    let mut out = Vec::with_capacity(total as usize);
    out.write_u32::<BigEndian>(total as u32).unwrap();
    out.extend_from_slice(fourcc);
    out.push(version);
    out.push(((flags >> 16) & 0xFF) as u8);
    out.push(((flags >> 8) & 0xFF) as u8);
    out.push((flags & 0xFF) as u8);
    out.extend_from_slice(body);
    out
}

// ---- mvhd encoder -------------------------------------------------------

/// Encodes `mvhd` body from a `BoxValue::Text` produced by `MvhdDecoder`.
///
/// The text format is `"timescale=N duration=N"`.  We also support encoding
/// a structured text with extra fields written back verbatim; for `--set`
/// the caller mutates specific fields before passing in the updated text.
///
/// For simplicity (and because mvhd has two timestamp fields that the decoder
/// currently discards), this encoder writes a version-0 mvhd body with
/// creation_time=0 and modification_time=0 unless supplied in the value.
pub struct MvhdEncoder;

impl BoxEncoder for MvhdEncoder {
    fn encode(&self, value: &BoxValue) -> anyhow::Result<Vec<u8>> {
        let text = match value {
            BoxValue::Text(t) => t.as_str(),
            _ => anyhow::bail!("MvhdEncoder: expected BoxValue::Text"),
        };

        let mut creation_time = 0u32;
        let mut modification_time = 0u32;
        let mut timescale = 1000u32;
        let mut duration = 0u32;
        let mut rate: u32 = 0x00010000; // 1.0 fixed-point
        let mut volume: u16 = 0x0100; // 1.0 fixed-point
        let mut next_track_id: u32 = 1;

        for part in text.split_whitespace() {
            if let Some(v) = part.strip_prefix("timescale=") {
                timescale = v.parse().unwrap_or(timescale);
            } else if let Some(v) = part.strip_prefix("duration=") {
                duration = v.parse().unwrap_or(duration);
            } else if let Some(v) = part.strip_prefix("creation_time=") {
                creation_time = v.parse().unwrap_or(creation_time);
            } else if let Some(v) = part.strip_prefix("modification_time=") {
                modification_time = v.parse().unwrap_or(modification_time);
            } else if let Some(v) = part.strip_prefix("rate=") {
                rate = v.parse().unwrap_or(rate);
            } else if let Some(v) = part.strip_prefix("volume=") {
                volume = v.parse().unwrap_or(volume);
            } else if let Some(v) = part.strip_prefix("next_track_id=") {
                next_track_id = v.parse().unwrap_or(next_track_id);
            }
        }

        let mut body = Vec::with_capacity(96);
        // version=0, flags=0 (written by wrap_full_box_header caller; here we
        // emit the post-version/flags body only)
        body.write_u32::<BigEndian>(creation_time)?;
        body.write_u32::<BigEndian>(modification_time)?;
        body.write_u32::<BigEndian>(timescale)?;
        body.write_u32::<BigEndian>(duration)?;
        body.write_u32::<BigEndian>(rate)?;
        body.write_u16::<BigEndian>(volume)?;
        // reserved: 2 + 8 + 36-byte matrix + 24-byte pre_defined
        body.extend_from_slice(&[0u8; 2 + 8]); // padding + reserved
        // Identity matrix
        body.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x00, // a  = 1.0
            0x00, 0x00, 0x00, 0x00, // b  = 0
            0x00, 0x00, 0x00, 0x00, // u  = 0
            0x00, 0x00, 0x00, 0x00, // c  = 0
            0x00, 0x01, 0x00, 0x00, // d  = 1.0
            0x00, 0x00, 0x00, 0x00, // v  = 0
            0x00, 0x00, 0x00, 0x00, // tx = 0
            0x00, 0x00, 0x00, 0x00, // ty = 0
            0x40, 0x00, 0x00, 0x00, // w  = 1.0 (16.16 fixed → 0x40000000)
        ]);
        body.extend_from_slice(&[0u8; 24]); // pre_defined
        body.write_u32::<BigEndian>(next_track_id)?;
        Ok(body)
    }
}

// ---- default encoder registry -------------------------------------------

pub fn default_encoder_registry() -> EncoderRegistry {
    EncoderRegistry::new().with_encoder(
        BoxKey::FourCC(FourCC(*b"mvhd")),
        "mvhd",
        Box::new(MvhdEncoder),
    )
}

// ---- BoxHeader helpers --------------------------------------------------

impl BoxHeader {
    /// Returns the file range [start, start+size) covered by this box.
    pub fn byte_range(&self) -> (u64, u64) {
        (self.start, self.start + self.size)
    }
}
