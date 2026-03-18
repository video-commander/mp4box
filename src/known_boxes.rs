use crate::boxes::FourCC;

/// Typed view over common MP4 / ISOBMFF boxes.
///
/// Anything not in this list becomes `KnownBox::Unknown(fourcc)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownBox {
    // File-level / top-level
    Ftyp,
    Moov,
    Mdat,
    Free,
    Skip,
    Wide,
    Meta,
    Pssh,
    Sidx,
    Ssix,
    Prft,
    Styp,
    Emsg,
    Mfra,
    Mfro,
    Pdin,

    // moov children
    Mvhd,
    Trak,
    Mvex,
    Udta,

    // trak children
    Tkhd,
    Edts,
    Mdia,
    Tref,
    Iprp,
    Meco,
    Ludt,

    // edts children
    Elst,

    // mdia children
    Mdhd,
    Hdlr,
    Minf,

    // minf children
    Vmhd,
    Smhd,
    Hmhd,
    Nmhd,
    Sthd,
    Dinf,
    Stbl,
    Gmhd,

    // gmhd children
    Gmin,
    Glbl,

    // ludt children
    Kind,

    // dinf children
    Dref,

    // stbl children
    Stsd,
    Stts,
    Ctts,
    Stsc,
    Stsz,
    Stz2,
    Stco,
    Co64,
    Stss,
    Stsh,
    Padb,
    Stdp,
    Sdtp,
    Sgpd,
    Sbgp,
    Subs,

    // fragmented / mvex / moof / traf
    Mehd,
    Trex,
    Moof,
    Mfhd,
    Traf,
    Tfhd,
    Tfdt,
    Trun,
    Tfra,

    // meta / HEIF-ish
    Iloc,
    Iinf,
    Infe,
    Iref,
    Ipco,
    Ipma,
    Ipci,
    Ispe,
    Pixi,
    AuxC,
    Clap,
    Colr,
    Hvcc,
    Avcc,
    Pitm,
    Irot,
    Imir,
    Rloc,
    Lsel,
    Tols,
    A1lx,
    A1op,
    Idat,
    Ipro,

    // Encryption / CENC
    Sinf,
    Schm,
    Schi,
    Tenc,
    Saio,
    Saiz,
    Senc,
    Frma,

    // Sample entries (video)
    Avc1,
    Avc2,
    Avc3,
    Avc4,
    Hev1,
    Hvc1,
    Vvc1,
    Mp4v,
    Vp08,
    Vp09,
    Av01,
    Dvh1,
    Dvhe,
    Dav1,
    Tx3g,
    Wvtt,
    Stpp,
    Tmcd,
    Encv,
    Enca,
    Enct,
    Ipcm,
    Fpcm,

    // Sample entries (audio)
    Mp4a,
    Ac3,
    Ec3,
    Opus,
    Samr,
    Sawb,
    Alac,
    Flac,

    // Codec configuration boxes (children of sample entries)
    Esds,
    Av1c,
    Vpcc,
    Dops,
    Dac3,
    Dec3,
    Dfla,
    Dvcc,
    Btrt,

    // iTunes metadata
    Ilst,
    IlstData,
    Mean,
    IlstName,

    // WebVTT
    Vttc,

    // HDR / color metadata
    Mdcv,
    Clli,

    // Spherical / 360 video
    St3d,
    Sv3d,
    Proj,
    Prhd,
    Equi,
    Cbmp,

    // ISOBMFF extras
    Xml,
    Bxml,
    Ainf,
    Leva,
    Trep,

    // PCM audio
    Srat,
    Chnl,
    Pcmc,

    // QuickTime-specific
    Wave,
    Chan,
    Tcmi,

    // Misc / QT-ish / common extras
    Pasp,
    Cslg,
    Cprt,
    Gama,
    Fiel,
    Tapt,

    // Raw UUID/vendor
    Uuid,

    // Anything else
    Unknown(FourCC),
}

