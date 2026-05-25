# ADR-005: zune-jpeg + zune-png as provisional image decoders

## Status

Accepted

## Date

2026-05-22

## Context

Lumen initially had custom JPEG and PNG decoders in `lumen-image`. The custom JPEG had two correctness issues:
1. Nearest-neighbor chroma upsampling instead of bilinear — visible color difference vs Chrome/Edge.
2. ICC profiles (APP2) ignored — incorrect colors on ICC-tagged images.

The custom PNG worked correctly but was ~2000 lines (inflate/filter/Adam7/palette/tRNS).

`zune-jpeg` and `zune-png` were already listed in the provisional dependency plan (§5 of lumen-plan.md) as the expected path for image decoding.

## Decision

Replace custom JPEG and PNG decoders with `zune-jpeg` + `zune-png` from the `zune-core` ecosystem.

These are **provisional accelerators** behind the `ImageDecoder` trait in `lumen-core::ext`.

Public API (`decode()`, `decode_jpeg()`, `decode_png()`, `Image`, `PixelFormat`) is unchanged.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Fix custom JPEG (add bilinear upsampling + ICC) | Implementing correct ICC profile application and bilinear chroma upsampling is ~500 LOC of additional specialized code with many edge cases. The custom decoder has no architectural value. |
| Keep custom PNG, replace only JPEG | Increases maintenance burden: two different decoder architectures in one crate. zune-png has identical API and behavior for all tested cases. |
| `image` / `png` crates | Heavier dependency graph; `image` is a large umbrella crate, we only need decoders. |

## Consequences

- **Positive:** bilinear chroma upsampling matches Chrome/Edge output; ICC profile support; ~2000 lines of decoder code removed; MIT/Apache/Zlib licensed; pure Rust, no-unsafe in decoder code.
- **Negative:** supply-chain dependency on `zune-*`; one behavior difference: zune-png silently accepts palette PNG without PLTE chunk (custom decoder returned error — test updated to "does not panic").
- **Future (graduation criterion):** if `zune-*` is abandoned or a CVE appears — restore custom PNG (git history preserved, well-tested). Custom JPEG stays provisional indefinitely: implementing DCT+Huffman+ICC from scratch has no architectural value.
