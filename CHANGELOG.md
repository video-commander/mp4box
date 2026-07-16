# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.0]

### Added

- **Per-field payload byte spans (`field_spans`).** Decoders can now report
  where each payload field lives inside a box, so consumers can highlight
  individual fields in a hex view and cross-reference them with decoded values.
  New `FieldSpan { name, start, length }` type (payload-relative offsets) and an
  opt-in `BoxDecoder::field_spans(version, flags, payload_len)` trait method that
  defaults to empty, leaving existing decoders unchanged. Spans are derived from
  box metadata **without reading the payload**, making them cheap and independent
  of `decode()`; `Box` gains a `field_spans` field populated under `decode=true`.
  Implemented for the fixed-layout boxes — `mvhd`, `tkhd`, `mdhd`, `hdlr`,
  `tenc`, `tfhd`, `tfdt`, `trex`, `sidx`, `elst`, `trun`, `emsg` (v1), and the
  sample-table headers (`stsz`/`stco`/`co64`/`stts`/`stss`/`ctts`/`stsc`) — with
  widths and presence tracking version and flag bits. Variable-first or
  descriptor-based layouts (`emsg` v0, `stz2`, `esds`), text-decoded boxes, and
  the repeating bodies of sample tables and index arrays stay header-only by
  design, since their per-entry offsets can only be known by reading the payload.
- **Human-readable CICP colour code points.** The `colr` (nclx) and `vpcC`
  decoders now name the colour primaries, transfer characteristics, and matrix
  coefficients (ISO/IEC 23091-2) instead of printing bare integers — e.g.
  `transfer=16 (PQ / SMPTE ST 2084)` and `transfer=18 (HLG / ARIB STD-B67)`,
  making HDR signalling legible at a glance. Lookup tables are exposed at
  `registry::cicp` for reuse.
- **`dvcC` / `dvvC` (Dolby Vision Configuration Box) decoding.** Both records
  are now decoded into `dv_version`, `dv_profile`, `dv_level`, the
  RPU/EL/BL presence flags, and the base-layer cross-compatibility id with its
  meaning (e.g. `bl_compatibility=1 (HDR10 (BT.2020 PQ))`). `dvvC` is also newly
  recognized as a known box.
- **Structured HDR/colour metadata.** The `colr`, `dvcC`/`dvvC`, `mdcv`, and
  `clli` decoders now expose typed data (`StructuredData::ColourInformation`,
  `DolbyVisionConfig`, `MasteringDisplayColourVolume`, `ContentLightLevel`)
  instead of only a text summary, so callers can read the fields directly and
  UIs render them as attribute lists. Text summaries are unchanged.

## [0.11.0]

### Added

- **`iods` (Object Descriptor Box) recognition and decoding.** The box is now
  identified (ISO/IEC 14496-14 §5.1) and its wrapped MPEG-4
  `InitialObjectDescriptor` decoded (ISO/IEC 14496-1 §7.2.6.4): object
  descriptor ID, inline URL (when the URL flag is set), and the five
  profile-level indications (OD, scene, audio, visual, graphics). Accepts both
  the `InitialObjectDescrTag` (0x02) and MP4 IOD tag (0x10); malformed
  descriptors fall back to raw bytes. Exposed as
  `StructuredData::ObjectDescriptor(IodsData)`.
- **Recognition of the Google/YouTube proprietary boxes** `gsst`, `gstd`,
  `gssd`, `gspu`, `gspm`, `gshh` (found under `moov/udta`). These are now
  labeled with human-readable names instead of showing as unknown; their
  payload layouts are undocumented, so contents are left as raw leaves.

### Changed

- The `registry` module was split from a single ~3.2k-line file into a
  directory module (`mod`, `data`, `decoders`, `codec_config`). This is an
  internal reorganization with no public API change: all `registry::*` paths
  are preserved via re-exports.

## [0.10.0]

### Added

- **System-specific `pssh` payload decoding** (new `drm` module). The 0.9.0
  decoders identified the DRM system; the data blob now decodes too:
  - **Widevine**: dependency-free protobuf decoder for the `WidevinePsshData`
    schema — algorithm, key IDs, provider, content ID (hex + printable-text
    form), policy, crypto period index, protection scheme (cenc/cbcs/cens/cbc1).
    Unknown fields are skipped; garbage input is rejected rather than
    misdecoded.
  - **PlayReady**: PlayReady Object parser — length-prefixed records, UTF-16LE
    `WRMHEADER` XML (versions 4.0–4.3), key IDs converted from the
    little-endian GUID layout to CENC byte order, `LA_URL`, and the full
    header XML.
