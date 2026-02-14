//! VAI container format serialization and deserialization

use crate::{Asset, Error, Result, TimelineEntry};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Magic bytes for VAI format: "VAI\0"
const MAGIC: [u8; 4] = [b'V', b'A', b'I', 0];

/// Current VAI format version
const VERSION: u16 = 1;

/// VAI file header
#[derive(Debug, Clone)]
pub struct VaiHeader {
    /// Format version
    pub version: u16,
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
    /// Frame rate numerator
    pub fps_num: u32,
    /// Frame rate denominator
    pub fps_den: u32,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Number of assets
    pub num_assets: u32,
    /// Number of timeline entries
    pub num_timeline_entries: u32,
}

impl VaiHeader {
    /// Creates a new VAI header
    pub fn new(
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        duration_ms: u64,
        num_assets: u32,
        num_timeline_entries: u32,
    ) -> Self {
        Self {
            version: VERSION,
            width,
            height,
            fps_num,
            fps_den,
            duration_ms,
            num_assets,
            num_timeline_entries,
        }
    }

    /// Reads a header from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read and validate magic bytes
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(Error::InvalidMagic);
        }

        // Read version
        let version = reader.read_u16::<LittleEndian>()?;
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }

        // Read remaining header fields
        let width = reader.read_u32::<LittleEndian>()?;
        let height = reader.read_u32::<LittleEndian>()?;
        let fps_num = reader.read_u32::<LittleEndian>()?;
        let fps_den = reader.read_u32::<LittleEndian>()?;
        let duration_ms = reader.read_u64::<LittleEndian>()?;
        let num_assets = reader.read_u32::<LittleEndian>()?;
        let num_timeline_entries = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            version,
            width,
            height,
            fps_num,
            fps_den,
            duration_ms,
            num_assets,
            num_timeline_entries,
        })
    }

    /// Writes the header to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_all(&MAGIC)?;
        writer.write_u16::<LittleEndian>(self.version)?;
        writer.write_u32::<LittleEndian>(self.width)?;
        writer.write_u32::<LittleEndian>(self.height)?;
        writer.write_u32::<LittleEndian>(self.fps_num)?;
        writer.write_u32::<LittleEndian>(self.fps_den)?;
        writer.write_u64::<LittleEndian>(self.duration_ms)?;
        writer.write_u32::<LittleEndian>(self.num_assets)?;
        writer.write_u32::<LittleEndian>(self.num_timeline_entries)?;
        Ok(())
    }
}

/// Complete VAI container
#[derive(Debug, Clone)]
pub struct VaiContainer {
    /// Container header
    pub header: VaiHeader,
    /// List of assets
    pub assets: Vec<Asset>,
    /// Timeline entries
    pub timeline: Vec<TimelineEntry>,
}

impl VaiContainer {
    /// Creates a new VAI container
    pub fn new(header: VaiHeader, assets: Vec<Asset>, timeline: Vec<TimelineEntry>) -> Self {
        Self {
            header,
            assets,
            timeline,
        }
    }

    /// Reads a VAI container from a reader
    pub fn read<R: Read>(mut reader: R) -> Result<Self> {
        // Read header
        let header = VaiHeader::read(&mut reader)?;

        // Read assets
        let mut assets = Vec::with_capacity(header.num_assets as usize);
        for _ in 0..header.num_assets {
            let id = reader.read_u32::<LittleEndian>()?;
            let width = reader.read_u32::<LittleEndian>()?;
            let height = reader.read_u32::<LittleEndian>()?;
            let data_len = reader.read_u32::<LittleEndian>()?;

            let mut data = vec![0u8; data_len as usize];
            reader.read_exact(&mut data)?;

            assets.push(Asset::new(id, width, height, data));
        }

        // Read timeline entries
        let mut timeline = Vec::with_capacity(header.num_timeline_entries as usize);
        for _ in 0..header.num_timeline_entries {
            let asset_id = reader.read_u32::<LittleEndian>()?;
            let start_time_ms = reader.read_u64::<LittleEndian>()?;
            let end_time_ms = reader.read_u64::<LittleEndian>()?;
            let position_x = reader.read_i32::<LittleEndian>()?;
            let position_y = reader.read_i32::<LittleEndian>()?;
            let z_order = reader.read_i32::<LittleEndian>()?;

            timeline.push(TimelineEntry::new(
                asset_id,
                start_time_ms,
                end_time_ms,
                position_x,
                position_y,
                z_order,
            ));
        }

        Ok(Self::new(header, assets, timeline))
    }

