//! System-specific `pssh` payload decoding.
//!
//! The `data` blob inside a Protection System Specific Header box (ISO
//! 23001-7) is opaque at the ISO-BMFF level; its format is defined by each
//! DRM system. This module decodes the two formats seen in practice:
//!
//! - **Widevine**: a small protobuf (`WidevinePsshData` in Google's schema).
//! - **PlayReady**: a PlayReady Object — little-endian length-prefixed
//!   records carrying a UTF-16LE `WRMHEADER` XML document.
//!
//! It also parses raw `pssh` box bytes outside a file context
//! ([`parse_pssh_boxes`]), the form carried by DASH `cenc:pssh` manifest
//! elements and pasted around in logs and tickets.

use crate::registry::{PsshData, drm_system_name, hex_string, uuid_string};

pub const WIDEVINE_SYSTEM_ID: [u8; 16] = [
    0xED, 0xEF, 0x8B, 0xA9, 0x79, 0xD6, 0x4A, 0xCE, 0xA3, 0xC8, 0x27, 0xDC, 0xD5, 0x1D, 0x21, 0xED,
];

pub const PLAYREADY_SYSTEM_ID: [u8; 16] = [
    0x9A, 0x04, 0xF0, 0x79, 0x98, 0x40, 0x42, 0x86, 0xAB, 0x92, 0xE6, 0x5B, 0xE0, 0x88, 0x5F, 0x95,
];

/// Decoded Widevine pssh payload (the `WidevinePsshData` protobuf).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WidevinePsshData {
    /// Legacy v0 algorithm field: "UNENCRYPTED" or "AESCTR".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub algorithm: Option<String>,
    /// Key IDs, 32-char lowercase hex each.
    pub key_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<String>,
    /// Content ID bytes as lowercase hex.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content_id: Option<String>,
    /// Content ID as text, when the bytes are printable ASCII.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content_id_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub crypto_period_index: Option<u32>,
    /// Protection scheme 4CC: "cenc", "cbcs", "cens", or "cbc1".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub protection_scheme: Option<String>,
}

/// Decoded PlayReady pssh payload (PlayReady Object + WRMHEADER).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayReadyPsshData {
    pub record_count: u16,
    /// WRMHEADER version attribute, e.g. "4.2.0.0".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub wrm_header_version: Option<String>,
    /// Key IDs converted from PlayReady's little-endian GUID order to the
    /// CENC byte order used everywhere else, 32-char lowercase hex each.
    pub key_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub la_url: Option<String>,
    /// The rights-management header (WRMHEADER) XML, re-encoded as UTF-8.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub xml: Option<String>,
}

// ---------- Widevine (protobuf) ----------

struct ProtoReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ProtoReader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = *self.data.get(self.pos)?;
            self.pos += 1;
            if shift >= 64 {
                return None;
            }
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
        }
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = self.varint()? as usize;
        let end = self.pos.checked_add(len)?;
        if end > self.data.len() {
            return None;
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Some(out)
    }

    fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => {
                self.pos = self.pos.checked_add(8)?;
            }
            2 => {
                self.bytes()?;
            }
            5 => {
                self.pos = self.pos.checked_add(4)?;
            }
            _ => return None,
        }
        (self.pos <= self.data.len()).then_some(())
    }
}

