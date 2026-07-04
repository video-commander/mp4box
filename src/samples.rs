use anyhow::Context;
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SampleInfo {
    /// 0-based sample index
    pub index: u32,

    /// Decode time (DTS) in track timescale units
    pub dts: u64,

    /// Presentation time (PTS) in track timescale units (DTS + composition offset)
    pub pts: u64,

    /// Start time in seconds (pts / timescale as f64)
    pub start_time: f64,

    /// Duration in track timescale units (from stts)
    pub duration: u32,

    /// Composition/rendered offset in track timescale units (from ctts, may be 0)
    pub rendered_offset: i64,

    /// Byte offset in the file (from stsc + stco/co64)
    pub file_offset: u64,

    /// Sample size in bytes (from stsz)
    pub size: u32,

    /// Whether this sample is a sync sample / keyframe (from stss)
    pub is_sync: bool,
}

/// Complete sample information and metadata for a single track in an MP4 file.
///
/// This structure represents all the sample-level information extracted from an MP4 track,
/// combining metadata from the track header and media information with detailed sample
/// data parsed from the sample table boxes (stbl). It provides a complete view of a
/// track's temporal structure, timing information, and individual sample properties.
///
/// The struct is designed for media analysis, debugging, and applications that need
/// detailed insight into MP4 file structure and sample organization.
///
/// # Fields
///
/// * `track_id` - Unique identifier for this track within the MP4 file (from tkhd box).
///   Track IDs are typically sequential starting from 1, but can have gaps.
///
/// * `handler_type` - Four-character code indicating the media type (from hdlr box):
///   - `"vide"` - Video track
///   - `"soun"` - Audio track
///   - `"hint"` - Hint track
///   - `"meta"` - Metadata track
///   - `"subt"` - Subtitle track
///   - And other standardized or custom handler types
///
/// * `timescale` - Time coordinate system for this track (from mdhd box).
///   Defines the number of time units per second. For example:
///   - Video tracks often use 90000 (90kHz) or frame rate multiples
///   - Audio tracks commonly use the sample rate (e.g., 48000 for 48kHz)
///
/// * `duration` - Total track duration in track timescale units (from mdhd box).
///   To get duration in seconds: `duration as f64 / timescale as f64`
///
/// * `sample_count` - Total number of samples/frames in this track.
///   Should equal `samples.len()` when all samples are successfully parsed.
///
/// * `samples` - Detailed information for each individual sample in the track.
///   Ordered chronologically by decode time (DTS). Each `SampleInfo` contains
///   timing, size, sync status, and file offset information.
///
/// # Example
///
/// ```rust,no_run
/// use mp4box::track_samples_from_path;
///
/// let track_samples = track_samples_from_path("video.mp4").unwrap();
///
/// for track in track_samples {
///     println!("Track {}: {} ({} samples)",
///              track.track_id,
///              track.handler_type,
///              track.sample_count);
///
///     let duration_sec = track.duration as f64 / track.timescale as f64;
///     println!("Duration: {:.2} seconds", duration_sec);
///
///     if track.handler_type == "vide" {
///         let keyframes = track.samples.iter()
///             .filter(|s| s.is_sync)
///             .count();
///         println!("Keyframes: {}", keyframes);
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct TrackSamples {
    pub track_id: u32,
    pub handler_type: String, // "vide", "soun", etc.
    pub timescale: u32,
    pub duration: u64, // in track timescale units
    pub sample_count: u32,
    pub samples: Vec<SampleInfo>,
}

