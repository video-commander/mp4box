use std::fmt;
use std::str::FromStr;

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct FourCC(pub [u8; 4]);

impl FourCC {
    pub fn as_str_lossy(&self) -> String {
        self.0
            .iter()
            .map(|&c| {
                if (32..=126).contains(&c) {
                    c as char
                } else {
                    '.'
                }
            })
            .collect()
    }
}
impl fmt::Debug for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str_lossy())
    }
}
impl fmt::Display for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str_lossy())
    }
}

impl From<[u8; 4]> for FourCC {
    fn from(b: [u8; 4]) -> Self {
        FourCC(b)
    }
}

impl FromStr for FourCC {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let b = s.as_bytes();
        if b.len() == 4 {
            Ok(FourCC([b[0], b[1], b[2], b[3]]))
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct BoxHeader {
    pub size: u64,   // total size including header, or 0=to parent end
    pub typ: FourCC, // 4CC or b"uuid"
    pub uuid: Option<[u8; 16]>,
    pub header_size: u64, // 8, 16, or 24
    pub start: u64,       // file offset of header start
}

#[derive(Debug)]
pub enum NodeKind {
    Container(Vec<BoxRef>),
    /// A FullBox that also contains child boxes (e.g. `meta`, `iref`, `stsd`).
    /// `data_offset`/`data_len` cover the payload after version/flags, which
    /// may include non-box fields (e.g. stsd's entry_count) before the children.
    FullContainer {
        version: u8,
        flags: u32,
        data_offset: u64,
        data_len: u64,
        children: Vec<BoxRef>,
    },
    FullBox {
        version: u8,
        flags: u32,
        data_offset: u64,
        data_len: u64,
    },
    Leaf {
        data_offset: u64,
        data_len: u64,
    },
    Unknown {
        data_offset: u64,
        data_len: u64,
    },
}

#[derive(Debug)]
pub struct BoxRef {
    pub hdr: BoxHeader,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BoxKey {
    FourCC(FourCC),
    Uuid([u8; 16]),
}
