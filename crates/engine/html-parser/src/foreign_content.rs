//! SVG foreign-content rules per HTML Living Standard §13.2.6.5 — reduced to
//! the shape [`crate::tree_builder`] needs (GAP-XMLDOC срез 3, BUG-685): SVG
//! only. Not implemented, and out of scope for this slice: MathML, the
//! HTML/MathML "integration point" exceptions (`<foreignObject>`/`<desc>`/
//! `<title>` do **not** switch children back to the HTML namespace),
//! foreign-attribute namespacing (`xlink:href` stays a plain attribute
//! instead of gaining `Namespace::XLink`). See `bugs/BUG-685-OPEN.md` for
//! the measured remainder.
//!
//! This module only supplies the static lookup tables and the "does this
//! start tag break out of foreign content" decision — the tree builder
//! drives the actual stack manipulation.

/// "Adjust SVG tag names" (§13.2.6.5, "insert a foreign element"). The
/// tokenizer already lower-cases every tag name (§13.2.5.8 "tag name
/// state"), so this maps the lower-cased form back to the SVG spec's
/// mixed-case local name. A name absent from the table means the
/// lower-case form is already correct (`rect`, `circle`, `path`, `g`,
/// `svg`, ...).
pub(crate) fn adjust_svg_tag_name(lower: &str) -> &str {
    match lower {
        "altglyph" => "altGlyph",
        "altglyphdef" => "altGlyphDef",
        "altglyphitem" => "altGlyphItem",
        "animatecolor" => "animateColor",
        "animatemotion" => "animateMotion",
        "animatetransform" => "animateTransform",
        "clippath" => "clipPath",
        "feblend" => "feBlend",
        "fecolormatrix" => "feColorMatrix",
        "fecomponenttransfer" => "feComponentTransfer",
        "fecomposite" => "feComposite",
        "feconvolvematrix" => "feConvolveMatrix",
        "fediffuselighting" => "feDiffuseLighting",
        "fedisplacementmap" => "feDisplacementMap",
        "fedistantlight" => "feDistantLight",
        "fedropshadow" => "feDropShadow",
        "feflood" => "feFlood",
        "fefunca" => "feFuncA",
        "fefuncb" => "feFuncB",
        "fefuncg" => "feFuncG",
        "fefuncr" => "feFuncR",
        "fegaussianblur" => "feGaussianBlur",
        "feimage" => "feImage",
        "femerge" => "feMerge",
        "femergenode" => "feMergeNode",
        "femorphology" => "feMorphology",
        "feoffset" => "feOffset",
        "fepointlight" => "fePointLight",
        "fespecularlighting" => "feSpecularLighting",
        "fespotlight" => "feSpotLight",
        "fetile" => "feTile",
        "feturbulence" => "feTurbulence",
        "foreignobject" => "foreignObject",
        "glyphref" => "glyphRef",
        "lineargradient" => "linearGradient",
        "radialgradient" => "radialGradient",
        "textpath" => "textPath",
        other => other,
    }
}

/// "Adjust SVG attribute names" (§13.2.6.5) — same shape as
/// [`adjust_svg_tag_name`], for attributes (`viewBox`, `preserveAspectRatio`,
/// ...). The tokenizer lower-cases attribute names the same way it
/// lower-cases tag names.
pub(crate) fn adjust_svg_attribute_name(lower: &str) -> &str {
    match lower {
        "attributename" => "attributeName",
        "attributetype" => "attributeType",
        "basefrequency" => "baseFrequency",
        "baseprofile" => "baseProfile",
        "calcmode" => "calcMode",
        "clippathunits" => "clipPathUnits",
        "diffuseconstant" => "diffuseConstant",
        "edgemode" => "edgeMode",
        "filterunits" => "filterUnits",
        "glyphref" => "glyphRef",
        "gradienttransform" => "gradientTransform",
        "gradientunits" => "gradientUnits",
        "kernelmatrix" => "kernelMatrix",
        "kernelunitlength" => "kernelUnitLength",
        "keypoints" => "keyPoints",
        "keysplines" => "keySplines",
        "keytimes" => "keyTimes",
        "lengthadjust" => "lengthAdjust",
        "limitingconeangle" => "limitingConeAngle",
        "markerheight" => "markerHeight",
        "markerunits" => "markerUnits",
        "markerwidth" => "markerWidth",
        "maskcontentunits" => "maskContentUnits",
        "maskunits" => "maskUnits",
        "numoctaves" => "numOctaves",
        "pathlength" => "pathLength",
        "patterncontentunits" => "patternContentUnits",
        "patterntransform" => "patternTransform",
        "patternunits" => "patternUnits",
        "pointsatx" => "pointsAtX",
        "pointsaty" => "pointsAtY",
        "pointsatz" => "pointsAtZ",
        "preservealpha" => "preserveAlpha",
        "preserveaspectratio" => "preserveAspectRatio",
        "primitiveunits" => "primitiveUnits",
        "refx" => "refX",
        "refy" => "refY",
        "repeatcount" => "repeatCount",
        "repeatdur" => "repeatDur",
        "requiredextensions" => "requiredExtensions",
        "requiredfeatures" => "requiredFeatures",
        "specularconstant" => "specularConstant",
        "specularexponent" => "specularExponent",
        "spreadmethod" => "spreadMethod",
        "startoffset" => "startOffset",
        "stddeviation" => "stdDeviation",
        "stitchtiles" => "stitchTiles",
        "surfacescale" => "surfaceScale",
        "systemlanguage" => "systemLanguage",
        "tablevalues" => "tableValues",
        "targetx" => "targetX",
        "targety" => "targetY",
        "textlength" => "textLength",
        "viewbox" => "viewBox",
        "viewtarget" => "viewTarget",
        "xchannelselector" => "xChannelSelector",
        "ychannelselector" => "yChannelSelector",
        "zoomandpan" => "zoomAndPan",
        other => other,
    }
}