impl From<FourCC> for KnownBox {
    fn from(cc: FourCC) -> Self {
        match &cc.0 {
            b"ftyp" => KnownBox::Ftyp,
            b"moov" => KnownBox::Moov,
            b"mdat" => KnownBox::Mdat,
            b"free" => KnownBox::Free,
            b"skip" => KnownBox::Skip,
            b"wide" => KnownBox::Wide,
            b"meta" => KnownBox::Meta,
            b"pssh" => KnownBox::Pssh,
            b"sidx" => KnownBox::Sidx,
            b"ssix" => KnownBox::Ssix,
            b"prft" => KnownBox::Prft,
            b"styp" => KnownBox::Styp,
            b"emsg" => KnownBox::Emsg,
            b"mfra" => KnownBox::Mfra,
            b"mfro" => KnownBox::Mfro,
            b"pdin" => KnownBox::Pdin,

            b"mvhd" => KnownBox::Mvhd,
            b"trak" => KnownBox::Trak,
            b"mvex" => KnownBox::Mvex,
            b"udta" => KnownBox::Udta,

            b"tkhd" => KnownBox::Tkhd,
            b"edts" => KnownBox::Edts,
            b"mdia" => KnownBox::Mdia,
            b"tref" => KnownBox::Tref,
            b"iprp" => KnownBox::Iprp,
            b"meco" => KnownBox::Meco,
            b"ludt" => KnownBox::Ludt,

            b"elst" => KnownBox::Elst,

            b"mdhd" => KnownBox::Mdhd,
            b"hdlr" => KnownBox::Hdlr,
            b"minf" => KnownBox::Minf,

            b"vmhd" => KnownBox::Vmhd,
            b"smhd" => KnownBox::Smhd,
            b"hmhd" => KnownBox::Hmhd,
            b"nmhd" => KnownBox::Nmhd,
            b"sthd" => KnownBox::Sthd,
            b"dinf" => KnownBox::Dinf,
            b"stbl" => KnownBox::Stbl,
            b"gmhd" => KnownBox::Gmhd,

            b"gmin" => KnownBox::Gmin,
            b"glbl" => KnownBox::Glbl,

            b"kind" => KnownBox::Kind,

            b"dref" => KnownBox::Dref,

            b"stsd" => KnownBox::Stsd,
            b"stts" => KnownBox::Stts,
            b"ctts" => KnownBox::Ctts,
            b"stsc" => KnownBox::Stsc,
            b"stsz" => KnownBox::Stsz,
            b"stz2" => KnownBox::Stz2,
            b"stco" => KnownBox::Stco,
            b"co64" => KnownBox::Co64,
            b"stss" => KnownBox::Stss,
            b"stsh" => KnownBox::Stsh,
            b"padb" => KnownBox::Padb,
            b"stdp" => KnownBox::Stdp,
            b"sdtp" => KnownBox::Sdtp,
            b"sgpd" => KnownBox::Sgpd,
            b"sbgp" => KnownBox::Sbgp,
            b"subs" => KnownBox::Subs,

            b"mehd" => KnownBox::Mehd,
            b"trex" => KnownBox::Trex,
            b"moof" => KnownBox::Moof,
            b"mfhd" => KnownBox::Mfhd,
            b"traf" => KnownBox::Traf,
            b"tfhd" => KnownBox::Tfhd,
            b"tfdt" => KnownBox::Tfdt,
            b"trun" => KnownBox::Trun,
            b"tfra" => KnownBox::Tfra,

            b"iloc" => KnownBox::Iloc,
            b"iinf" => KnownBox::Iinf,
            b"infe" => KnownBox::Infe,
            b"iref" => KnownBox::Iref,
            b"ipco" => KnownBox::Ipco,
            b"ipma" => KnownBox::Ipma,
            b"ipci" => KnownBox::Ipci,
            b"ispe" => KnownBox::Ispe,
            b"pixi" => KnownBox::Pixi,
            b"auxC" => KnownBox::AuxC,
            b"clap" => KnownBox::Clap,
            b"colr" => KnownBox::Colr,
            b"hvcC" => KnownBox::Hvcc,
            b"avcC" => KnownBox::Avcc,
            b"pitm" => KnownBox::Pitm,
            b"irot" => KnownBox::Irot,
            b"imir" => KnownBox::Imir,
            b"rloc" => KnownBox::Rloc,
            b"lsel" => KnownBox::Lsel,
            b"tols" => KnownBox::Tols,
            b"a1lx" => KnownBox::A1lx,
            b"a1op" => KnownBox::A1op,
            b"idat" => KnownBox::Idat,
            b"ipro" => KnownBox::Ipro,

            b"sinf" => KnownBox::Sinf,
            b"schm" => KnownBox::Schm,
            b"schi" => KnownBox::Schi,
            b"tenc" => KnownBox::Tenc,
            b"saio" => KnownBox::Saio,
            b"saiz" => KnownBox::Saiz,
            b"senc" => KnownBox::Senc,
            b"frma" => KnownBox::Frma,

            b"avc1" => KnownBox::Avc1,
            b"avc2" => KnownBox::Avc2,
            b"avc3" => KnownBox::Avc3,
            b"avc4" => KnownBox::Avc4,
            b"hev1" => KnownBox::Hev1,
            b"hvc1" => KnownBox::Hvc1,
            b"vvc1" => KnownBox::Vvc1,
            b"mp4v" => KnownBox::Mp4v,
            b"vp08" => KnownBox::Vp08,
            b"vp09" => KnownBox::Vp09,
            b"av01" => KnownBox::Av01,
            b"dvh1" => KnownBox::Dvh1,
            b"dvhe" => KnownBox::Dvhe,
            b"dav1" => KnownBox::Dav1,
            b"tx3g" => KnownBox::Tx3g,
            b"wvtt" => KnownBox::Wvtt,
            b"stpp" => KnownBox::Stpp,
            b"tmcd" => KnownBox::Tmcd,
            b"encv" => KnownBox::Encv,
            b"enca" => KnownBox::Enca,
            b"enct" => KnownBox::Enct,
            b"ipcm" => KnownBox::Ipcm,
            b"fpcm" => KnownBox::Fpcm,

            b"mp4a" => KnownBox::Mp4a,
            b"ac-3" => KnownBox::Ac3,
            b"ec-3" => KnownBox::Ec3,
            b"opus" => KnownBox::Opus,
            b"samr" => KnownBox::Samr,
            b"sawb" => KnownBox::Sawb,
            b"alac" => KnownBox::Alac,
            b"fLaC" => KnownBox::Flac,

            b"esds" => KnownBox::Esds,
            b"av1C" => KnownBox::Av1c,
            b"vpcC" => KnownBox::Vpcc,
            b"dOps" => KnownBox::Dops,
            b"dac3" => KnownBox::Dac3,
            b"dec3" => KnownBox::Dec3,
            b"dfLa" => KnownBox::Dfla,
            b"dvcC" => KnownBox::Dvcc,
            b"btrt" => KnownBox::Btrt,

            b"ilst" => KnownBox::Ilst,
            b"data" => KnownBox::IlstData,
            b"mean" => KnownBox::Mean,
            b"name" => KnownBox::IlstName,

            b"vttC" => KnownBox::Vttc,

            b"mdcv" => KnownBox::Mdcv,
            b"clli" => KnownBox::Clli,

            b"st3d" => KnownBox::St3d,
            b"sv3d" => KnownBox::Sv3d,
            b"proj" => KnownBox::Proj,
            b"prhd" => KnownBox::Prhd,
            b"equi" => KnownBox::Equi,
            b"cbmp" => KnownBox::Cbmp,

            b"xml " => KnownBox::Xml,
            b"bxml" => KnownBox::Bxml,
            b"ainf" => KnownBox::Ainf,
            b"leva" => KnownBox::Leva,
            b"trep" => KnownBox::Trep,

            b"srat" => KnownBox::Srat,
            b"chnl" => KnownBox::Chnl,
            b"pcmC" => KnownBox::Pcmc,

            b"wave" => KnownBox::Wave,
            b"chan" => KnownBox::Chan,
            b"tcmi" => KnownBox::Tcmi,

            b"pasp" => KnownBox::Pasp,
            b"cslg" => KnownBox::Cslg,
            b"cprt" => KnownBox::Cprt,
            b"gama" => KnownBox::Gama,
            b"fiel" => KnownBox::Fiel,
            b"tapt" => KnownBox::Tapt,

            b"uuid" => KnownBox::Uuid,

            _ => KnownBox::Unknown(cc),
        }
    }
}

