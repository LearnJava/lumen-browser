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
    /// The font-feature-settings descriptor (optional), CSS Fonts L3 §6.4. Raw string.
    pub feature_settings: Option<String>,
    /// The font-variation-settings descriptor (optional), CSS Fonts L4 §7.4. Raw string.
    pub variation_settings: Option<String>,
    /// The font-display descriptor (optional): "auto" | "block" | "swap" | "fallback" | "optional".
    pub display: Option<String>,
    /// The ascent-override descriptor (optional), CSS Fonts L4 §14.1: "normal" | `<percentage>`. Raw string.
    pub ascent_override: Option<String>,
    /// The descent-override descriptor (optional), CSS Fonts L4 §14.2: "normal" | `<percentage>`. Raw string.
    pub descent_override: Option<String>,
    /// The line-gap-override descriptor (optional), CSS Fonts L4 §14.3: "normal" | `<percentage>`. Raw string.
    pub line_gap_override: Option<String>,
    /// The size-adjust descriptor (optional), CSS Fonts L4 §14.4: `<percentage>`. Raw string.
    pub size_adjust: Option<String>,
}

/// The CSS Fonts L4 §6-§14 descriptors not covered by [`FontFace::new`]'s
/// original (Level 3) parameter list. Grouped into one struct so
/// [`FontFace::with_extended_descriptors`] stays under clippy's
/// too-many-arguments limit — `lumen-dom` cannot take a `lumen-css-parser`
/// `FontFaceRule` reference directly, the two are sibling leaf crates with
/// no dependency between them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFaceExtendedDescriptors {
    /// The font-feature-settings descriptor (optional). Raw string.
    pub feature_settings: Option<String>,
    /// The font-variation-settings descriptor (optional). Raw string.
    pub variation_settings: Option<String>,
    /// The font-display descriptor (optional).
    pub display: Option<String>,
    /// The ascent-override descriptor (optional). Raw string.
    pub ascent_override: Option<String>,
    /// The descent-override descriptor (optional). Raw string.
    pub descent_override: Option<String>,
    /// The line-gap-override descriptor (optional). Raw string.
    pub line_gap_override: Option<String>,
    /// The size-adjust descriptor (optional). Raw string.
    pub size_adjust: Option<String>,
}

impl FontFace {
    /// Create a new FontFace from @font-face rule components.
    ///
    /// The CSS Fonts L4 §6-§14 descriptors (`font-feature-settings`,
    /// `font-variation-settings`, `font-display`, the four metrics-override
    /// descriptors) default to `None` here — use [`Self::with_extended_descriptors`]
    /// to set them from a fully parsed `@font-face` rule.
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
            feature_settings: None,
            variation_settings: None,
            display: None,
            ascent_override: None,
            descent_override: None,
            line_gap_override: None,
            size_adjust: None,
        }
    }

    /// Set the CSS Fonts L4 descriptors not covered by [`Self::new`]'s
    /// original (Level 3) parameter list — kept as a separate builder step
    /// rather than growing `new`'s already six-argument signature further.
    #[must_use]
    pub fn with_extended_descriptors(mut self, descriptors: FontFaceExtendedDescriptors) -> Self {
        self.feature_settings = descriptors.feature_settings;
        self.variation_settings = descriptors.variation_settings;
        self.display = descriptors.display;
        self.ascent_override = descriptors.ascent_override;
        self.descent_override = descriptors.descent_override;
        self.line_gap_override = descriptors.line_gap_override;
        self.size_adjust = descriptors.size_adjust;
        self
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

    /// Flip every face `predicate` selects to [`FontFaceStatus::Loaded`].
    ///
    /// FONTLOAD-2 (`bugs/BUG-467-OPEN.md` gap 2): a CSS-declared `url()`
    /// face's status previously never left `Loading`/`Unloaded` — the
    /// background fetch that resolves it (`LoadEvent::FontLoaded`,
    /// `crates/shell/src/app/user_event.rs`) updated the render font
    /// registry but never this collection. Called from that handler once
    /// the fetch completes.
    pub fn mark_loaded(&mut self, mut predicate: impl FnMut(&FontFace) -> bool) {
        for face in &mut self.faces {
            if predicate(face) {
                face.status = FontFaceStatus::Loaded;
            }
        }
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
