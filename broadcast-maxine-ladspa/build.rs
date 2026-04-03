use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/maxine_ladspa.c");
    println!("cargo:rerun-if-env-changed=NVAFX_SDK");
    // Declare has_nvafx as a valid cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_nvafx)");

    let Some(sdk_root) = find_nvafx_sdk() else {
        println!("cargo:warning=──────────────────────────────────────────────────");
        println!("cargo:warning=NVIDIA Audio Effects SDK not found.");
        println!("cargo:warning=GPU noise suppression (Maxine backend) will not be built.");
        println!("cargo:warning=");
        println!("cargo:warning=To enable it:");
        println!("cargo:warning=  1. Run: NGC_API_KEY=<key> bash scripts/install-maxine-sdk.sh");
        println!("cargo:warning=  2. Re-run: cargo build --release");
        println!("cargo:warning=──────────────────────────────────────────────────");
        return;
    };

    println!(
        "cargo:warning=Building NVIDIA Maxine LADSPA plugin (SDK: {})",
        sdk_root.display()
    );

    // SDK v2.x layout:
    //   nvafx/include/       — nvAudioEffects.h (core API)
    //   features/denoiser/include/ — denoiser.h (effect selector)
    //   nvafx/lib/           — libnv_audiofx.so (base SDK)
    //   features/denoiser/lib/ — libnv_audiofx_denoiser.so (denoiser feature)
    //   external/cuda/lib/   — bundled CUDA/TRT/cuDNN (no system CUDA needed)
    let core_include  = sdk_root.join("nvafx/include");
    let feat_include  = sdk_root.join("features/denoiser/include");
    let core_lib      = sdk_root.join("nvafx/lib");
    let feat_lib      = sdk_root.join("features/denoiser/lib");
    let bundled_cuda  = sdk_root.join("external/cuda/lib");

    cc::Build::new()
        .file("src/maxine_ladspa.c")
        .include(&core_include)
        .include(&feat_include)
        .flag_if_supported("-Wno-format-truncation")
        .opt_level(2)
        .compile("maxine_ladspa_c");

    // Link SDK shared libs (dynamically — they ship their own TRT/CUDA).
    println!("cargo:rustc-link-search=native={}", core_lib.display());
    println!("cargo:rustc-link-lib=dylib=nv_audiofx");

    println!("cargo:rustc-link-search=native={}", feat_lib.display());
    println!("cargo:rustc-link-lib=dylib=nv_audiofx_denoiser");

    // pthread for serialising concurrent TRT engine loads
    println!("cargo:rustc-link-lib=dylib=pthread");
    // dl for RTLD_GLOBAL preloading of libnvinfer_plugin in the constructor
    println!("cargo:rustc-link-lib=dylib=dl");

    // Bundled CUDA/TRT — runtime only, not linked at build time.
    // We emit this so LD_LIBRARY_PATH hints are available, but the libs
    // themselves are loaded transitively by libnv_audiofx.so at runtime.
    if bundled_cuda.exists() {
        println!("cargo:rustc-link-search=native={}", bundled_cuda.display());
    }

    println!("cargo:rustc-cfg=has_nvafx");
}

/// Find the SDK root (the `Audio_Effects_SDK` directory).
/// Returns `Some(path)` when `nvafx/include/nvAudioEffects.h` is present.
fn find_nvafx_sdk() -> Option<PathBuf> {
    if let Ok(val) = std::env::var("NVAFX_SDK") {
        let p = PathBuf::from(&val);
        if p.join("nvafx/include/nvAudioEffects.h").exists() {
            return Some(p);
        }
        // Accept NVAFX_SDK pointing one level up (containing current/ symlink).
        let current = p.join("current");
        if current.join("nvafx/include/nvAudioEffects.h").exists() {
            return Some(current);
        }
        eprintln!("build.rs: NVAFX_SDK={val} set but nvAudioEffects.h not found there");
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{home}/.local/share/nvidia-maxine-sdk/current"),
        format!("{home}/.local/share/nvidia-maxine-sdk/2.1.0"),
        "/opt/nvidia-maxine-sdk/current".to_string(),
        "/opt/nvidia-maxine-sdk".to_string(),
    ];

    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.join("nvafx/include/nvAudioEffects.h").exists() {
            return Some(p);
        }
    }
    None
}
