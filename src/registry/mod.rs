//! Box decoder registry: the [`BoxDecoder`] trait, the [`Registry`] that
//! maps box keys to decoders, and [`default_registry`] wiring up the
//! built-in decoders. Decoder implementations live in [`decoders`], the
//! structured payload types in [`data`], and shared codec parsing in
//! [`codec_config`].

use crate::boxes::{BoxHeader, BoxKey, FourCC};
use std::collections::HashMap;
use std::io::Read;

mod codec_config;
mod data;
mod decoders;

pub use data::*;
pub use decoders::*;

/// A value returned from a box decoder.
///
/// Decoders may return either a human-readable text summary, raw bytes, or structured data.
#[derive(Debug, Clone)]
pub enum BoxValue {
    Text(String),
    Bytes(Vec<u8>),
    Structured(StructuredData),
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

// ---------- Default registry ----------
pub fn default_registry() -> Registry {
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
        .with_decoder(
            BoxKey::FourCC(FourCC(*b"iods")),
            "iods",
            Box::new(IodsDecoder),
        )
}
