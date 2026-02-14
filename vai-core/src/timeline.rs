//! Timeline data structures for VAI format

/// Represents a single timeline entry that describes when and where an asset appears
#[derive(Debug, Clone, Copy)]
pub struct TimelineEntry {
    /// Asset ID to display
    pub asset_id: u32,
    /// Start time in milliseconds
    pub start_time_ms: u64,
    /// End time in milliseconds
    pub end_time_ms: u64,
    /// X position relative to the frame (can be negative for partially off-screen)
    pub position_x: i32,
    /// Y position relative to the frame
    pub position_y: i32,
    /// Layering order (lower = further back; background = 0)
    pub z_order: i32,
}

impl TimelineEntry {
    /// Creates a new timeline entry
    pub fn new(
        asset_id: u32,
        start_time_ms: u64,
        end_time_ms: u64,
        position_x: i32,
        position_y: i32,
        z_order: i32,
    ) -> Self {
        Self {
            asset_id,
            start_time_ms,
            end_time_ms,
            position_x,
            position_y,
            z_order,
        }
    }

    /// Checks if this entry is active at the given timestamp
    pub fn is_active(&self, timestamp_ms: u64) -> bool {
        timestamp_ms >= self.start_time_ms && timestamp_ms < self.end_time_ms
    }

    /// Returns the duration of this entry in milliseconds
    pub fn duration_ms(&self) -> u64 {
        self.end_time_ms.saturating_sub(self.start_time_ms)
    }
}
