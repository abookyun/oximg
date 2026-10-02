fn main() {
    // The linear-light shrink hook (src/pipeline/linear_shrink.c) is
    // compiled against the headers of the libjpeg that mozjpeg-sys
    // builds, so it sees that build's struct layouts and ABI version.
    let include =
        std::env::var_os("DEP_JPEG_INCLUDE").expect("mozjpeg-sys exports its include dirs");
    let mut c = cc::Build::new();
    for dir in std::env::split_paths(&include) {
        c.include(dir);
    }
    // libjpeg reports fatal errors by unwinding through C frames (see
    // src/panic_guard.rs); this file's frames must allow it too.
    c.flag_if_supported("-fexceptions");
    c.file("src/pipeline/linear_shrink.c")
        .compile("oximg_linear_shrink");
    println!("cargo:rerun-if-changed=src/pipeline/linear_shrink.c");
    println!("cargo:rerun-if-changed=build.rs");

    // SVT-AV1 linking, only when the avif feature is enabled. Use a
    // Release build of the library: several distros ship debug builds
    // that encode at half speed.
    if std::env::var_os("CARGO_FEATURE_AVIF").is_some() {
        if let Ok(dir) = std::env::var("SVT_AV1_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
        } else {
            for dir in ["/opt/homebrew/lib", "/usr/local/lib", "/usr/lib"] {
                if std::path::Path::new(dir)
                    .join("libSvtAv1Enc.dylib")
                    .exists()
                    || std::path::Path::new(dir).join("libSvtAv1Enc.so").exists()
                    || std::path::Path::new(dir).join("libSvtAv1Enc.a").exists()
                {
                    println!("cargo:rustc-link-search=native={dir}");
                    break;
                }
            }
        }
        println!("cargo:rustc-link-lib=SvtAv1Enc");
        println!("cargo:rerun-if-env-changed=SVT_AV1_LIB_DIR");
    }
}
