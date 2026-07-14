//! Coding-independent code point (CICP) lookups.
//!
//! ISO/IEC 23091-2 assigns numeric code points to colour primaries, transfer
//! characteristics, and matrix coefficients. These same values appear in the
//! `colr` (nclx) box, `vpcC`, AV1 sequence headers, and HEVC/AVC VUI. The
//! functions here map the raw numbers to short human-readable names so decoded
//! box output reads as `transfer=16 (PQ / SMPTE ST 2084)` instead of a bare
//! integer.
//!
//! Only the entries relevant to real-world MP4 delivery are named; anything
//! unassigned/reserved falls through to `None` and callers print the number
//! alone.

/// Short name for a colour-primaries code point (CICP `ColourPrimaries`).
pub fn primaries_name(code: u16) -> Option<&'static str> {
    Some(match code {
        1 => "BT.709",
        2 => "unspecified",
        4 => "BT.470M",
        5 => "BT.470BG / BT.601 625",
        6 => "BT.601 525 / SMPTE 170M",
        7 => "SMPTE 240M",
        8 => "Film (C)",
        9 => "BT.2020",
        10 => "SMPTE ST 428 (XYZ)",
        11 => "SMPTE RP 431-2 (DCI P3)",
        12 => "SMPTE EG 432-1 (Display P3)",
        22 => "EBU Tech 3213-E",
        _ => return None,
    })
}

/// Short name for a transfer-characteristics code point (CICP
/// `TransferCharacteristics`). This is the field that distinguishes HDR
/// transfer functions (PQ = 16, HLG = 18) from SDR ones.
pub fn transfer_name(code: u16) -> Option<&'static str> {
    Some(match code {
        1 => "BT.709",
        2 => "unspecified",
        4 => "BT.470M (gamma 2.2)",
        5 => "BT.470BG (gamma 2.8)",
        6 => "BT.601 / SMPTE 170M",
        7 => "SMPTE 240M",
        8 => "linear",
        9 => "log 100:1",
        10 => "log 316:1",
        11 => "IEC 61966-2-4 (xvYCC)",
        12 => "BT.1361",
        13 => "sRGB / sYCC",
        14 => "BT.2020 10-bit",
        15 => "BT.2020 12-bit",
        16 => "PQ / SMPTE ST 2084",
        17 => "SMPTE ST 428 (XYZ)",
        18 => "HLG / ARIB STD-B67",
        _ => return None,
    })
}

/// Short name for a matrix-coefficients code point (CICP `MatrixCoefficients`).
pub fn matrix_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0 => "Identity (RGB/GBR)",
        1 => "BT.709",
        2 => "unspecified",
        4 => "FCC 73.682",
        5 => "BT.470BG / BT.601 625",
        6 => "BT.601 525 / SMPTE 170M",
        7 => "SMPTE 240M",
        8 => "YCgCo",
        9 => "BT.2020 non-constant luminance",
        10 => "BT.2020 constant luminance",
        11 => "SMPTE ST 2085 (YDzDx)",
        12 => "Chromaticity-derived NCL",
        13 => "Chromaticity-derived CL",
        14 => "ICtCp",
        _ => return None,
    })
}

/// Formats a code point as `N (Name)` when named, or `N` when unassigned.
pub fn labeled(code: u16, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{code} ({n})"),
        None => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_hdr_code_points() {
        assert_eq!(primaries_name(9), Some("BT.2020"));
        assert_eq!(transfer_name(16), Some("PQ / SMPTE ST 2084"));
        assert_eq!(transfer_name(18), Some("HLG / ARIB STD-B67"));
        assert_eq!(matrix_name(9), Some("BT.2020 non-constant luminance"));
    }

    #[test]
    fn unassigned_falls_through() {
        assert_eq!(transfer_name(200), None);
        assert_eq!(labeled(200, transfer_name(200)), "200");
        assert_eq!(labeled(16, transfer_name(16)), "16 (PQ / SMPTE ST 2084)");
    }
}
