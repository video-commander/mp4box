# mp4box

[![Crates.io](https://img.shields.io/crates/v/mp4box.svg)](https://crates.io/crates/mp4box)
[![Docs.rs](https://docs.rs/mp4box/badge.svg)](https://docs.rs/mp4box)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust Version](https://img.shields.io/badge/rust-1.88+-orange.svg)

A minimal, dependency-light MP4/ISOBMFF parser and editor for Rust.
Parses the full box tree (including codec configuration inside `stsd`), decodes known boxes into typed structures, extracts per-sample tables from both progressive and fragmented (fMP4/DASH/CMAF) files, reads iTunes metadata, and performs non-destructive box editing with automatic size and chunk-offset fixup.
Suitable for CLIs, Tauri backends, media inspectors, and developer tooling.

---

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
mp4box = "0.9.0"
anyhow = "1.0"  # For error handling in examples
```

The library core depends only on `anyhow` and `serde`. Two default-on
features add the rest:

- `edit` — non-destructive box editing (no extra dependencies)
- `cli` — the `mp4dump`/`mp4info`/`mp4samples`/`mp4edit` binaries
  (pulls in `clap` and `serde_json`)

For the lightest library-only build:

```toml
mp4box = { version = "0.9.0", default-features = false, features = ["edit"] }
```

---

## Quick Start

```rust
use mp4box::get_boxes;
use std::fs::File;

fn main() -> anyhow::Result<()> {
    let mut file = File::open("video.mp4")?;
    let size = file.metadata()?.len();

    // Parse all boxes without decoding
    let boxes = get_boxes(&mut file, size, false)?;
    println!("Found {} top-level boxes", boxes.len());

    // Parse with decoding for known box types
    let mut file = File::open("video.mp4")?;
    let decoded_boxes = get_boxes(&mut file, size, true)?;

    // Print decoded info for ftyp box
    if let Some(ftyp) = decoded_boxes.iter().find(|b| b.typ == "ftyp") {
        println!("File type: {}", ftyp.decoded.as_ref().unwrap_or(&"unknown".to_string()));
    }

    Ok(())
}
```

---

## Features

- **Zero-dependency core parser** (`Read + Seek`)
- **Full MP4/ISOBMFF box tree**
  - Leaf, FullBox (version/flags), Container, FullBox containers (`meta`, `stsd`), UUID
  - Large-size (64-bit) boxes; recursion into `stsd` sample entries and their
    codec configuration children (`avcC`, `hvcC`, `esds`, `dOps`, ...)
- **Known-box registry** — hundreds of ISO/HEIF/MPEG boxes with full names
- **Typed structured decoding** — `mvhd`, `tkhd`, `mdhd`, `stsd`, the whole
  sample table family, `elst`, `sidx`, and the fragment boxes
  (`tfhd`/`tfdt`/`trun`/`trex`) decode to serializable structs
- **Sample tables** — per-sample DTS/PTS, duration, size, file offset, and
  keyframe flags for progressive *and* fragmented (fMP4/DASH/CMAF) files
- **iTunes metadata** — read tags with `get_itunes_tags`, write them with the
  edit API
- **Non-destructive editing** (`edit` feature) — remove/insert/replace boxes,
  patch header fields, set tags; box sizes and `stco`/`co64` chunk offsets
  are fixed up automatically
- **Tolerant parsing** — `get_boxes_tolerant` recovers from malformed boxes,
  returning the partial tree plus located issues instead of an error
- **Custom decoders** — attach your own parser for any 4CC or UUID
- **Frontend-friendly JSON** — works perfectly in Tauri or WebView apps
- **CLI tools** — `mp4dump`, `mp4info`, `mp4samples`, `mp4edit`

---

## Example: Build a JSON tree (for GUIs / Tauri)

```rust
use mp4box::get_boxes;
use std::fs::File;

let mut file = File::open("video.mp4")?;
let size = file.metadata()?.len();
let boxes = get_boxes(&mut file, size, /*decode=*/ true)?;
println!("{} top-level boxes", boxes.len());
```

This returns a serializable tree:

```json
[
  {
    "offset": 0,
    "size": 32,
    "typ": "ftyp",
    "full_name": "File Type Box",
    "kind": "leaf",
    "decoded": "major=isom minor=512 compatible=[...]",
    "children": null
  }
]
```

Known boxes also carry `structured_data` — typed values instead of strings:

```rust
use mp4box::registry::StructuredData;

if let Some(StructuredData::MovieHeader(mvhd)) = &boxes[1].children.as_ref().unwrap()[0].structured_data {
    println!("duration: {:.2}s", mvhd.duration as f64 / mvhd.timescale as f64);
}
```

---

## Example: Per-sample tables (progressive and fragmented)

```rust
use mp4box::track_samples_from_path;

for track in track_samples_from_path("video.mp4")? {
    let keyframes = track.samples.iter().filter(|s| s.is_sync).count();
    println!(
        "track {} ({}): {} samples, {} keyframes, {:.2}s",
        track.track_id,
        track.handler_type,
        track.sample_count,
        keyframes,
        track.duration as f64 / track.timescale as f64,
    );
    // each sample: dts, pts, duration, size, file_offset, is_sync
}
```

Fragmented files (`moof`/`traf`/`trun`) are handled transparently: samples are
assembled across all fragments using the tfhd/trex defaulting rules, with
byte offsets and keyframe flags matching what ffprobe reports.

---

## Example: iTunes metadata

```rust
use mp4box::get_itunes_tags;
use std::fs::File;

let mut file = File::open("video.mp4")?;
let size = file.metadata()?.len();
let tags = get_itunes_tags(&mut file, size)?;
// {"title": "...", "artist": "...", "encoder": "...", "track": "3/12", ...}
```

---

## Example: Editing (requires the `edit` feature)

Editing is non-destructive: the source file is never modified, untouched
bytes are streamed through verbatim (an `mdat` is never loaded into memory),
and ancestor box sizes plus `stco`/`co64` chunk offsets are recomputed
automatically. Re-serializing with no edits reproduces the input byte for
byte.

```rust
use mp4box::edit::Editor;

let mut editor = Editor::new();
editor.set_tag("title", "My Movie")?;
editor.set_field("moov/mvhd", "creation_time", "0");
editor.remove("moov/udta/meta");           // paths support indices: moov/trak[1]/...
editor.remove_all("free");                 // strip every `free` box
editor.faststart();                        // move moov before mdat

let stats = editor.process_file("in.mp4", "out.mp4")?;
println!("{} chunk offsets adjusted", stats.chunk_offsets_adjusted);
```

Fragmented (`moof`/`sidx`) and HEIF (`iloc`) files are refused with a clear
error rather than corrupted — their internal offsets are not covered by the
fixup pass yet.

---

## Command-line tools

### `mp4dump` — box tree explorer

```bash
$ mp4dump input.mp4
   0x0         32 ftyp (File Type Box)
  0x20          8 free (Free Space Box)
  0x28    3374542 mdat (Media Data Box)
0x337df6     170458 moov (Movie Box) (container)
  0x337dfe        108 mvhd (Movie Header Box) (ver=0, flags=0x000000)
  ...
```

```bash
$ mp4dump input.mp4 --decode
   0x0         32 ftyp (File Type Box)
        -> major=isom minor=512 compatible=["isom", "iso2", "avc1", "mp41"]
...
           0x191        174 stsd (Sample Description Box) (container, ver=0, flags=0x000000)
        -> codec=avc1 1920x1080 entries=1
             0x1a1        158 avc1 (AVC Video Sample Entry) (container)
               0x1f7         56 avcC (AVC Decoder Configuration Box)
        -> configurationVersion=1 profile=100 compat=0x00 level=4.0 nal_length_size=4
```

Filter a subtree, emit JSON, or dump raw payload bytes:

```bash
$ mp4dump input.mp4 --filter 'moov/trak[0]/mdia/minf/stbl'
$ mp4dump input.mp4 --decode --json
$ mp4dump input.mp4 --raw stsd --bytes 256
```

Damaged files are handled gracefully by default: parsing recovers past
malformed boxes, prints the partial tree, reports what was wrong and where
on stderr, and exits with code 2. Use `--strict` to fail on the first
malformed box instead.

```bash
$ mp4dump damaged.mp4
...
2 parse issue(s):
  0x294b4a2: box 'mdia' declares size 284285 which overruns its container by 84287 bytes; clamped
  0x2960979: box 'stsz' declares size 126484 which overruns its container by 13741 bytes; clamped
$ echo $?
2
```

### `mp4info` — media summary

```bash
$ mp4info input.mp4
File: input.mp4
Major brand: isom
Minor version: 512
Compatible brands: isom, iso2, avc1, mp41
Movie duration: 600000 ticks @ 1000 -> 600.000 s
Tracks:
  Track 1:
    type: video
    codec: avc1
    size: 320x180
    timescale: 12800
    duration: 7680000 ticks -> 600.000 s
    language: und
```

### `mp4samples` — per-sample tables

```bash
$ mp4samples input.mp4 --limit 3
Track 1 (vide) timescale=12800 duration=7680000 sample_count=15000
idx    start(s)   dur(ts)  size   offset      sync
----------------------------------------------------
    0     0.0800      512   3417         48 *
    1     0.1600      512    264       3465
    2     0.1200      512    159       3729
```

### `mp4edit` — non-destructive editor

```bash
$ mp4edit --tag title="My Movie" --tag artist="Me" \
          --set moov/mvhd.creation_time=0 \
          --remove-all free \
          input.mp4 output.mp4
wrote output.mp4 (158008454 bytes, 2336 chunk offsets adjusted)
```

Optimize for progressive playback (moov before mdat, like `qt-faststart`):

```bash
$ mp4edit --faststart input.mp4 output.mp4
```

---

## Adding Custom Box Decoders

Extend the default registry with a decoder for any 4CC or UUID; the decoded
value shows up in the same tree as the built-ins.

```rust
use mp4box::{BoxHeader, BoxKey, FourCC, get_boxes_with_registry};
use mp4box::registry::{BoxDecoder, BoxValue, default_registry};
use std::fs::File;
use std::io::Read;

struct MyDecoder;

impl BoxDecoder for MyDecoder {
    fn decode(
        &self,
        r: &mut dyn Read,
        _hdr: &BoxHeader,
        _version: Option<u8>,
        _flags: Option<u32>,
    ) -> anyhow::Result<BoxValue> {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        Ok(BoxValue::Text(format!("{} bytes", buf.len())))
    }
}

let reg = default_registry().with_decoder(
    BoxKey::FourCC(FourCC(*b"xyz ")),
    "xyz ",
    Box::new(MyDecoder),
);

let mut file = File::open("video.mp4")?;
let size = file.metadata()?.len();
let boxes = get_boxes_with_registry(&mut file, size, true, &reg)?;
```

---

## Examples

Runnable examples live in [`examples/`](examples/):

| Example | Shows |
|---|---|
| `boxes` | Walking the box tree with names and version/flags |
| `media_info` | Typed structured data: movie/track headers, codec details, edit lists, tags |
| `samples` | Sample table analysis |
| `fragments` | Fragmented MP4 inspection and assembled per-track samples |
| `edit` | Setting tags and zeroing timestamps with the edit API |
| `simple` | Hex-dumping a byte range |

```bash
cargo run --example media_info -- video.mp4
```

---

## License

MIT