/// Extracts sample information from all tracks in an MP4 file using a generic reader.
///
/// This function reads an MP4 file from any source that implements `Read + Seek` (such as
/// a file, buffer, or network stream) and extracts detailed sample information from all
/// video and audio tracks found in the file.
///
/// # Parameters
///
/// * `reader` - A mutable reference to any type implementing `Read + Seek` traits.
///   The reader will be used to parse the MP4 box structure and extract sample data.
///
/// # Returns
///
/// Returns `Ok(Vec<TrackSamples>)` containing sample information for each track found,
/// or an `Err` if the file cannot be parsed or is not a valid MP4 file.
///
/// Each `TrackSamples` contains:
/// - Track metadata (ID, handler type, timescale, duration)
/// - Individual sample information (timing, size, sync status, file offsets)
///
/// # Errors
///
/// This function may return an error in the following cases:
/// - I/O errors when reading from the source
/// - Invalid or corrupted MP4 file structure
/// - Missing required MP4 boxes (moov, trak, etc.)
/// - Memory allocation failures for large files
///
/// # Example
///
/// ```rust,no_run
/// use std::fs::File;
/// use mp4box::track_samples_from_reader;
///
/// let file = File::open("video.mp4").unwrap();
/// let track_samples = track_samples_from_reader(file).unwrap();
///
/// for track in track_samples {
///     println!("Track {}: {} samples", track.track_id, track.sample_count);
/// }
/// ```
pub fn track_samples_from_reader<R: Read + Seek>(
    mut reader: R,
) -> anyhow::Result<Vec<TrackSamples>> {
    let file_size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;

    let boxes = crate::get_boxes(&mut reader, file_size, /*decode=*/ true)
        .context("getting boxes from reader")?;

    let mut result = Vec::new();

    for moov_box in boxes.iter().filter(|b| b.typ == "moov") {
        if let Some(children) = &moov_box.children {
            for trak_box in children.iter().filter(|b| b.typ == "trak") {
                if let Some(track_samples) =
                    crate::samples::extract_track_samples(trak_box, &mut reader)?
                {
                    result.push(track_samples);
                }
            }
        }
    }

    Ok(result)
}

/// Extracts sample information from all tracks in an MP4 file specified by file path.
///
/// This is a convenience function that opens a file from the filesystem and delegates
/// to `track_samples_from_reader()` to perform the actual parsing. It's the most common
/// way to extract sample information when working with MP4 files on disk.
///
/// # Parameters
///
/// * `path` - A path-like type (anything implementing `AsRef<Path>`) pointing to the
///   MP4 file to analyze. This includes `String`, `&str`, `PathBuf`, and `&Path`.
///
/// # Returns
///
/// Returns `Ok(Vec<TrackSamples>)` containing sample information for each track found,
/// or an `Err` if the file cannot be opened, read, or parsed.
///
/// # Errors
///
/// This function may return an error in the following cases:
/// - File not found or insufficient permissions to read the file
/// - All errors that can occur in `track_samples_from_reader()`
/// - Invalid or corrupted MP4 file structure
/// - Missing required MP4 boxes (moov, trak, etc.)
///
/// # Example
///
/// ```rust,no_run
/// use mp4box::track_samples_from_path;
/// use std::path::Path;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Using string literal
///     let samples = track_samples_from_path("video.mp4")?;
///
///     // Using Path
///     let path = Path::new("/path/to/video.mp4");
///     let samples = track_samples_from_path(path)?;
///
///     for track in samples {
///         println!("Track {} has {} samples of type {}",
///                  track.track_id, track.sample_count, track.handler_type);
///     }
///     Ok(())
/// }
/// ```
pub fn track_samples_from_path(path: impl AsRef<Path>) -> anyhow::Result<Vec<TrackSamples>> {
    let file = File::open(path)?;
    track_samples_from_reader(file)
}