- `drm::parse_pssh_boxes`: parse one or more concatenated raw `pssh` boxes
  outside a file context (the form carried by DASH `cenc:pssh` elements);
  `drm::pssh_from_raw_widevine` wraps bare Widevine payloads from packager
  logs; `WIDEVINE_SYSTEM_ID` / `PLAYREADY_SYSTEM_ID` constants.
- `util::base64_decode`: dependency-free standard/URL-safe base64 decoding
  (padding optional).

### Changed

- **Breaking**: `PsshData` gained `widevine` and `playready` fields (boxed,
  `Option`, omitted from JSON when absent). Code constructing `PsshData` with
  a struct literal or destructuring it exhaustively must be updated; JSON
  consumers are unaffected.
- The `pssh` one-line summary now surfaces decoded payload highlights
  (provider, protection scheme, WRMHEADER version, license URL).

## [0.9.0]

A correctness-focused overhaul that also adds non-destructive editing,
fragmented-MP4 support, tolerant parsing, and DRM/DASH decoders. The library now
depends only on `anyhow` + `serde` for parse-only builds.

### Added

- **Non-destructive box editing** (`edit` module, `mp4edit` CLI — on by default).
  - Extent-tree design: unmodified boxes stay as byte-range references into the
    source; serializing an unedited tree reproduces the file byte-for-byte, and
    `mdat` is streamed rather than held in memory.
  - Ancestor box sizes are recomputed bottom-up (correct by construction);
    `stco`/`co64` offsets are remapped through an exact old→new extent map — no
    heuristics.
  - Commands: `Remove` / `RemoveAll` / `Insert` / `Replace` / `Set` / `SetTag` /
    `Faststart`, addressed by slash paths with `[n]` indexing and `©` fourccs.
  - `Set` patches named fields in place (mvhd/tkhd/mdhd timestamps, timescale,
    duration, rate, volume, track_id, layer, width/height, language) with
    version-aware offsets — no re-encoding.
  - `SetTag` builds the `moov/udta/meta/ilst` chain (ffmpeg-compatible `hdlr`)
    when missing.
  - **Faststart**: move `moov` before `mdat` for progressive playback (like
    `qt-faststart` / `ffmpeg -movflags +faststart`); idempotent.
- **Fragmented MP4 (fMP4 / DASH / CMAF) sample extraction.**
  `track_samples_from_reader` / `_from_path` now walk `moof/traf/trun`,
  implementing the ISO 14496-12 §8.8 defaulting chain (trun → first_sample_flags
  → tfhd → trex), with decode times from `tfdt` (running-DTS fallback across
  fragments) and byte offsets from `tfhd` base or the enclosing `moof`.
- **Tolerant parsing.** `parse_boxes_tolerant` / `get_boxes_tolerant` return the
  partial tree plus located `ParseIssue`s (offset + description); damage is
  contained to the enclosing container. `mp4dump` is now tolerant by default
  (partial tree, issues on stderr, exit code 2); `--strict` restores fail-fast.
- **Typed structured decoders** for movie/fragment boxes: `MovieHeader`,
  `EditList`, `SegmentIndex`, `TrackFragmentHeader`, `TrackFragmentDecodeTime`,
  `TrackExtends` (elst/sidx include full entry lists with SAP info).
- **DRM/DASH decoders**: `pssh` (Widevine/PlayReady/FairPlay/ClearKey/Marlin
  recognition, v1 KIDs), `tenc`, `emsg`, `senc`, `schm`, `frma` — verified
  end-to-end against a real CENC-encrypted file.
- **AAC**: `esds` now decodes `AudioSpecificConfig` (object type, profile,
  core/extension sample rates, channels, SBR/PS) — HE-AAC and HE-AAC v2 now
  report correctly instead of as AAC-LC.
- New decoders: `stz2`, `avcC`, `hvcC`.
- New cargo features: `edit` (default, no extra deps) and `cli` (default; gates
  clap + serde_json).
- New examples: `edit.rs`, `fragments.rs`, `media_info.rs`.

### Fixed

- **FullBox decoding**: mvhd/tkhd/elst/sidx no longer re-parse already-stripped
  version/flags (every field was shifted 4+ bytes — mvhd reported garbage
  timescale/duration, tkhd always `track_id=0`).
