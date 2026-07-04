//! `Set` support: version-aware, in-place patching of named fields inside
//! known boxes. Only the addressed bytes change — matrices, reserved
//! regions, and unknown trailing data are preserved verbatim, and no
//! re-encoding is involved.

/// Wire format of a patchable field.
#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    U32,
    U64,
    I16,
    /// 16.16 fixed point, set from a decimal value (e.g. width, rate)
    Fixed1616,
    /// 8.8 fixed point (volume)
    Fixed88,
    /// ISO-639-2/T packed 3-letter language code
    Lang,
}

impl FieldKind {
    pub fn width(&self) -> usize {
        match self {
            FieldKind::I16 | FieldKind::Fixed88 | FieldKind::Lang => 2,
            FieldKind::U32 | FieldKind::Fixed1616 => 4,
            FieldKind::U64 => 8,
        }
    }
}

/// Byte offset (relative to the start of the box payload, i.e. the version
/// byte) and kind of `field` in a `fourcc` box of the given version.
/// Returns `None` for unknown box/field combinations.
pub fn field_spec(fourcc: &[u8; 4], version: u8, field: &str) -> Option<(usize, FieldKind)> {
    use FieldKind::*;
    // All offsets are 4 + <offset within the post-version/flags body>.
    let spec = match (fourcc, version, field) {
        // mvhd v0: creation(4) modification(4) timescale(4) duration(4)
        //          rate(4) volume(2) ...
        (b"mvhd", 0, "creation_time") => (4, U32),
        (b"mvhd", 0, "modification_time") => (8, U32),
        (b"mvhd", 0, "timescale") => (12, U32),
        (b"mvhd", 0, "duration") => (16, U32),
        (b"mvhd", 0, "rate") => (20, Fixed1616),
        (b"mvhd", 0, "volume") => (24, Fixed88),
        // reserved(10) matrix(36) pre_defined(24) => next_track_id
        (b"mvhd", 0, "next_track_id") => (24 + 2 + 10 + 36 + 24, U32),

        // mvhd v1: creation(8) modification(8) timescale(4) duration(8) ...
        (b"mvhd", 1, "creation_time") => (4, U64),
        (b"mvhd", 1, "modification_time") => (12, U64),
        (b"mvhd", 1, "timescale") => (20, U32),
        (b"mvhd", 1, "duration") => (24, U64),
        (b"mvhd", 1, "rate") => (32, Fixed1616),
        (b"mvhd", 1, "volume") => (36, Fixed88),
        (b"mvhd", 1, "next_track_id") => (36 + 2 + 10 + 36 + 24, U32),

        // tkhd v0: creation(4) modification(4) track_id(4) reserved(4)
        //          duration(4) reserved(8) layer(2) alternate_group(2)
        //          volume(2) reserved(2) matrix(36) width(4) height(4)
        (b"tkhd", 0, "creation_time") => (4, U32),
        (b"tkhd", 0, "modification_time") => (8, U32),
        (b"tkhd", 0, "track_id") => (12, U32),
        (b"tkhd", 0, "duration") => (20, U32),
        (b"tkhd", 0, "layer") => (32, I16),
        (b"tkhd", 0, "alternate_group") => (34, I16),
        (b"tkhd", 0, "volume") => (36, Fixed88),
        (b"tkhd", 0, "width") => (40 + 36, Fixed1616),
        (b"tkhd", 0, "height") => (40 + 36 + 4, Fixed1616),

        // tkhd v1: creation(8) modification(8) track_id(4) reserved(4)
        //          duration(8) ...
        (b"tkhd", 1, "creation_time") => (4, U64),
        (b"tkhd", 1, "modification_time") => (12, U64),
        (b"tkhd", 1, "track_id") => (20, U32),
        (b"tkhd", 1, "duration") => (28, U64),
        (b"tkhd", 1, "layer") => (44, I16),
        (b"tkhd", 1, "alternate_group") => (46, I16),
        (b"tkhd", 1, "volume") => (48, Fixed88),
        (b"tkhd", 1, "width") => (52 + 36, Fixed1616),
        (b"tkhd", 1, "height") => (52 + 36 + 4, Fixed1616),

        // mdhd v0: creation(4) modification(4) timescale(4) duration(4)
        //          language(2)
        (b"mdhd", 0, "creation_time") => (4, U32),
        (b"mdhd", 0, "modification_time") => (8, U32),
        (b"mdhd", 0, "timescale") => (12, U32),
        (b"mdhd", 0, "duration") => (16, U32),
        (b"mdhd", 0, "language") => (20, Lang),

        // mdhd v1: creation(8) modification(8) timescale(4) duration(8)
        //          language(2)
        (b"mdhd", 1, "creation_time") => (4, U64),
        (b"mdhd", 1, "modification_time") => (12, U64),
        (b"mdhd", 1, "timescale") => (20, U32),
        (b"mdhd", 1, "duration") => (24, U64),
        (b"mdhd", 1, "language") => (32, Lang),

        _ => return None,
    };
    Some(spec)
}

/// Parse `value` per the field kind and patch it into `payload` at `offset`.
pub fn patch_field(
    payload: &mut [u8],
    offset: usize,
    kind: FieldKind,
    value: &str,
) -> anyhow::Result<()> {
    let width = kind.width();
    anyhow::ensure!(
        offset + width <= payload.len(),
        "field at offset {} does not fit in a {}-byte payload",
        offset,
        payload.len()
    );
    let dst = &mut payload[offset..offset + width];

    match kind {
        FieldKind::U32 => dst.copy_from_slice(&value.parse::<u32>()?.to_be_bytes()),
        FieldKind::U64 => dst.copy_from_slice(&value.parse::<u64>()?.to_be_bytes()),
        FieldKind::I16 => dst.copy_from_slice(&value.parse::<i16>()?.to_be_bytes()),
        FieldKind::Fixed1616 => {
            let v = value.parse::<f64>()?;
            anyhow::ensure!(
                (0.0..65536.0).contains(&v),
                "value {} out of range for 16.16 fixed point",
                v
            );
            dst.copy_from_slice(&((v * 65536.0).round() as u32).to_be_bytes());
        }
        FieldKind::Fixed88 => {
            let v = value.parse::<f64>()?;
            anyhow::ensure!(
                (0.0..256.0).contains(&v),
                "value {} out of range for 8.8 fixed point",
                v
            );
            dst.copy_from_slice(&((v * 256.0).round() as u16).to_be_bytes());
        }
        FieldKind::Lang => {
            let b = value.as_bytes();
            anyhow::ensure!(
                b.len() == 3 && b.iter().all(|c| c.is_ascii_lowercase()),
                "language must be a 3-letter lowercase ISO-639-2 code, got {:?}",
                value
            );
            let packed: u16 = (((b[0] - 0x60) as u16) << 10)
                | (((b[1] - 0x60) as u16) << 5)
                | ((b[2] - 0x60) as u16);
            dst.copy_from_slice(&packed.to_be_bytes());
        }
    }
    Ok(())
}