/// Extracts sample information from a single track box (trak) in an MP4 file.
///
/// This function processes a specific track box from an already-parsed MP4 file structure
/// and extracts all sample-related information from its sample table boxes (stbl).
/// It's a lower-level function typically used internally by `track_samples_from_reader()`.
///
/// The function navigates through the MP4 box hierarchy (trak → mdia → minf → stbl) to
/// locate and parse the various sample table boxes (stts, stsc, stsz, stco, etc.) that
/// contain the sample metadata.
///
/// # Parameters
///
/// * `trak_box` - A reference to a parsed track box (`trak`) from an MP4 file. This box
///   should contain the complete track structure including media information and sample tables.
/// * `reader` - A mutable reference to the file reader, used for seeking to specific
///   byte offsets when calculating sample file positions.
///
/// # Returns
///
/// Returns:
/// - `Ok(Some(TrackSamples))` - Successfully extracted sample information from the track
/// - `Ok(None)` - Track box is valid but contains no usable sample information
/// - `Err(anyhow::Error)` - Failed to parse the track due to structural issues
///
/// The returned `TrackSamples` contains:
/// - Track metadata (ID, media handler type, timescale, duration)
/// - Complete sample information (timing, sizes, sync points, file offsets)
///
/// # Errors
///
/// This function may return an error in the following cases:
/// - Missing required child boxes (mdia, minf, stbl)
/// - Corrupted or invalid sample table data
/// - Inconsistent sample counts between different sample tables
/// - I/O errors when calculating file offsets
/// - Memory allocation failures for tracks with many samples
///
/// # Example
///
/// ```rust,no_run
/// use mp4box::get_boxes;
/// use mp4box::samples::extract_track_samples;
/// use std::fs::File;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut file = File::open("video.mp4")?;
///     let file_size = file.metadata()?.len();
///     let boxes = get_boxes(&mut file, file_size, true)?;
///
///     // Find moov box and extract samples from each track
///     for moov_box in boxes.iter().filter(|b| b.typ == "moov") {
///         if let Some(children) = &moov_box.children {
///             for trak_box in children.iter().filter(|b| b.typ == "trak") {
///                 if let Some(samples) = extract_track_samples(trak_box, &mut file)? {
///                     println!("Found track with {} samples", samples.sample_count);
///                 }
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
pub fn extract_track_samples<R: Read + Seek>(
    trak_box: &crate::Box,
    reader: &mut R,
) -> anyhow::Result<Option<TrackSamples>> {
    // use crate::{BoxValue, StructuredData}; // Will be used when we implement proper parsing

    // Find track ID from tkhd
    let track_id = find_track_id(trak_box)?;

    // Find handler type from mdhd
    let (handler_type, timescale, duration) = find_media_info(trak_box)?;

    // Find sample table (stbl) box
    let stbl_box = find_stbl_box(trak_box)?;

    // Extract sample table data
    let sample_tables = extract_sample_tables(stbl_box)?;

    // Build sample information from the tables
    let _ = reader;
    let samples = build_sample_info(&sample_tables, timescale)?;
    let sample_count = samples.len() as u32;

    Ok(Some(TrackSamples {
        track_id,
        handler_type,
        timescale,
        duration,
        sample_count,
        samples,
    }))
}

fn find_track_id(trak_box: &crate::Box) -> anyhow::Result<u32> {
    use crate::registry::StructuredData;

    // Look for tkhd box to get track ID
    if let Some(children) = &trak_box.children {
        for child in children {
            if child.typ == "tkhd" {
                // Extract track ID from structured data
                if let Some(StructuredData::TrackHeader(tkhd_data)) = &child.structured_data {
                    return Ok(tkhd_data.track_id);
                }
            }
        }
    }
    anyhow::bail!("No tkhd box found or track ID could not be parsed")
}

fn find_media_info(trak_box: &crate::Box) -> anyhow::Result<(String, u32, u64)> {
    use crate::registry::StructuredData;

    // Look for mdia/mdhd and mdia/hdlr boxes
    if let Some(children) = &trak_box.children {
        for child in children {
            if child.typ == "mdia"
                && let Some(mdia_children) = &child.children
            {
                let mut timescale = 1000; // Default
                let mut duration = 0; // Default
                let mut handler_type = String::from("vide"); // Default

                for mdia_child in mdia_children {
                    if mdia_child.typ == "mdhd" {
                        // Parse timescale and duration from mdhd
                        if let Some(StructuredData::MediaHeader(mdhd_data)) =
                            &mdia_child.structured_data
                        {
                            timescale = mdhd_data.timescale;
                            duration = mdhd_data.duration;
                        }
                    }
                    if mdia_child.typ == "hdlr" {
                        // Parse handler type from hdlr
                        if let Some(StructuredData::HandlerReference(hdlr_data)) =
                            &mdia_child.structured_data
                        {
                            handler_type = hdlr_data.handler_type.clone();
                        }
                    }
                }

                return Ok((handler_type, timescale, duration));
            }
        }
    }
    Ok((String::from("vide"), 1000, 0))
}

