//! Progress tracking with ETA estimation

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Thread-safe progress tracker with ETA estimation
pub struct ProgressTracker {
    total: u64,
    processed: Arc<AtomicU64>,
    start_time: Instant,
    label: String,
}

impl ProgressTracker {
    /// Creates a new progress tracker
    pub fn new(total: u64, label: &str) -> Self {
        Self {
            total,
            processed: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            label: label.to_string(),
        }
    }

    /// Returns an atomic counter that can be shared across threads
    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.processed)
    }

    /// Increments the processed count by one and optionally prints progress
    pub fn increment_and_report(&self, report_interval: u64) {
        let current = self.processed.fetch_add(1, Ordering::Relaxed) + 1;
        if current % report_interval == 0 || current == self.total {
            self.print_progress(current);
        }
    }

    /// Prints current progress with ETA
    fn print_progress(&self, current: u64) {
        let elapsed = self.start_time.elapsed();
        let elapsed_secs = elapsed.as_secs_f64();

        let percent = if self.total > 0 {
            (current as f64 / self.total as f64) * 100.0
        } else {
            0.0
        };

        if current > 0 && current < self.total {
            let rate = current as f64 / elapsed_secs;
            let remaining = (self.total - current) as f64 / rate;
            let eta = format_duration(remaining);
            println!(
                "  {} {}/{} ({:.1}%) - elapsed: {} - ETA: {}",
                self.label,
                current,
                self.total,
                percent,
                format_duration(elapsed_secs),
                eta,
            );
        } else if current == self.total {
            println!(
                "  {} {}/{} (100.0%) - completed in {}",
                self.label,
                current,
                self.total,
                format_duration(elapsed_secs),
            );
        }
    }
}

/// Formats seconds into a human-readable duration string
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        let mins = (secs / 60.0).floor() as u64;
        let remaining = secs - (mins as f64 * 60.0);
        format!("{}m {:.0}s", mins, remaining)
    } else {
        let hours = (secs / 3600.0).floor() as u64;
        let remaining = secs - (hours as f64 * 3600.0);
        let mins = (remaining / 60.0).floor() as u64;
        let remaining_secs = remaining - (mins as f64 * 60.0);
        format!("{}h {}m {:.0}s", hours, mins, remaining_secs)
    }
}