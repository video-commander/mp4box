# CLAUDE.md

Guidance for working in the `mp4box` crate: a dependency-light MP4/ISOBMFF
parser, decoder, and non-destructive editor. Core parsing depends only on
`anyhow` + `serde`; `clap`/`serde_json` are pulled in solely by the CLI.

## Commands

```bash
cargo build                      # default features: edit + cli
cargo test                       # all unit + integration tests
cargo test --test decoder_regressions   # one integration test file
cargo fmt --check                # CI-checked; run cargo fmt before committing
cargo clippy --all-targets       # expected to be warning-clean

# Library-only consumers build without the CLI:
cargo build --no-default-features --features edit
```

Edition 2024, MSRV 1.88. Always run `cargo fmt` and `cargo clippy --all-targets`
before finishing a change — both are gates.

## Layout

- `src/lib.rs` — crate docs + the public re-export surface. The API is what's
  re-exported here (`get_boxes`, `Box`, `registry::*`, `parser::*`, ...).
- `src/parser.rs` — low-level box walking: `BoxHeader`, `read_box_header`,
  `parse_boxes`, tolerant parsing (`parse_boxes_tolerant` → `ParseIssue`).
- `src/boxes.rs` — `BoxKey`, `FourCC`, `NodeKind` (Leaf / FullBox / Container /
  FullContainer / Unknown).
- `src/api.rs` — the JSON-facing `Box` struct and `get_boxes*`. `build_box`
  turns a parsed node into a `Box`; `decode_value` runs the registry.
- `src/registry/` — decoding:
  - `mod.rs` — `BoxDecoder` trait, `Registry`, and `default_registry()` which
    wires every fourcc to its decoder.
  - `decoders.rs` — one `*Decoder` per box type.
  - `data.rs` — `StructuredData` enum + the typed `*Data` structs and their
    `summary()` one-liners. Also `FieldSpan`.
  - `codec_config.rs` — `esds`/AudioSpecificConfig, avcC/hvcC/... helpers.
- `src/samples.rs` — per-sample tables for progressive and fragmented files.
- `src/edit/` — non-destructive editor (`edit` feature): extent-tree model,
  size/chunk-offset fixup, field patching, tag writing.
- `src/drm.rs` — pssh/Widevine/PlayReady payload decoding.
- `src/bin/` — `mp4dump`, `mp4info`, `mp4samples`, `mp4edit` (`cli` feature).
- `tests/` — integration tests; `examples/` — runnable examples.

## Key invariants (easy to get wrong)

- **FullBox version/flags are stripped by the parser.** A decoder's `payload`
  reader starts *after* the 1-byte version + 3-byte flags; the values arrive as
  the `version`/`flags` params. Don't re-read them in a decoder.
- **`BoxHeader.header_size` excludes version/flags** — it's just size+type
  (+largesize/+uuid). So a full box's payload length is
  `size - header_size - 4`.
- **`Box.payload_offset`/`payload_size`** describe the region a decoder reads
  (after version/flags for full boxes); containers report `None`.
- **`field_spans` are payload-relative.** `start = 0` is the first payload
  byte. Consumers add `payload_offset` to map into box/file coordinates.
- **Decoding is opt-in via `decode`.** With `decode=false`, `decoded`,
  `structured_data`, and `field_spans` are all `None` and no payload is read —
  keep it that way (structure-only parsing must stay cheap).

## Adding or extending a decoder

1. Add/extend the typed struct + `StructuredData` variant in
   `registry/data.rs`, and its `summary()` arm.
2. Implement `BoxDecoder::decode` in `registry/decoders.rs`.
3. Register it in `default_registry()` in `registry/mod.rs`.
4. Add a synthetic-fixture test in the matching `tests/*.rs` file.

### `field_spans` (hex-field highlighting)

`BoxDecoder::field_spans(version, flags, payload_len) -> Vec<FieldSpan>` reports
each payload field's byte range for hex-view highlighting. It has a default of
empty (opt-in) and is computed from metadata **without reading the payload**, so
it's deterministic and cheap. Guidelines:

- Offsets are payload-relative and must mirror `decode()`'s exact layout,
  including version-dependent widths and flag-gated fields.
- Use the `span(name, len, &mut pos)` helper in `decoders.rs` to lay fields out
  in order.
- Name spans to match the `StructuredData` field names — UIs cross-highlight
  detail rows against spans by name.
- For variable-length or repeating layouts (sample-table bodies, arrays,
  descriptor-based boxes like `esds`, or variable-first layouts like `emsg` v0),
  span only the fixed header and stop. Don't try to enumerate array entries —
  their offsets need the payload, which `field_spans` deliberately can't read.

## Testing conventions

Integration tests build **synthetic fixtures** so they're self-contained.
`tests/decoder_regressions.rs` has the shared helpers: `plain_box`, `full_box`
(prepends version + 24-bit flags), `parse` (decode=true), `find`, and `span`.
Prefer that style — small hand-built boxes with asserted offsets — over binary
fixtures.

A few tests need **real media files** (e.g. `real_fragmented_file_ground_truth`
in `tests/fragmented.rs`). These read from `$MP4_FIXTURES_DIR` and skip when it's
unset, so plain `cargo test` still works offline. To run them, fetch a *pinned*
release of [`video-commander/mp4-fixtures`][fx] and point the env var at it:

```bash
scripts/fetch-fixtures.sh                    # downloads + verifies the pinned tag
export MP4_FIXTURES_DIR="$PWD/target/fixtures"
cargo test
```

The pin lives in `scripts/fetch-fixtures.sh` (`FIXTURES_TAG`, currently
`fixtures-v2`). **Never point these tests at a local working copy of the
fixtures repo** — it drifts on every regeneration and the ground-truth constants
go stale. When you bump `FIXTURES_TAG`, the fixture bytes change, so re-derive
the asserted counts/offsets (ffprobe + the parser must agree) in the same commit.

[fx]: https://github.com/video-commander/mp4-fixtures

## Downstream

This crate is consumed by **video-commander** (`~/Source/video-commander`) via
crates.io. To ship a change there, publish a new version and bump its
dependency. video-commander can also patch in this local checkout for
development (`[patch.crates-io]` in its `src-tauri/Cargo.toml`) — remember to
publish and remove that patch before merging downstream.