/// Parses a bare Widevine pssh payload. Returns `None` for anything that
/// doesn't decode cleanly as the Widevine protobuf, so it can double as a
/// "is this Widevine data?" probe.
pub fn parse_widevine_pssh_data(data: &[u8]) -> Option<WidevinePsshData> {
    if data.is_empty() {
        return None;
    }
    let mut out = WidevinePsshData {
        algorithm: None,
        key_ids: Vec::new(),
        provider: None,
        content_id: None,
        content_id_text: None,
        policy: None,
        crypto_period_index: None,
        protection_scheme: None,
    };
    let mut recognized = 0usize;
    let mut r = ProtoReader { data, pos: 0 };
    while r.pos < data.len() {
        let key = r.varint()?;
        let field = key >> 3;
        let wire = (key & 7) as u8;
        match (field, wire) {
            (1, 0) => {
                out.algorithm = Some(match r.varint()? {
                    0 => "UNENCRYPTED".to_string(),
                    1 => "AESCTR".to_string(),
                    n => n.to_string(),
                });
                recognized += 1;
            }
            (2, 2) => {
                out.key_ids.push(hex_string(r.bytes()?));
                recognized += 1;
            }
            (3, 2) => {
                out.provider = Some(String::from_utf8(r.bytes()?.to_vec()).ok()?);
                recognized += 1;
            }
            (4, 2) => {
                let id = r.bytes()?;
                out.content_id = Some(hex_string(id));
                if !id.is_empty() && id.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                    out.content_id_text = Some(String::from_utf8(id.to_vec()).ok()?);
                }
                recognized += 1;
            }
            (6, 2) => {
                out.policy = Some(String::from_utf8(r.bytes()?.to_vec()).ok()?);
                recognized += 1;
            }
            (7, 0) => {
                out.crypto_period_index = Some(u32::try_from(r.varint()?).ok()?);
                recognized += 1;
            }
            (9, 0) => {
                let fourcc = u32::try_from(r.varint()?).ok()?.to_be_bytes();
                out.protection_scheme = if fourcc.iter().all(|b| b.is_ascii_graphic()) {
                    Some(String::from_utf8(fourcc.to_vec()).ok()?)
                } else {
                    Some(hex_string(&fourcc))
                };
                recognized += 1;
            }
            _ => r.skip(wire)?,
        }
    }
    // Random bytes can survive the field loop; require at least one field we
    // actually understand before claiming this was a Widevine payload.
    (recognized > 0).then_some(out)
}

// ---------- PlayReady (PlayReady Object + WRMHEADER XML) ----------

fn utf16le_to_string(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units)
        .ok()
        .map(|s| s.trim_start_matches('\u{feff}').to_string())
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Text content of the first `<tag>...</tag>` element.
fn xml_element_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&close)?;
    Some(&xml[start..end])
}

/// Value of `attr="..."` inside the given tag text.
fn xml_attr_value<'a>(tag_text: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = tag_text.find(&needle)? + needle.len();
    let end = start + tag_text[start..].find('"')?;
    Some(&tag_text[start..end])
}

/// PlayReady KIDs are base64 of the KID GUID in Microsoft's mixed-endian
/// layout; the first three GUID fields are little-endian. Reversing them
/// yields the big-endian byte order CENC uses.
fn guid_le_to_be(mut guid: [u8; 16]) -> [u8; 16] {
    guid[0..4].reverse();
    guid[4..6].reverse();
    guid[6..8].reverse();
    guid
}

fn playready_kid_to_hex(b64: &str) -> Option<String> {
    let bytes = crate::util::base64_decode(b64.trim())?;
    let guid: [u8; 16] = bytes.try_into().ok()?;
    Some(hex_string(&guid_le_to_be(guid)))
}

/// All KIDs in a WRMHEADER, across its versions: 4.0 uses `<KID>base64</KID>`,
/// 4.1+ use `<KID ... VALUE="base64">` (nested in `<KIDS>` for 4.2/4.3).
fn extract_playready_kids(xml: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut search = 0usize;
    while let Some(found) = xml[search..].find("<KID") {
        let after = search + found + "<KID".len();
        search = after;
        match xml.as_bytes().get(after) {
            Some(b'>') => {
                if let Some(end) = xml[after + 1..].find("</KID>")
                    && let Some(kid) = playready_kid_to_hex(&xml[after + 1..after + 1 + end])
                {
                    out.push(kid);
                }
            }
            Some(c) if c.is_ascii_whitespace() => {
                if let Some(end) = xml[after..].find('>')
                    && let Some(value) = xml_attr_value(&xml[after..after + end], "VALUE")
                    && let Some(kid) = playready_kid_to_hex(value)
                {
                    out.push(kid);
                }
            }
            // "<KIDS" and other tags sharing the prefix.
            _ => {}
        }
    }
    out.dedup();
    out
}