- tkhd v0 now reads 4-byte timestamps; mdhd honors v1 64-bit times.
- New `FullContainer` node kind fixes `meta`/`iref`/`trep`/`stsd` children
  previously parsed as garbage; QuickTime-style `meta` is auto-detected.
- `stsd` now recurses into sample entries and their codec-config children, making
  the esds/dOps/colr/pasp/btrt decoders reachable on real files.
- Box classification fixes (dinf/tref/schi as containers; schm/tenc/senc/emsg/…
  as FullBoxes; accept the spec `Opus` fourcc).
- `tfhd` flag mixup fixed (`0x010000` is duration_is_empty, not
  default_base_is_moof).
- Sample tables: `track_samples_*` work again on v0-tkhd files; sample building
  is a single O(n) pass; `mp4samples` prints real stts/ctts/stsc/stco/stsz
  instead of values scraped from Debug strings.

### Changed

- **Dependencies slimmed 7 → 2** for library consumers (transitive tree 33 → 11
  crates): dropped `byteorder` (small `ReadExt` trait), `thiserror`
  (hand-written impls), and `hex` (unused). clap and serde_json are gated behind
  the default-on `cli` feature; parse-only builds use
  `default-features = false, features = ["edit"]`.
- MSRV set to Rust 1.88; refreshed crate metadata (description, keywords,
  categories); added MIT LICENSE file.

## [0.8.0]

### Added

- `get_itunes_tags` API: walks `moov → udta → meta → ilst` and extracts UTF-8
  values from iTunes metadata atoms (©nam, ©ART, ©alb, ©day, ©gen, ©cmt, ©des,
  desc, cprt, aART), returning a map keyed by ffmpeg `-metadata` names (title,
  artist, album, year, genre, comment, description, copyright, album_artist).

## [0.7.0]

### Added

- 57 new box types in the `KnownBox` enum: codec-config (esds, av1C, vpcC, dOps,
  dac3, dec3, dfLa, dvcC, btrt), iTunes metadata (ilst, data, mean, name),
  subtitle/text (tx3g, wvtt, stpp, sthd, vttC), Dolby Vision, encrypted sample
  entries (encv, enca, enct), PCM audio, HDR (mdcv, clli), HEIF extras, 360
  video (st3d, sv3d, proj, …), DASH/CMAF (kind, ludt), and QuickTime boxes.
- 22 new registry decoders: btrt, esds, av1C, vpcC, dOps, dac3, dec3, dfLa,
  colr, pasp, mdcv, clli, kind, irot, imir, data/mean/name (iTunes), and
  fragment boxes trun/tfhd/tfdt/trex.
- `mp4dump` shows the full box name alongside the 4CC and formats structured
  values as human-readable strings; `examples/boxes.rs` does a full tree walk
  with per-track sample-table stats.

## [0.6.0]

### Added

- Direct structured parsing of decoded box payloads, laying the groundwork for
  sample-table extraction.

### Fixed

- Corrected `timescale` and `duration` parsing.
- Corrected track-id decoding.

## [0.5.0]

### Added

- Reader-based public APIs: parsing functions now accept any `Read + Seek`
  source (local files or remote URLs) rather than a path only. Examples, tests,
  and docs updated to match.

## [0.4.0]

### Added

- `stsd` decoding to extract codec, width, and height.
- `mp4info` binary and accompanying example.

### Fixed

- Corrected language-code parsing in the `mdhd` decoder.

## [0.3.0]

### Added

- JSON output API for `mp4dump`.
- `KnownBox` enum with full box names for most known box types.
- Additional core boxes and an initial release CI pipeline.

## [0.2.0]

### Added

- JSON API, full box-name lookup, and coverage of most known boxes.

## [0.1.0]

### Added

- Initial release: basic MP4/ISOBMFF box-tree parser with an example and the
  `mp4dump` tool.

[0.10.0]: https://github.com/alfg/mp4box/releases/tag/v0.10.0
[0.9.0]: https://github.com/alfg/mp4box/releases/tag/v0.9.0
[0.8.0]: https://github.com/alfg/mp4box/releases/tag/v0.8.0
[0.7.0]: https://github.com/alfg/mp4box/releases/tag/v0.7.0
[0.6.0]: https://github.com/alfg/mp4box/releases/tag/v0.6.0
[0.5.0]: https://github.com/alfg/mp4box/releases/tag/v0.5.0
[0.4.0]: https://github.com/alfg/mp4box/releases/tag/v0.4.0
[0.3.0]: https://github.com/alfg/mp4box/releases/tag/v0.3.0
