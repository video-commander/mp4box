//! MPEG codec-configuration parsing shared across decoders:
//! AudioSpecificConfig (ISO 14496-3) and MPEG-4 descriptor lengths.

use super::data::AudioSpecificConfig;

pub(crate) fn read_descriptor_length(buf: &[u8], pos: &mut usize) -> Option<u32> {
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
pub(crate) fn parse_audio_specific_config(buf: &[u8]) -> Option<AudioSpecificConfig> {
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
