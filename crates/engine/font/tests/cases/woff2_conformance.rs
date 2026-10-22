//! WOFF2 conformance: containers the W3C WOFF2 spec says MUST be rejected
//! (BUG-1404). Fixtures are the WPT `css/WOFF2/support/*.woff2` files.

use std::path::PathBuf;

fn support(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/wpt/css/WOFF2/support")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Files the spec says MUST be rejected, grouped by the violated condition.
const MUST_REJECT: &[(&str, &[&str])] = &[
    (
        "extraneous bytes between/after blocks",
        &[
            "blocks-extraneous-data-003", "blocks-extraneous-data-004",
            "blocks-extraneous-data-005", "blocks-extraneous-data-006",
            "blocks-extraneous-data-007", "blocks-extraneous-data-008",
        ],
    ),
    (
        "metadata/private block overlaps the preceding block",
        &["blocks-overlap-001", "blocks-overlap-002", "blocks-overlap-003"],
    ),
    ("UIntBase128 exceeds 2^32-1", &["datatypes-invalid-base128-002"]),
    ("header length differs from file size", &["header-length-001", "header-length-002"]),
    (
        "hmtx transform flags: reserved bits set / null transform",
        &["tabledata-transform-hmtx-003", "tabledata-transform-hmtx-004"],
    ),
    (
        "transform version not allowed for the table",
        &["tabledata-transform-bad-flag-001", "tabledata-transform-bad-flag-002"],
    ),
    (
        "loca: origLength off / transformed loca not empty",
        &[
            "tabledata-bad-origlength-loca-001", "tabledata-bad-origlength-loca-002",
            "tabledata-non-zero-loca-001",
        ],
    ),
    ("decompressed data longer than the tables", &["tabledata-decompressed-length-002"]),
    (
        "glyf bbox: composite without one / empty glyph with one",
        &["tabledata-glyf-bbox-002", "tabledata-glyf-bbox-003"],
    ),
    ("extraneous data before the last table", &["tabledata-extraneous-data-001"]),
];

#[test]
fn invalid_containers_are_rejected() {
    for (why, files) in MUST_REJECT {
        for f in *files {
            let r = lumen_font::decode_woff2(&support(&format!("{f}.woff2")));
            assert!(r.is_err(), "{f} ({why}) must be rejected");
        }
    }
}

/// The new checks must not turn away containers that are valid.
#[test]
fn valid_containers_still_decode() {
    for f in [
        "tabledata-glyf-bbox-001",
        "tabledata-recontruct-loca-001",
        "tabledata-transform-hmtx-001",
        "tabledata-transform-hmtx-002",
        "metadata-noeffect-001",
        "metadata-noeffect-002",
        "privatedata-noeffect-001",
        "valid-001",
    ] {
        let name = format!("{f}.woff2");
        let r = lumen_font::decode_woff2(&support(&name));
        assert!(r.is_ok(), "{f} must decode, got {:?}", r.err());
    }
}
