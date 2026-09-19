//! Линкует FFmpeg shared-dev дистрибутив (`FFMPEG_DIR/lib` + `FFMPEG_DIR/bin`)
//! только когда включена feature `ffmpeg` — без неё крейт не трогает FFmpeg
//! вовсе, чтобы `cargo build --workspace` не требовал FFmpeg ни на одной
//! машине, кроме той, что явно просит эту feature (ADR-030).
//!
//! Постоянное исключение из `clippy::panic` (docs/lint-policy.md §10):
//! build-скрипт не входит в собираемый продукт, паника здесь — это
//! диагностика конфигурации сборки, а не поведение движка в рантайме.
#![allow(clippy::panic)]

fn main() {
    println!("cargo::rustc-check-cfg=cfg(ffmpeg_linked)");

    let ffmpeg_feature = std::env::var_os("CARGO_FEATURE_FFMPEG").is_some();
    if !ffmpeg_feature {
        return;
    }

    let ffmpeg_dir = std::env::var("FFMPEG_DIR").unwrap_or_else(|_| {
        panic!(
            "lumen-media-ffmpeg: feature \"ffmpeg\" включена, но переменная FFMPEG_DIR не \
             задана. Она должна указывать на FFmpeg shared-dev дистрибутив (headers + import \
             libs + DLL, например GyanD/codexffmpeg full_build-shared) — см. \
             docs/decisions/ADR-030-media-codec-strategy-ffmpeg.md."
        )
    });

    let lib_dir = format!("{ffmpeg_dir}/lib");
    let bin_dir = format!("{ffmpeg_dir}/bin");
    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-search=native={bin_dir}");
    println!("cargo:rustc-link-lib=dylib=avformat");
    println!("cargo:rustc-link-lib=dylib=avcodec");
    println!("cargo:rustc-link-lib=dylib=avutil");
    println!("cargo:rustc-link-lib=dylib=swscale");
    println!("cargo:rustc-cfg=ffmpeg_linked");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
}