/// §13.2.6.5 "any other start tag" breakout list: these HTML tag names pop
/// back out of foreign content instead of becoming a foreign (SVG) element,
/// even while the current node is SVG. `font` only breaks out when it
/// carries a `color`, `face`, or `size` attribute — the spec's carve-out for
/// legacy markup that nests a `<font>` inside inline SVG expecting HTML
/// semantics.
pub(crate) fn breaks_out_of_foreign_content(lower_name: &str, attrs: &[(String, String)]) -> bool {
    match lower_name {
        "b" | "big" | "blockquote" | "body" | "br" | "center" | "code" | "dd" | "div" | "dl"
        | "dt" | "em" | "embed" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head" | "hr"
        | "i" | "img" | "li" | "listing" | "menu" | "meta" | "nobr" | "ol" | "p" | "pre"
        | "ruby" | "s" | "small" | "span" | "strong" | "strike" | "sub" | "sup" | "table"
        | "tt" | "u" | "ul" | "var" => true,
        "font" => attrs
            .iter()
            .any(|(k, _)| matches!(k.as_str(), "color" | "face" | "size")),
        _ => false,
    }
}

/// Strips a namespace prefix bound to XHTML in the vendored WPT corpus
/// (`xmlns:h="…/1999/xhtml"`, `xmlns:html="…/1999/xhtml"` — both forms
/// occur, `h:` is by far the more common one) and returns the local name
/// beneath it, e.g. `strip_known_html_prefix("h:script") == Some("script")`.
///
/// This is a hardcoded pair, not a resolver: real XML namespace resolution
/// walks the ancestor chain for `xmlns:*` declarations, which is out of
/// scope here (GAP-XMLDOC срез 5, same "point fix, not a resolver"
/// boundary as the rest of this module — see `bugs/BUG-685-OPEN.md`
/// "Третья грань, случай 1"). Other prefixes seen in the same corpus
/// (`d:testDescription`, `m:mi`, `rdf:li`, `svg:svg`) are bound to
/// different namespaces (SVG 1.1 test metadata, MathML, RDF, SVG itself)
/// and must NOT break out — `strip_known_html_prefix` only ever matches
/// `h:`/`html:`.
pub(crate) fn strip_known_html_prefix(lower_name: &str) -> Option<&str> {
    lower_name
        .strip_prefix("html:")
        .or_else(|| lower_name.strip_prefix("h:"))
        .filter(|suffix| !suffix.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_svg_tag_case() {
        assert_eq!(adjust_svg_tag_name("lineargradient"), "linearGradient");
        assert_eq!(adjust_svg_tag_name("foreignobject"), "foreignObject");
        assert_eq!(adjust_svg_tag_name("fegaussianblur"), "feGaussianBlur");
    }

    #[test]
    fn leaves_already_lowercase_tags_alone() {
        assert_eq!(adjust_svg_tag_name("rect"), "rect");
        assert_eq!(adjust_svg_tag_name("svg"), "svg");
        assert_eq!(adjust_svg_tag_name("path"), "path");
    }

    #[test]
    fn restores_svg_attribute_case() {
        assert_eq!(adjust_svg_attribute_name("viewbox"), "viewBox");
        assert_eq!(
            adjust_svg_attribute_name("preserveaspectratio"),
            "preserveAspectRatio"
        );
    }

    #[test]
    fn plain_attributes_left_alone() {
        assert_eq!(adjust_svg_attribute_name("id"), "id");
        assert_eq!(adjust_svg_attribute_name("class"), "class");
    }

    #[test]
    fn breakout_tags_detected() {
        assert!(breaks_out_of_foreign_content("div", &[]));
        assert!(breaks_out_of_foreign_content("p", &[]));
        assert!(!breaks_out_of_foreign_content("rect", &[]));
        assert!(!breaks_out_of_foreign_content("g", &[]));
    }

    #[test]
    fn font_breaks_out_only_with_legacy_attrs() {
        assert!(!breaks_out_of_foreign_content("font", &[]));
        assert!(breaks_out_of_foreign_content(
            "font",
            &[("color".to_string(), "red".to_string())]
        ));
        assert!(!breaks_out_of_foreign_content(
            "font",
            &[("id".to_string(), "x".to_string())]
        ));
    }

    #[test]
    fn strips_known_html_prefixes() {
        assert_eq!(strip_known_html_prefix("h:script"), Some("script"));
        assert_eq!(strip_known_html_prefix("html:link"), Some("link"));
        assert_eq!(strip_known_html_prefix("h:div"), Some("div"));
    }

    #[test]
    fn leaves_other_prefixes_and_bare_names_alone() {
        assert_eq!(strip_known_html_prefix("d:testDescription"), None);
        assert_eq!(strip_known_html_prefix("m:mi"), None);
        assert_eq!(strip_known_html_prefix("rdf:li"), None);
        assert_eq!(strip_known_html_prefix("svg:svg"), None);
        assert_eq!(strip_known_html_prefix("script"), None);
        assert_eq!(strip_known_html_prefix("h:"), None);
        assert_eq!(strip_known_html_prefix("html:"), None);
    }
}
