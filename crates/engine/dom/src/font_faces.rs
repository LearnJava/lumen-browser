use super::*;

/// The status of a FontFace: whether it's been loaded, is loading, or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontFaceStatus {
    /// The font has not yet been loaded.
    Unloaded,
    /// The font is currently loading.
    Loading,
    /// The font has been successfully loaded.
    Loaded,
    /// The font failed to load.
    Error,
}

/// Represents a @font-face rule and its loading status.
/// CSS Fonts Module Level 4 §11.1 — FontFace interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontFace {
    /// The font-family name (e.g., "Roboto", "Inter").
    pub family: String,
    /// The font-style descriptor: "normal", "italic", or "oblique".
    pub style: String,
    /// The font-weight descriptor: "400", "700", "400 700", etc.
    pub weight: String,
    /// The font-stretch descriptor (optional): "normal", "condensed", etc.
    pub stretch: Option<String>,
    /// The unicode-range descriptor (optional): defines which Unicode characters this face supports.
    pub unicode_range: Option<String>,
    /// The src descriptor: comma-separated list of sources with format hints.
    pub src: String,
    /// Whether this font has been successfully loaded.
    pub status: FontFaceStatus,
}

impl FontFace {
    /// Create a new FontFace from @font-face rule components.
    pub fn new(
        family: String,
        style: String,
        weight: String,
        stretch: Option<String>,
        unicode_range: Option<String>,
        src: String,
    ) -> Self {
        Self {
            family,
            style,
            weight,
            stretch,
            unicode_range,
            src,
            status: FontFaceStatus::Unloaded,
        }
    }
}

/// A collection of FontFace objects representing all @font-face rules in the document.
/// CSS Fonts Module Level 4 §11.2 — FontFaceSet interface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontFaceSet {
    /// All FontFace objects extracted from stylesheets.
    faces: Vec<FontFace>,
}

impl FontFaceSet {
    /// Create a new empty FontFaceSet.
    pub fn new() -> Self {
        Self {
            faces: Vec::new(),
        }
    }

    /// Add a FontFace to the set.
    pub fn add(&mut self, face: FontFace) {
        self.faces.push(face);
    }

    /// Get the number of FontFaces in the set.
    pub fn size(&self) -> usize {
        self.faces.len()
    }

    /// Check if the set contains a FontFace with a specific family name.
    pub fn has_family(&self, family: &str) -> bool {
        self.faces.iter().any(|f| f.family == family)
    }

    /// Get all FontFaces with a specific family name.
    pub fn get_by_family(&self, family: &str) -> Vec<&FontFace> {
        self.faces.iter().filter(|f| f.family == family).collect()
    }

    /// Get all FontFaces.
    pub fn all(&self) -> &[FontFace] {
        &self.faces
    }

    /// Clear all FontFaces from the set.
    pub fn clear(&mut self) {
        self.faces.clear();
    }
}

impl Document {
    /// Get a reference to the document's FontFaceSet collection.
    /// Contains all FontFace objects extracted from @font-face rules in stylesheets.
    pub fn fonts(&self) -> &FontFaceSet {
        &self.fonts
    }

    /// Get a mutable reference to the document's FontFaceSet collection.
    /// Used internally to add FontFace objects as stylesheets are parsed.
    pub fn fonts_mut(&mut self) -> &mut FontFaceSet {
        &mut self.fonts
    }
}