impl KnownBox {
    /// Returns `true` if this box type is a container (i.e. can have children).
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            KnownBox::Moov
                | KnownBox::Trak
                | KnownBox::Mdia
                | KnownBox::Minf
                | KnownBox::Stbl
                | KnownBox::Edts
                | KnownBox::Udta
                | KnownBox::Meta
                | KnownBox::Moof
                | KnownBox::Mvex
                | KnownBox::Mfra
                | KnownBox::Meco
                | KnownBox::Traf
                | KnownBox::Sinf
                | KnownBox::Iprp
                | KnownBox::Iref
                | KnownBox::Ipco
                | KnownBox::Ipma
                | KnownBox::Ilst
                | KnownBox::Ludt
                | KnownBox::Gmhd
                | KnownBox::Ipro
                | KnownBox::Sv3d
                | KnownBox::Proj
                | KnownBox::Trep
                | KnownBox::Wave
        )
    }

    /// Returns `true` if this box type is a FullBox (has version/flags).
    pub fn is_full_box(&self) -> bool {
        matches!(
            self,
            KnownBox::Mvhd
                | KnownBox::Tkhd
                | KnownBox::Mdhd
                | KnownBox::Hdlr
                | KnownBox::Vmhd
                | KnownBox::Smhd
                | KnownBox::Nmhd
                | KnownBox::Sthd
                | KnownBox::Dref
                | KnownBox::Stts
                | KnownBox::Ctts
                | KnownBox::Stsc
                | KnownBox::Stsz
                | KnownBox::Stz2
                | KnownBox::Stco
                | KnownBox::Co64
                | KnownBox::Stss
                | KnownBox::Stsh
                | KnownBox::Padb
                | KnownBox::Stdp
                | KnownBox::Stsd
                | KnownBox::Sdtp
                | KnownBox::Sgpd
                | KnownBox::Sbgp
                | KnownBox::Subs
                | KnownBox::Elst
                | KnownBox::Sidx
                | KnownBox::Mehd
                | KnownBox::Trex
                | KnownBox::Mfhd
                | KnownBox::Tfhd
                | KnownBox::Tfdt
                | KnownBox::Trun
                | KnownBox::Tfra
                | KnownBox::Iloc
                | KnownBox::Iinf
                | KnownBox::Infe
                | KnownBox::Pitm
                | KnownBox::Pssh
                | KnownBox::Schi
                | KnownBox::Saio
                | KnownBox::Saiz
                | KnownBox::Esds
                | KnownBox::Vpcc
                | KnownBox::Dfla
                | KnownBox::Kind
                | KnownBox::St3d
                | KnownBox::Prhd
                | KnownBox::Equi
                | KnownBox::Cbmp
                | KnownBox::Tols
                | KnownBox::A1op
                | KnownBox::Pdin
                | KnownBox::Xml
                | KnownBox::Bxml
                | KnownBox::Ainf
                | KnownBox::Leva
                | KnownBox::Srat
                | KnownBox::Chnl
                | KnownBox::Gmin
                | KnownBox::Mean
                | KnownBox::IlstName
                | KnownBox::IlstData
        )
    }
}