/// Parses a bare PlayReady pssh payload (a PlayReady Object). Returns `None`
/// for anything that doesn't carry a readable WRMHEADER record.
pub fn parse_playready_pssh_data(data: &[u8]) -> Option<PlayReadyPsshData> {
    if data.len() < 10 {
        return None;
    }
    let total = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if total < 10 || total > data.len() {
        return None;
    }
    let record_count = u16::from_le_bytes(data[4..6].try_into().unwrap());
    let mut pos = 6usize;
    let mut xml: Option<String> = None;
    for _ in 0..record_count {
        if pos + 4 > total {
            return None;
        }
        let record_type = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap());
        let record_len = u16::from_le_bytes(data[pos + 2..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + record_len > total {
            return None;
        }
        // Type 1 is the rights-management header; 2 and 3 are reserved / an
        // embedded license store, which carry no fields we surface.
        if record_type == 1 && xml.is_none() {
            xml = utf16le_to_string(&data[pos..pos + record_len]);
        }
        pos += record_len;
    }
    let xml = xml?;
    let wrm_header_version = xml
        .find("<WRMHEADER")
        .and_then(|i| {
            let tag_end = i + xml[i..].find('>')?;
            xml_attr_value(&xml[i..tag_end], "version")
        })
        .map(str::to_string);
    Some(PlayReadyPsshData {
        record_count,
        wrm_header_version,
        key_ids: extract_playready_kids(&xml),
        la_url: xml_element_text(&xml, "LA_URL").map(xml_unescape),
        xml: Some(xml),
    })
}

// ---------- pssh box parsing ----------

/// Parses the body of a `pssh` box: everything after the FullBox
/// version/flags. Decodes the system-specific data blob when the system is
/// recognized; unknown systems still yield the generic fields.
pub(crate) fn parse_pssh_body(version: u8, flags: u32, body: &[u8]) -> anyhow::Result<PsshData> {
    if body.len() < 16 {
        anyhow::bail!("pssh body truncated ({} bytes)", body.len());
    }
    let system_id: [u8; 16] = body[0..16].try_into().unwrap();
    let mut pos = 16usize;

    let mut key_ids = Vec::new();
    if version >= 1 {
        if pos + 4 > body.len() {
            anyhow::bail!("pssh KID count truncated");
        }
        let kid_count = u32::from_be_bytes(body[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + kid_count.saturating_mul(16) > body.len() {
            anyhow::bail!("pssh KID list truncated");
        }
        for _ in 0..kid_count {
            key_ids.push(hex_string(&body[pos..pos + 16]));
            pos += 16;
        }
    }

    if pos + 4 > body.len() {
        anyhow::bail!("pssh data size truncated");
    }
    let data_size = u32::from_be_bytes(body[pos..pos + 4].try_into().unwrap());
    pos += 4;
    // Tolerate a data blob shorter than declared; parse what is there.
    let data = &body[pos..pos + (data_size as usize).min(body.len() - pos)];

    let widevine = (system_id == WIDEVINE_SYSTEM_ID)
        .then(|| parse_widevine_pssh_data(data))
        .flatten()
        .map(Box::new);
    let playready = (system_id == PLAYREADY_SYSTEM_ID)
        .then(|| parse_playready_pssh_data(data))
        .flatten()
        .map(Box::new);

    Ok(PsshData {
        version,
        flags,
        system_id: uuid_string(&system_id),
        system_name: drm_system_name(&system_id).map(str::to_string),
        key_ids,
        data_size,
        widevine,
        playready,
    })
}

/// Parses one or more concatenated raw `pssh` boxes — the form carried by
/// DASH `cenc:pssh` manifest elements. Fails on anything that isn't a
/// well-formed pssh box, so callers can use it as a format probe.
pub fn parse_pssh_boxes(data: &[u8]) -> anyhow::Result<Vec<PsshData>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while data.len() - pos >= 8 {
        if &data[pos + 4..pos + 8] != b"pssh" {
            anyhow::bail!("not a pssh box at offset {pos}");
        }
        let declared = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        // size 0 means "extends to the end"; largesize (1) never occurs in
        // practice for pssh and is rejected by the minimum below.
        let size = if declared == 0 {
            data.len() - pos
        } else {
            declared
        };
        if size < 12 || pos + size > data.len() {
            anyhow::bail!("invalid pssh box size {declared} at offset {pos}");
        }
        let payload = &data[pos + 8..pos + size];
        let version = payload[0];
        let flags = u32::from_be_bytes([0, payload[1], payload[2], payload[3]]);
        out.push(parse_pssh_body(version, flags, &payload[4..])?);
        pos += size;
    }
    if out.is_empty() {
        anyhow::bail!("no pssh box found");
    }
    Ok(out)
}

/// Wraps a bare Widevine payload (protobuf without box framing, as found in
/// packager logs) in a synthetic [`PsshData`]. Returns `None` when the bytes
/// don't decode as Widevine.
pub fn pssh_from_raw_widevine(data: &[u8]) -> Option<PsshData> {
    let widevine = parse_widevine_pssh_data(data)?;
    Some(PsshData {
        version: 0,
        flags: 0,
        system_id: uuid_string(&WIDEVINE_SYSTEM_ID),
        system_name: Some("Widevine".to_string()),
        key_ids: Vec::new(),
        data_size: data.len() as u32,
        widevine: Some(Box::new(widevine)),
        playready: None,
    })
}
