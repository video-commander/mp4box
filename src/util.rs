use std::io::{Read, Seek, SeekFrom};

/// Minimal big-endian (and one little-endian) read helpers over any
/// [`Read`], replacing the `byteorder` dependency. Implemented for unsized
/// types so it works on `&mut dyn Read`.
pub trait ReadExt: Read {
    fn read_u8(&mut self) -> std::io::Result<u8> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn read_u16_be(&mut self) -> std::io::Result<u16> {
        let mut b = [0u8; 2];
        self.read_exact(&mut b)?;
        Ok(u16::from_be_bytes(b))
    }
    fn read_i16_be(&mut self) -> std::io::Result<i16> {
        let mut b = [0u8; 2];
        self.read_exact(&mut b)?;
        Ok(i16::from_be_bytes(b))
    }
    fn read_u32_be(&mut self) -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_be_bytes(b))
    }
    fn read_i32_be(&mut self) -> std::io::Result<i32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(i32::from_be_bytes(b))
    }
    fn read_u64_be(&mut self) -> std::io::Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_be_bytes(b))
    }
    fn read_i64_be(&mut self) -> std::io::Result<i64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(i64::from_be_bytes(b))
    }
    /// Little-endian: the Opus `dOps` input sample rate keeps Ogg byte order.
    fn read_u32_le(&mut self) -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
}

impl<R: Read + ?Sized> ReadExt for R {}

pub fn read_slice<R: Read + Seek>(r: &mut R, offset: u64, len: u64) -> std::io::Result<Vec<u8>> {
    r.seek(SeekFrom::Start(offset))?;
    let mut v = vec![0u8; len as usize];
    r.read_exact(&mut v)?;
    Ok(v)
}

pub fn hex_dump(bytes: &[u8], start_offset: u64) -> String {
    // Simple hexdump
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let offs = start_offset + (i as u64) * 16;
        let hexs: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();
        let ascii: String = chunk
            .iter()
            .map(|b| {
                let c = *b;
                if (32..=126).contains(&c) {
                    c as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{:08x}  {:<48}  |{}|\n", offs, hexs, ascii));
    }
    out
}
