use super::*;

/// Type of a performance entry (mark, measure, navigation, resource, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerformanceEntryType {
    /// User-created timestamp via `performance.mark()`.
    Mark,
    /// User-created duration via `performance.measure()`.
    Measure,
    /// Navigation timing (page load start).
    Navigation,
    /// Resource fetch timing (e.g., stylesheet, script, image).
    Resource,
    /// Paint timing (first-paint, first-contentful-paint).
    Paint,
    /// Layout timing (internal, tracks layout/paint operations).
    Layout,
}

impl fmt::Display for PerformanceEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mark => write!(f, "mark"),
            Self::Measure => write!(f, "measure"),
            Self::Navigation => write!(f, "navigation"),
            Self::Resource => write!(f, "resource"),
            Self::Paint => write!(f, "paint"),
            Self::Layout => write!(f, "layout"),
        }
    }
}

/// A single performance entry (mark, measure, or resource timing).
/// W3C Performance Timeline §3 PerformanceEntry interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceEntry {
    /// The type of entry (mark, measure, etc.).
    pub entry_type: PerformanceEntryType,
    /// The name of the entry (e.g., "myMark", "myMeasure").
    pub name: String,
    /// Start time relative to the navigation start (milliseconds, DOMHighResTimeStamp).
    pub start_time: f64,
    /// Duration of the entry (milliseconds). For marks, typically 0.
    pub duration: f64,
}

impl PerformanceEntry {
    /// Create a new performance entry.
    pub fn new(
        entry_type: PerformanceEntryType,
        name: String,
        start_time: f64,
        duration: f64,
    ) -> Self {
        Self {
            entry_type,
            name,
            start_time,
            duration,
        }
    }

    /// Get the end time of this entry (start_time + duration).
    pub fn end_time(&self) -> f64 {
        self.start_time + self.duration
    }
}

/// Collection of performance entries.
/// Tracks marks and measures across the document lifetime.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceEntries {
    /// All recorded performance entries.
    entries: Vec<PerformanceEntry>,
}

impl PerformanceEntries {
    /// Create a new empty performance entries collection.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a performance entry.
    pub fn add_entry(&mut self, entry: PerformanceEntry) {
        self.entries.push(entry);
    }

    /// Get all performance entries.
    pub fn all(&self) -> &[PerformanceEntry] {
        &self.entries
    }

    /// Get entries by type (mark, measure, etc.).
    pub fn get_by_type(&self, entry_type: PerformanceEntryType) -> Vec<&PerformanceEntry> {
        self.entries
            .iter()
            .filter(|e| e.entry_type == entry_type)
            .collect()
    }

    /// Get entries by name.
    pub fn get_by_name(&self, name: &str) -> Vec<&PerformanceEntry> {
        self.entries
            .iter()
            .filter(|e| e.name == name)
            .collect()
    }

    /// Get a single entry by name (returns the first match).
    pub fn get_first_by_name(&self, name: &str) -> Option<&PerformanceEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Clear all performance entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the count of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Placeholder for PerformanceObserver observer registration.
/// P3 will implement JS binding for observe/disconnect/callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceObserver {
    /// Unique handle for this observer (assigned by shell runtime).
    handle: Option<u32>,
    /// Entry types to observe (mark, measure, etc.).
    entry_types: Vec<PerformanceEntryType>,
}

impl PerformanceObserver {
    /// Create a new PerformanceObserver.
    pub fn new() -> Self {
        Self {
            handle: None,
            entry_types: Vec::new(),
        }
    }

    /// Add entry types to observe.
    pub fn observe(&mut self, entry_types: Vec<PerformanceEntryType>) {
        self.entry_types = entry_types;
    }

    /// Disconnect the observer.
    pub fn disconnect(&mut self) {
        self.handle = None;
        self.entry_types.clear();
    }

    /// Get the observed entry types.
    pub fn observed_types(&self) -> &[PerformanceEntryType] {
        &self.entry_types
    }

    /// Check if this observer is watching a specific entry type.
    pub fn is_observing(&self, entry_type: PerformanceEntryType) -> bool {
        self.entry_types.contains(&entry_type)
    }

    /// Set the observer handle (assigned by shell runtime when registered).
    pub fn set_handle(&mut self, handle: u32) {
        self.handle = Some(handle);
    }

    /// Get the observer handle.
    pub fn handle(&self) -> Option<u32> {
        self.handle
    }
}

impl Default for PerformanceObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    // ── Performance Timeline (W3C Performance Timeline §3) ────────────────────

    /// Set the timing origin (navigation start time in milliseconds since epoch).
    /// All subsequent performance timings are relative to this value.
    pub fn set_timing_origin(&mut self, timestamp_ms: f64) {
        self.timing_origin = timestamp_ms;
    }

    /// Get the current time relative to timing_origin (milliseconds).
    /// Returns time since navigation start.
    pub fn current_time(&self) -> f64 {
        // In Phase 1, we use a simple approach: caller must pass current time.
        // In Phase 2, this will integrate with shell's high-res timer.
        0.0
    }

    /// Record a performance mark at the current time.
    /// `timestamp_ms` is relative to timing_origin; if None, uses current_time().
    pub fn mark(&mut self, name: String, timestamp_ms: Option<f64>) {
        let start_time = timestamp_ms.unwrap_or_else(|| self.current_time());
        let entry = PerformanceEntry::new(PerformanceEntryType::Mark, name, start_time, 0.0);
        self.performance.add_entry(entry);
    }

    /// Record a performance measure between two marks.
    /// `start_mark` and `end_mark` are mark names. If not found, measure creation fails.
    /// Returns `Some(duration)` on success, `None` if marks not found.
    pub fn measure(&mut self, name: String, start_mark: &str, end_mark: &str) -> Option<f64> {
        let start_entry = self.performance.get_first_by_name(start_mark)?;
        let start_time = start_entry.start_time;

        let end_entry = self.performance.get_first_by_name(end_mark)?;
        let end_time = end_entry.start_time;

        let duration = (end_time - start_time).max(0.0);
        let entry = PerformanceEntry::new(PerformanceEntryType::Measure, name, start_time, duration);
        self.performance.add_entry(entry);
        Some(duration)
    }

    /// Get a reference to the performance entries collection.
    pub fn performance_entries(&self) -> &PerformanceEntries {
        &self.performance
    }

    /// Get a mutable reference to the performance entries collection.
    /// Used internally to add entries during rendering.
    pub fn performance_entries_mut(&mut self) -> &mut PerformanceEntries {
        &mut self.performance
    }

    /// Get all performance entries of a specific type.
    pub fn performance_entries_by_type(
        &self,
        entry_type: PerformanceEntryType,
    ) -> Vec<&PerformanceEntry> {
        self.performance.get_by_type(entry_type)
    }

    /// Get all performance entries with a specific name.
    pub fn performance_entries_by_name(&self, name: &str) -> Vec<&PerformanceEntry> {
        self.performance.get_by_name(name)
    }

    /// Clear all performance entries.
    pub fn clear_performance_entries(&mut self) {
        self.performance.clear();
    }
}
