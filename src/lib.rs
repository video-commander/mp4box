//! # mp4box
//!
//! A minimal, dependency-light MP4/ISOBMFF parser for Rust.
//!
//! This crate parses the MP4 box tree (including 64-bit "large" boxes and
//! UUID boxes), classifies known box types, and provides optional decoding
//! of common box types through a pluggable registry system.
//!
//! ## Features
//! - Zero-dependency core parser that works with any `Read + Seek` source
//! - Support for large (64-bit) boxes and UUID boxes  
//! - Comprehensive known-box registry with human-readable names
//! - JSON-serializable output perfect for web UIs and APIs
//! - Optional structured decoding with pluggable decoders
//! - Command-line tools for MP4 inspection and debugging
//!
//! ## Use Cases  
//! - CLIs for inspecting MP4 structure (e.g. `mp4dump`)
//! - Tauri/Electron desktop apps that need JSON output for UI
//! - Backend services that need to inspect or validate MP4 files
//! - Media processing tools and debugging utilities
//!
//! # Quick start
//!
//! ```no_run
//! use mp4box::get_boxes;
//! use std::fs::File;
//!
//! fn main() -> anyhow::Result<()> {
//!     let mut file = File::open("video.mp4")?;
//!     let size = file.metadata()?.len();
//!     
//!     // Parse structure only
//!     let boxes = get_boxes(&mut file, size, false)?;
//!     println!("Found {} top-level boxes", boxes.len());
//!     
//!     // Parse with decoding of known box types
//!     let mut file = File::open("video.mp4")?;
//!     let decoded_boxes = get_boxes(&mut file, size, true)?;
//!     
//!     // Print file type info
//!     if let Some(ftyp) = decoded_boxes.iter().find(|b| b.typ == "ftyp") {
//!         println!("File type: {}",
//!             ftyp.decoded.as_deref().unwrap_or("unknown"));
//!     }
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Lower-level parsing
//!
//! For more control, you can use the lower-level parser functions:
//!
//! ```no_run
//! use mp4box::parser::{read_box_header, parse_children};
//! use mp4box::known_boxes::KnownBox;
//! use std::fs::File;
//! use std::io::{Seek, SeekFrom};
//!
//! fn main() -> anyhow::Result<()> {
//!     let mut file = File::open("video.mp4")?;
//!     let file_len = file.metadata()?.len();
//!     
//!     while file.stream_position()? < file_len {
//!         let header = read_box_header(&mut file)?;
//!         let known = KnownBox::from(header.typ);
//!         println!("Box: {} ({}) at offset {:#x}",
//!             header.typ, known.full_name(), header.start);
//!             
//!         let end = if header.size == 0 { file_len } else { header.start + header.size };
//!         file.seek(SeekFrom::Start(end))?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! For more examples, see the `mp4dump` and `mp4info` binaries in this repository.

pub mod api;
pub mod boxes;
pub mod known_boxes;
pub mod parser;
pub mod registry;
pub mod samples;
pub mod util;

pub use boxes::{BoxHeader, BoxKey, BoxRef, FourCC, NodeKind};
pub use parser::{parse_children, read_box_header};
pub use registry::{
    BoxValue, Co64Data, CttsData, CttsEntry, HdlrData, MdhdData, Registry, SampleEntry, StcoData,
    StructuredData, StscData, StscEntry, StsdData, StssData, StszData, SttsData, SttsEntry,
};

// High-level API
pub use api::{Box, HexDump, get_boxes, get_boxes_with_registry, hex_range};
pub use samples::{SampleInfo, TrackSamples, track_samples_from_path, track_samples_from_reader};