fn find_stbl_box(trak_box: &crate::Box) -> anyhow::Result<&crate::Box> {
    // Navigate to mdia/minf/stbl
    if let Some(children) = &trak_box.children {
        for child in children {
            if child.typ == "mdia"
                && let Some(mdia_children) = &child.children
            {
                for mdia_child in mdia_children {
                    if mdia_child.typ == "minf"
                        && let Some(minf_children) = &mdia_child.children
                    {
                        for minf_child in minf_children {
                            if minf_child.typ == "stbl" {
                                return Ok(minf_child);
                            }
                        }
                    }
                }
            }
        }
    }
    anyhow::bail!("stbl box not found")
}

#[derive(Debug)]
struct SampleTables {
    stsd: Option<crate::registry::StsdData>,
    stts: Option<crate::registry::SttsData>,
    ctts: Option<crate::registry::CttsData>,
    stsc: Option<crate::registry::StscData>,
    stsz: Option<crate::registry::StszData>,
    stss: Option<crate::registry::StssData>,
    stco: Option<crate::registry::StcoData>,
    co64: Option<crate::registry::Co64Data>,
}

fn extract_sample_tables(stbl_box: &crate::Box) -> anyhow::Result<SampleTables> {
    let mut tables = SampleTables {
        stsd: None,
        stts: None,
        ctts: None,
        stsc: None,
        stsz: None,
        stss: None,
        stco: None,
        co64: None,
    };

    // Extract structured data directly from child boxes
    if let Some(children) = &stbl_box.children {
        for child in children {
            if let Some(structured_data) = &child.structured_data {
                match structured_data {
                    crate::registry::StructuredData::SampleDescription(data) => {
                        tables.stsd = Some(data.clone());
                    }
                    crate::registry::StructuredData::DecodingTimeToSample(data) => {
                        tables.stts = Some(data.clone());
                    }
                    crate::registry::StructuredData::CompositionTimeToSample(data) => {
                        tables.ctts = Some(data.clone());
                    }
                    crate::registry::StructuredData::SampleToChunk(data) => {
                        tables.stsc = Some(data.clone());
                    }
                    crate::registry::StructuredData::SampleSize(data) => {
                        tables.stsz = Some(data.clone());
                    }
                    crate::registry::StructuredData::SyncSample(data) => {
                        tables.stss = Some(data.clone());
                    }
                    crate::registry::StructuredData::ChunkOffset(data) => {
                        tables.stco = Some(data.clone());
                    }
                    crate::registry::StructuredData::ChunkOffset64(data) => {
                        tables.co64 = Some(data.clone());
                    }
                    // MediaHeader, HandlerReference, and TrackHeader are not sample table data, ignore them
                    crate::registry::StructuredData::MediaHeader(_) => {}
                    crate::registry::StructuredData::HandlerReference(_) => {}
                    crate::registry::StructuredData::TrackHeader(_) => {}
                    crate::registry::StructuredData::TrackFragmentRun(_) => {}
                }
            }
        }
    }

    Ok(tables)
}

/// Sequential cursor over run-length encoded table entries (stts/ctts).
/// Yields one value per sample in a single forward pass.
struct RunCursor<'a, T> {
    entries: &'a [T],
    idx: usize,
    used: u32,
}

impl<'a, T> RunCursor<'a, T> {
    fn new(entries: &'a [T]) -> Self {
        Self {
            entries,
            idx: 0,
            used: 0,
        }
    }

