//! SVG and MathML foreign-content rules per HTML Living Standard §13.2.6.5
//! — reduced to the shape [`crate::tree_builder`] needs. SVG namespacing
//! (GAP-XMLDOC срез 3, BUG-685), MathML namespacing (GAP-XMLDOC срез 6,
//! same bug) and the HTML/SVG/MathML "integration point" exceptions
//! (GAP-XMLDOC срез 8, same bug — `<foreignObject>`/`<desc>`/`<title>`,
//! MathML `<annotation-xml>` with an HTML-flavoured `encoding`, and the
//! MathML text integration points `<mi>`/`<mo>`/`<mn>`/`<ms>`/`<mtext>`) are
//! all covered; the §13.2.6.5 "any other start tag" breakout list is shared
//! between the two namespaces per spec. Foreign-attribute namespacing
//! (`xlink:href` and friends, GAP-XMLDOC срез 10, same bug) is covered for
//! the eleven names §13.2.6.5 "adjust foreign attributes" lists; general
//! namespace-prefix resolution (an arbitrary `xmlns:foo="..."` binding) is
//! not — same "point fix, not a resolver" boundary as
//! [`strip_known_html_prefix`]. See `bugs/BUG-685-OPEN.md` for the measured
//! remainder.
//!
//! This module only supplies the static lookup tables and the "does this
//! start tag break out of foreign content" decision — the tree builder
//! drives the actual stack manipulation.

use lumen_dom::Namespace;

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

/// "Adjust MathML attribute names" (§13.2.6.5, "insert a foreign element")
/// — MathML has exactly one case-sensitive attribute, unlike SVG's dozens;
/// every other name is already correct lower-case.
pub(crate) fn adjust_mathml_attribute_name(lower: &str) -> &str {
    match lower {
        "definitionurl" => "definitionURL",
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

/// "Adjust foreign attributes" (§13.2.6.5) — the eleven `xlink:`/`xml:`/
/// `xmlns` attribute names get a real namespace instead of staying a plain
/// HTML-namespaced attribute, on any element in the SVG or MathML namespace
/// (the spec runs this step for both, not per-namespace like the tag/
/// attribute-case tables above). The tokenizer already lower-cases every
/// attribute name (§13.2.5.32 "attribute name state"), and all eleven names
/// are already lower-case in the spec, so there is no case to restore here
/// — this table only classifies. `local` in the returned pair is the
/// qualified name as written (`xlink:href`, not `href`): Lumen's attribute
/// model has no separate prefix field, so the qualified string doubles as
/// both the storage key existing `Node::get_attr("xlink:href")` call sites
/// already use and the serialization name; only `namespace` changes.
pub(crate) fn adjust_foreign_attribute(name: &str) -> Option<Namespace> {
    match name {
        "xlink:actuate" | "xlink:arcrole" | "xlink:href" | "xlink:role" | "xlink:show"
        | "xlink:title" | "xlink:type" => Some(Namespace::XLink),
        "xml:lang" | "xml:space" => Some(Namespace::Xml),
        "xmlns" | "xmlns:xlink" => Some(Namespace::XmlNs),
        _ => None,
    }
}

/// §13.2.6.5 "any other start tag" breakout list: these HTML tag names pop
/// back out of foreign content instead of becoming a foreign (SVG or
/// MathML) element, even while the current node is foreign — the spec
/// shares this exact list between both namespaces. `font` only breaks out
/// when it carries a `color`, `face`, or `size` attribute — the spec's
/// carve-out for legacy markup that nests a `<font>` inside inline SVG/
/// MathML expecting HTML semantics.
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

/// HTML LS §13.2.6.5 "HTML integration point" — SVG side: `<foreignObject>`,
/// `<desc>`, `<title>`. A start tag or character token encountered while the
/// current node is one of these is processed as if it were HTML content
/// (new elements get the HTML namespace) instead of the ordinary
/// foreign-content rules — the MathML side of the same concept
/// (`annotation-xml` with an HTML-flavoured `encoding`) needs the element's
/// attributes, so it lives on `IncrementalTreeBuilder` directly rather than
/// here (GAP-XMLDOC срез 8, BUG-685).
pub(crate) fn is_svg_html_integration_point(local: &str) -> bool {
    matches!(local, "foreignObject" | "desc" | "title")
}

/// HTML LS §13.2.6.5 "MathML text integration point": `<mi>`, `<mo>`,
/// `<mn>`, `<ms>`, `<mtext>`. A start tag whose name is neither `mglyph` nor
/// `malignmark`, or a character token, is processed as HTML content while
/// the current node is one of these — same effect as an HTML integration
/// point, but the exception list differs (GAP-XMLDOC срез 8, BUG-685).
pub(crate) fn is_mathml_text_integration_point(local: &str) -> bool {
    matches!(local, "mi" | "mo" | "mn" | "ms" | "mtext")
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
    fn adjusts_mathml_definitionurl() {
        assert_eq!(adjust_mathml_attribute_name("definitionurl"), "definitionURL");
        assert_eq!(adjust_mathml_attribute_name("id"), "id");
        assert_eq!(adjust_mathml_attribute_name("encoding"), "encoding");
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
    fn foreign_attributes_get_their_namespace() {
        assert_eq!(adjust_foreign_attribute("xlink:href"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:show"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:actuate"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:arcrole"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:role"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:title"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xlink:type"), Some(Namespace::XLink));
        assert_eq!(adjust_foreign_attribute("xml:lang"), Some(Namespace::Xml));
        assert_eq!(adjust_foreign_attribute("xml:space"), Some(Namespace::Xml));
        assert_eq!(adjust_foreign_attribute("xmlns"), Some(Namespace::XmlNs));
        assert_eq!(adjust_foreign_attribute("xmlns:xlink"), Some(Namespace::XmlNs));
    }

    #[test]
    fn plain_and_unknown_prefixed_attributes_are_not_foreign() {
        assert_eq!(adjust_foreign_attribute("href"), None);
        assert_eq!(adjust_foreign_attribute("id"), None);
        // Custom xmlns bindings other than the two the spec lists by name
        // are out of scope (general namespace resolution, not a point fix).
        assert_eq!(adjust_foreign_attribute("xmlns:foo"), None);
        assert_eq!(adjust_foreign_attribute("xml:base"), None);
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
    fn svg_integration_points_detected() {
        assert!(is_svg_html_integration_point("foreignObject"));
        assert!(is_svg_html_integration_point("desc"));
        assert!(is_svg_html_integration_point("title"));
        assert!(!is_svg_html_integration_point("rect"));
        assert!(!is_svg_html_integration_point("g"));
    }

    #[test]
    fn mathml_text_integration_points_detected() {
        assert!(is_mathml_text_integration_point("mi"));
        assert!(is_mathml_text_integration_point("mo"));
        assert!(is_mathml_text_integration_point("mn"));
        assert!(is_mathml_text_integration_point("ms"));
        assert!(is_mathml_text_integration_point("mtext"));
        assert!(!is_mathml_text_integration_point("math"));
        assert!(!is_mathml_text_integration_point("annotation-xml"));
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