    /// Writes the VAI container to a writer
    pub fn write<W: Write>(&self, mut writer: W) -> Result<()> {
        // Write header
        self.header.write(&mut writer)?;

        // Write assets
        for asset in &self.assets {
            writer.write_u32::<LittleEndian>(asset.id)?;
            writer.write_u32::<LittleEndian>(asset.width)?;
            writer.write_u32::<LittleEndian>(asset.height)?;
            writer.write_u32::<LittleEndian>(asset.data.len() as u32)?;
            writer.write_all(&asset.data)?;
        }

        // Write timeline entries
        for entry in &self.timeline {
            writer.write_u32::<LittleEndian>(entry.asset_id)?;
            writer.write_u64::<LittleEndian>(entry.start_time_ms)?;
            writer.write_u64::<LittleEndian>(entry.end_time_ms)?;
            writer.write_i32::<LittleEndian>(entry.position_x)?;
            writer.write_i32::<LittleEndian>(entry.position_y)?;
            writer.write_i32::<LittleEndian>(entry.z_order)?;
        }

        Ok(())
    }

    /// Gets an asset by ID
    pub fn get_asset(&self, id: u32) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Gets all timeline entries active at a given timestamp
    pub fn get_active_entries(&self, timestamp_ms: u64) -> Vec<&TimelineEntry> {
        let mut entries: Vec<&TimelineEntry> = self
            .timeline
            .iter()
            .filter(|e| e.is_active(timestamp_ms))
            .collect();

        // Sort by z_order (lower first)
        entries.sort_by_key(|e| e.z_order);
        entries
    }

    /// Calculates the frame rate as a float
    pub fn fps(&self) -> f64 {
        self.header.fps_num as f64 / self.header.fps_den as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_header_roundtrip() {
        let header = VaiHeader::new(1920, 1080, 30, 1, 5000, 10, 20);

        let mut buffer = Vec::new();
        header.write(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let read_header = VaiHeader::read(&mut cursor).unwrap();

        assert_eq!(header.version, read_header.version);
        assert_eq!(header.width, read_header.width);
        assert_eq!(header.height, read_header.height);
        assert_eq!(header.fps_num, read_header.fps_num);
        assert_eq!(header.fps_den, read_header.fps_den);
        assert_eq!(header.duration_ms, read_header.duration_ms);
        assert_eq!(header.num_assets, read_header.num_assets);
        assert_eq!(header.num_timeline_entries, read_header.num_timeline_entries);
    }

    #[test]
    fn test_container_roundtrip() {
        let header = VaiHeader::new(1920, 1080, 30, 1, 1000, 1, 1);
        let assets = vec![Asset::new(0, 100, 100, vec![1, 2, 3, 4])];
        let timeline = vec![TimelineEntry::new(0, 0, 1000, 0, 0, 0)];

        let container = VaiContainer::new(header, assets, timeline);

        let mut buffer = Vec::new();
        container.write(&mut buffer).unwrap();

        let read_container = VaiContainer::read(Cursor::new(buffer)).unwrap();

        assert_eq!(container.header.width, read_container.header.width);
        assert_eq!(container.assets.len(), read_container.assets.len());
        assert_eq!(container.timeline.len(), read_container.timeline.len());
    }
}