impl KnownBox {
    /// Human-readable name suitable for UI (e.g. "File Type Box").
    pub fn full_name(&self) -> &'static str {
        match self {
            KnownBox::Ftyp => "File Type Box",
            KnownBox::Moov => "Movie Box",
            KnownBox::Mdat => "Media Data Box",
            KnownBox::Free => "Free Space Box",
            KnownBox::Skip => "Skip Box",
            KnownBox::Wide => "Wide Placeholder Box",
            KnownBox::Meta => "Metadata Box",
            KnownBox::Pssh => "Protection System Specific Header",
            KnownBox::Sidx => "Segment Index Box",
            KnownBox::Ssix => "Subsegment Index Box",
            KnownBox::Prft => "Producer Reference Time",
            KnownBox::Styp => "Segment Type Box",
            KnownBox::Emsg => "Event Message Box",
            KnownBox::Mfra => "Movie Fragment Random Access Box",
            KnownBox::Mfro => "Movie Fragment Random Access Offset Box",
            KnownBox::Pdin => "Progressive Download Information Box",
            KnownBox::Mvhd => "Movie Header Box",
            KnownBox::Trak => "Track Box",
            KnownBox::Mvex => "Movie Extends Box",
            KnownBox::Udta => "User Data Box",
            KnownBox::Tkhd => "Track Header Box",
            KnownBox::Edts => "Edit Box",
            KnownBox::Mdia => "Media Box",
            KnownBox::Tref => "Track Reference Box",
            KnownBox::Iprp => "Item Properties Box",
            KnownBox::Meco => "Additional Metadata Container Box",
            KnownBox::Ludt => "Track User Data Box",
            KnownBox::Elst => "Edit List Box",
            KnownBox::Mdhd => "Media Header Box",
            KnownBox::Hdlr => "Handler Reference Box",
            KnownBox::Minf => "Media Information Box",
            KnownBox::Vmhd => "Video Media Header Box",
            KnownBox::Smhd => "Sound Media Header Box",
            KnownBox::Hmhd => "Hint Media Header Box",
            KnownBox::Nmhd => "Null Media Header Box",
            KnownBox::Sthd => "Subtitle Media Header Box",
            KnownBox::Dinf => "Data Information Box",
            KnownBox::Stbl => "Sample Table Box",
            KnownBox::Gmhd => "Generic Media Header Box",
            KnownBox::Gmin => "Base Media Information Header Box",
            KnownBox::Glbl => "Global Sample Description Box",
            KnownBox::Kind => "Track Kind Box",
            KnownBox::Dref => "Data Reference Box",
            KnownBox::Stsd => "Sample Description Box",
            KnownBox::Stts => "Decoding Time-to-Sample Box",
            KnownBox::Ctts => "Composition Time-to-Sample Box",
            KnownBox::Stsc => "Sample-to-Chunk Box",
            KnownBox::Stsz => "Sample Size Box",
            KnownBox::Stz2 => "Compact Sample Size Box",
            KnownBox::Stco => "Chunk Offset Box",
            KnownBox::Co64 => "Chunk Offset (64-bit) Box",
            KnownBox::Stss => "Sync Sample Box",
            KnownBox::Stsh => "Shadow Sync Sample Box",
            KnownBox::Padb => "Padding Bits Box",
            KnownBox::Stdp => "Sample Degradation Priority Box",
            KnownBox::Sdtp => "Sample Dependency Flags Box",
            KnownBox::Sgpd => "Sample Group Description Box",
            KnownBox::Sbgp => "Sample-to-Group Box",
            KnownBox::Subs => "Sub-Sample Information Box",
            KnownBox::Mehd => "Movie Extends Header Box",
            KnownBox::Trex => "Track Extends Box",
            KnownBox::Moof => "Movie Fragment Box",
            KnownBox::Mfhd => "Movie Fragment Header Box",
            KnownBox::Traf => "Track Fragment Box",
            KnownBox::Tfhd => "Track Fragment Header Box",
            KnownBox::Tfdt => "Track Fragment Decode Time Box",
            KnownBox::Trun => "Track Fragment Run Box",
            KnownBox::Tfra => "Track Fragment Random Access Box",
            KnownBox::Iloc => "Item Location Box",
            KnownBox::Iinf => "Item Information Box",
            KnownBox::Infe => "Item Info Entry Box",
            KnownBox::Iref => "Item Reference Box",
            KnownBox::Ipco => "Item Property Container Box",
            KnownBox::Ipma => "Item Property Association Box",
            KnownBox::Ipci => "Item Property Container Info Box",
            KnownBox::Ispe => "Image Spatial Extents Property",
            KnownBox::Pixi => "Pixel Information Property",
            KnownBox::AuxC => "Auxiliary Type Property",
            KnownBox::Clap => "Clean Aperture Box",
            KnownBox::Colr => "Colour Information Box",
            KnownBox::Hvcc => "HEVC Decoder Configuration Box",
            KnownBox::Avcc => "AVC Decoder Configuration Box",
            KnownBox::Pitm => "Primary Item Box",
            KnownBox::Irot => "Image Rotation Box",
            KnownBox::Imir => "Image Mirror Box",
            KnownBox::Rloc => "Relative Location Box",
            KnownBox::Lsel => "Layer Selector Box",
            KnownBox::Tols => "Target Output Layer Set Box",
            KnownBox::A1lx => "AV1 Layer Extents Box",
            KnownBox::A1op => "AV1 Operating Point Selector Box",
            KnownBox::Idat => "Item Data Box",
            KnownBox::Ipro => "Item Protection Box",
            KnownBox::Sinf => "Protection Scheme Information Box",
            KnownBox::Schm => "Scheme Type Box",
            KnownBox::Schi => "Scheme Information Box",
            KnownBox::Tenc => "Track Encryption Box",
            KnownBox::Saio => "Sample Auxiliary Information Offsets Box",
            KnownBox::Saiz => "Sample Auxiliary Information Sizes Box",
            KnownBox::Senc => "Sample Encryption Box",
            KnownBox::Frma => "Original Format Box",
            KnownBox::Avc1 => "AVC Video Sample Entry",
            KnownBox::Avc2 => "AVC2 Video Sample Entry",
            KnownBox::Avc3 => "AVC3 Video Sample Entry",
            KnownBox::Avc4 => "AVC4 Video Sample Entry",
            KnownBox::Hev1 => "HEVC Video Sample Entry (hev1)",
            KnownBox::Hvc1 => "HEVC Video Sample Entry (hvc1)",
            KnownBox::Vvc1 => "VVC Video Sample Entry",
            KnownBox::Mp4v => "MPEG-4 Visual Sample Entry",
            KnownBox::Vp08 => "VP8 Video Sample Entry",
            KnownBox::Vp09 => "VP9 Video Sample Entry",
            KnownBox::Av01 => "AV1 Video Sample Entry",
            KnownBox::Dvh1 => "Dolby Vision HEVC Sample Entry (dvh1)",
            KnownBox::Dvhe => "Dolby Vision HEVC Sample Entry (dvhe)",
            KnownBox::Dav1 => "Dolby Vision AV1 Sample Entry",
            KnownBox::Tx3g => "3GPP Timed Text Sample Entry",
            KnownBox::Wvtt => "WebVTT Sample Entry",
            KnownBox::Stpp => "TTML Sample Entry",
            KnownBox::Tmcd => "Timecode Sample Entry",
            KnownBox::Encv => "Encrypted Video Sample Entry",
            KnownBox::Enca => "Encrypted Audio Sample Entry",
            KnownBox::Enct => "Encrypted Text Sample Entry",
            KnownBox::Ipcm => "In-band PCM Sample Entry",
            KnownBox::Fpcm => "Float PCM Sample Entry",
            KnownBox::Mp4a => "MPEG-4 Audio Sample Entry",
            KnownBox::Ac3 => "AC-3 Audio Sample Entry",
            KnownBox::Ec3 => "Enhanced AC-3 Audio Sample Entry",
            KnownBox::Opus => "Opus Audio Sample Entry",
            KnownBox::Samr => "AMR-NB Audio Sample Entry",
            KnownBox::Sawb => "AMR-WB Audio Sample Entry",
            KnownBox::Alac => "Apple Lossless Sample Entry",
            KnownBox::Flac => "FLAC Audio Sample Entry",
            KnownBox::Esds => "Elementary Stream Descriptor",
            KnownBox::Av1c => "AV1 Codec Configuration Box",
            KnownBox::Vpcc => "VP Codec Configuration Box",
            KnownBox::Dops => "Opus Specific Box",
            KnownBox::Dac3 => "AC-3 Bitstream Information Box",
            KnownBox::Dec3 => "Enhanced AC-3 Bitstream Information Box",
            KnownBox::Dfla => "FLAC Specific Box",
            KnownBox::Dvcc => "Dolby Vision Configuration Box",
            KnownBox::Btrt => "Bitrate Box",
            KnownBox::Ilst => "iTunes Metadata List",
            KnownBox::IlstData => "iTunes Metadata Value",
            KnownBox::Mean => "iTunes Reverse DNS Domain",
            KnownBox::IlstName => "iTunes Reverse DNS Name",
            KnownBox::Vttc => "WebVTT Configuration Box",
            KnownBox::Mdcv => "Mastering Display Color Volume Box",
            KnownBox::Clli => "Content Light Level Information Box",
            KnownBox::St3d => "Stereo 3D Box",
            KnownBox::Sv3d => "Spherical Video V2 Box",
            KnownBox::Proj => "Projection Box",
            KnownBox::Prhd => "Projection Header Box",
            KnownBox::Equi => "Equirectangular Projection Box",
            KnownBox::Cbmp => "Cube Map Projection Box",
            KnownBox::Xml => "XML Box",
            KnownBox::Bxml => "Binary XML Box",
            KnownBox::Ainf => "Asset Information Box",
            KnownBox::Leva => "Level Assignment Box",
            KnownBox::Trep => "Track Extension Properties Box",
            KnownBox::Srat => "Sampling Rate Box",
            KnownBox::Chnl => "Channel Layout Box",
            KnownBox::Pcmc => "PCM Configuration Box",
            KnownBox::Wave => "WAVE Configuration Box",
            KnownBox::Chan => "Audio Channel Layout Box",
            KnownBox::Tcmi => "Timecode Media Information Box",
            KnownBox::Pasp => "Pixel Aspect Ratio Box",
            KnownBox::Cslg => "Composition Shift Least Greatest Box",
            KnownBox::Cprt => "Copyright Box",
            KnownBox::Gama => "Gamma Box",
            KnownBox::Fiel => "Field Handling Box",
            KnownBox::Tapt => "Track Aperture Mode Dimensions Box",
            KnownBox::Uuid => "UUID Box",
            KnownBox::Unknown(_) => "Unknown Box",
        }
    }
}