    /// Advance one sample; returns the entry covering it, or `None` once the
    /// table is exhausted.
    fn next(&mut self, count_of: impl Fn(&T) -> u32) -> Option<&'a T> {
        while let Some(e) = self.entries.get(self.idx) {
            if self.used < count_of(e) {
                self.used += 1;
                return Some(e);
            }
            self.idx += 1;
            self.used = 0;
        }
        None
    }
}

fn build_sample_info(tables: &SampleTables, timescale: u32) -> anyhow::Result<Vec<SampleInfo>> {
    // Sample count and sizes come from stsz (or stz2, which decodes to the
    // same shape).
    let Some(stsz) = &tables.stsz else {
        return Ok(Vec::new());
    };
    let sample_count = stsz.sample_count;

    let size_of = |i: u32| -> u32 {
        if stsz.sample_size > 0 {
            stsz.sample_size
        } else {
            stsz.sample_sizes.get(i as usize).copied().unwrap_or(0)
        }
    };

    // ---- File offsets: walk chunks once via stsc + stco/co64 ----
    let chunk_offset_at = |index: usize| -> Option<u64> {
        if let Some(co64) = &tables.co64 {
            co64.chunk_offsets.get(index).copied()
        } else if let Some(stco) = &tables.stco {
            stco.chunk_offsets.get(index).copied().map(u64::from)
        } else {
            None
        }
    };
    let chunk_count = if let Some(co64) = &tables.co64 {
        co64.chunk_offsets.len()
    } else if let Some(stco) = &tables.stco {
        stco.chunk_offsets.len()
    } else {
        0
    };

    // Samples not covered by the tables keep offset 0 (unknown).
    let mut file_offsets = vec![0u64; sample_count as usize];
    if let Some(stsc) = &tables.stsc {
        let mut sample_idx = 0u32;
        'chunks: for (i, entry) in stsc.entries.iter().enumerate() {
            // Chunk range covered by this stsc entry (1-based first_chunk).
            let first = (entry.first_chunk.max(1) - 1) as usize;
            let next_first = stsc
                .entries
                .get(i + 1)
                .map(|e| (e.first_chunk.max(1) - 1) as usize)
                .unwrap_or(chunk_count);

            for chunk in first..next_first.min(chunk_count) {
                let Some(base) = chunk_offset_at(chunk) else {
                    break 'chunks;
                };
                let mut offset = base;
                for _ in 0..entry.samples_per_chunk {
                    if sample_idx >= sample_count {
                        break 'chunks;
                    }
                    file_offsets[sample_idx as usize] = offset;
                    offset += size_of(sample_idx) as u64;
                    sample_idx += 1;
                }
            }
        }
    }

    // ---- Timing and sync tables, one forward pass each ----
    let empty_stts = Vec::new();
    let mut stts_cursor = RunCursor::new(
        tables
            .stts
            .as_ref()
            .map(|t| t.entries.as_slice())
            .unwrap_or(&empty_stts),
    );
    let empty_ctts = Vec::new();
    let mut ctts_cursor = RunCursor::new(
        tables
            .ctts
            .as_ref()
            .map(|t| t.entries.as_slice())
            .unwrap_or(&empty_ctts),
    );
    let stss_set: Option<std::collections::HashSet<u32>> = tables
        .stss
        .as_ref()
        .map(|s| s.sample_numbers.iter().copied().collect());

    // If stts runs out before sample_count (non-compliant but seen in the
    // wild), keep using its last delta; a missing/empty table yields 0
    // rather than a fabricated frame rate.
    let last_stts_delta = tables
        .stts
        .as_ref()
        .and_then(|t| t.entries.last())
        .map(|e| e.sample_delta)
        .unwrap_or(0);

    let mut samples = Vec::with_capacity(sample_count as usize);
    let mut current_dts = 0u64;

    for i in 0..sample_count {
        let duration = stts_cursor
            .next(|e| e.sample_count)
            .map(|e| e.sample_delta)
            .unwrap_or(last_stts_delta);

        // A ctts that doesn't cover this sample means no composition offset.
        let composition_offset = ctts_cursor
            .next(|e| e.sample_count)
            .map(|e| e.sample_offset)
            .unwrap_or(0);

        let pts = current_dts.saturating_add_signed(composition_offset as i64);

        samples.push(SampleInfo {
            index: i,
            dts: current_dts,
            pts,
            start_time: if timescale > 0 {
                pts as f64 / timescale as f64
            } else {
                0.0
            },
            duration,
            rendered_offset: composition_offset as i64,
            file_offset: file_offsets[i as usize],
            size: size_of(i),
            // stss uses 1-based sample numbers; no stss box = all sync.
            is_sync: stss_set.as_ref().is_none_or(|set| set.contains(&(i + 1))),
        });

        current_dts += duration as u64;
    }

    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{StructuredData, TkhdData};

    #[test]
    fn test_find_track_id_from_structured_data() {
        // Create a mock tkhd box with structured data
        let tkhd_data = TkhdData {
            version: 0,
            flags: 0,
            track_id: 42,
            duration: 48000,
            width: 1920.0,
            height: 1080.0,
        };

        let tkhd_box = crate::Box {
            offset: 0,
            size: 0,
            header_size: 0,
            payload_offset: None,
            payload_size: None,
            typ: "tkhd".to_string(),
            uuid: None,
            version: Some(0),
            flags: Some(0),
            kind: "full".to_string(),
            full_name: "Track Header Box".to_string(),
            decoded: None,
            structured_data: Some(StructuredData::TrackHeader(tkhd_data)),
            children: None,
        };

        let trak_box = crate::Box {
            offset: 0,
            size: 0,
            header_size: 0,
            payload_offset: None,
            payload_size: None,
            typ: "trak".to_string(),
            uuid: None,
            version: None,
            flags: None,
            kind: "container".to_string(),
            full_name: "Track Box".to_string(),
            decoded: None,
            structured_data: None,
            children: Some(vec![tkhd_box]),
        };

        // Test that we can extract the correct track ID
        let track_id = find_track_id(&trak_box).unwrap();
        assert_eq!(track_id, 42);
    }

    #[test]
    fn test_find_track_id_multiple_tracks() {
        // Test with different track IDs to ensure each gets the right one
        for expected_id in [1, 3, 7, 255] {
            let tkhd_data = TkhdData {
                version: 0,
                flags: 0,
                track_id: expected_id,
                duration: 24000,
                width: 0.0,
                height: 0.0,
            };

            let tkhd_box = crate::Box {
                offset: 0,
                size: 0,
                header_size: 0,
                payload_offset: None,
                payload_size: None,
                typ: "tkhd".to_string(),
                uuid: None,
                version: Some(0),
                flags: Some(0),
                kind: "full".to_string(),
                full_name: "Track Header Box".to_string(),
                decoded: None,
                structured_data: Some(StructuredData::TrackHeader(tkhd_data)),
                children: None,
            };

            let trak_box = crate::Box {
                offset: 0,
                size: 0,
                header_size: 0,
                payload_offset: None,
                payload_size: None,
                typ: "trak".to_string(),
                uuid: None,
                version: None,
                flags: None,
                kind: "container".to_string(),
                full_name: "Track Box".to_string(),
                decoded: None,
                structured_data: None,
                children: Some(vec![tkhd_box]),
            };

            let track_id = find_track_id(&trak_box).unwrap();
            assert_eq!(track_id, expected_id);
        }
    }

    #[test]
    fn test_find_track_id_no_tkhd_box() {
        // Test error case when no tkhd box is present
        let trak_box = crate::Box {
            offset: 0,
            size: 0,
            header_size: 0,
            payload_offset: None,
            payload_size: None,
            typ: "trak".to_string(),
            uuid: None,
            version: None,
            flags: None,
            kind: "container".to_string(),
            full_name: "Track Box".to_string(),
            decoded: None,
            structured_data: None,
            children: Some(vec![]),
        };

        let result = find_track_id(&trak_box);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No tkhd box found")
        );
    }
}
