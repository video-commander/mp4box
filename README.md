# mp4box

[![Crates.io](https://img.shields.io/crates/v/mp4box.svg)](https://crates.io/crates/mp4box)
[![Docs.rs](https://docs.rs/mp4box/badge.svg)](https://docs.rs/mp4box)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust Version](https://img.shields.io/badge/rust-1.70+-orange.svg)

A minimal, dependency-light MP4/ISOBMFF parser for Rust.  
Parses the MP4 box tree, supports large-size and UUID boxes, and exposes a full known-box table with human-readable names.  
Includes an optional pluggable decoder registry for structured box interpretation.  
Suitable for CLIs, Tauri backends, media inspectors, and developer tooling.

---

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
mp4box = "0.7.0"
anyhow = "1.0"  # For error handling in examples
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

- **Zero-dependency parser** (`Read + Seek`)
- **Full MP4/ISOBMFF box tree**
  - Leaf, FullBox (version/flags), Container, UUID
- **Large-size (64-bit) support**
- **UUID box support**
- **Known-Box registry**
  - `ftyp → File Type Box`
  - `moov → Movie Box`
  - Hundreds of ISO/HEIF/MPEG boxes with full names
- **Custom decoders**
  - Attach your own parser for any 4CC or UUID
- **Frontend-friendly JSON**
  - Works perfectly in Tauri or WebView apps

---

## Example: Parse the box tree

```rust
use mp4box::parser::{read_box_header, parse_children};
use mp4box::boxes::{BoxRef, NodeKind};
use std::fs::File;
use std::io::{Seek, SeekFrom};

fn main() -> anyhow::Result<()> {
    let mut f = File::open("input.mp4")?;
    let file_len = f.metadata()?.len();

    while f.stream_position()? < file_len {
        let h = read_box_header(&mut f)?;
        println!("Found box {} @ {:#x}, size={}", h.typ, h.start, h.size);

        let end = if h.size == 0 { file_len } else { h.start + h.size };
        f.seek(SeekFrom::Start(end))?;
    }

    Ok(())
}
```

---

## Example: Build a JSON tree (for GUIs / Tauri)

```rust
use mp4box::get_boxes;
use std::fs::File;

let mut file = File::open("video.mp4")?;
let size = file.metadata()?.len();
let boxes = get_boxes(&mut file, size, /*decode=*/ false)?;
println!("{} top-level boxes", boxes.len());
```

This returns:

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

---

## Example: Command-line tool (`mp4dump`)

`mp4dump` is included as an optional binary that uses this crate to inspect MP4 files.

### Show structure

```bash
$ mp4dump input.mp4
   0x0         32 ftyp
  0x20          8 free
  0x28    3374542 mdat
0x337df6     170458 moov (container)
  0x337dfe        108 mvhd (ver=0)
  ...
```

### Decode known boxes

```bash
$ mp4dump input.mp4 --decode
   0x0         32 ftyp
        -> major=isom minor=512 compatible=["isom","iso2","avc1","mp41"]
```

```bash
$ mp4dump input.mp4 --decode --json
[
  {
    "offset": 0,
    "size": 32,
    "typ": "ftyp",
    "uuid": null,
    "version": null,
    "flags": null,
    "kind": "leaf",
    "full_name": "File Type Box",
    "decoded": "major=isom minor=512 compatible=[\"isom\", \"iso2\", \"avc1\", \"mp41\"]",
    "children": null
  },
  ...
```

### Dump raw bytes (like `xxd`)

```bash
$ mp4dump input.mp4 --raw stsd --bytes 256
== Dump stsd payload: offset=0x337fe6, len=156 ==
00000000: 00 00 00 00 00 00 00 01 61 76 63 31 ...
`````

---

## Adding Custom Box Decoders

```rust
use mp4box::registry::{Registry, BoxDecoder, BoxValue};
use mp4box::boxes::{BoxHeader, BoxKey};
use std::io::Read;

struct MyDecoder;

impl BoxDecoder for MyDecoder {
    fn decode(&self, r: &mut dyn Read, _hdr: &BoxHeader) -> anyhow::Result<BoxValue> {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        Ok(BoxValue::Bytes(buf))
    }
}

let reg = Registry::new()
    .with_decoder(BoxKey::FourCC(*b"ftyp"), "ftyp", Box::new(MyDecoder));
```

---

## License

MIT
