use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let native_root = manifest_dir.parent().expect("core manifest parent");
    let project_root = native_root.parent().expect("native workspace parent");
    println!("cargo:rerun-if-env-changed=DESKTOP_PET_RIG_JS_RUNTIME");
    println!("cargo:rerun-if-env-changed=DESKTOP_PET_DISTRIBUTION");
    println!("cargo:rustc-check-cfg=cfg(desktop_pet_legacy)");
    let distribution = env::var("DESKTOP_PET_DISTRIBUTION").unwrap_or_else(|_| "neutral".into());
    match distribution.as_str() {
        "neutral" => {}
        "legacy-0.3.13" => println!("cargo:rustc-cfg=desktop_pet_legacy"),
        _ => panic!("DESKTOP_PET_DISTRIBUTION must be neutral or legacy-0.3.13"),
    }
    for path in [
        "native/core/build.rs",
        "native/rig/RigBridge.h",
        "native/rig/RigBridge.swift",
        "native/rig/RigMotion.swift",
        "native/rig/RigNativeHost.swift",
        "native/rig/RigModel.swift",
        "native/rig/RigDecoder.swift",
        "native/rig/RigDecodeWorker.swift",
        "native/rig/RigDecodeWorker.entitlements.plist",
        "native/rig/RigDeformation.swift",
        "native/rig/RigHitTesting.swift",
        "native/rig/RigMetalRenderer.swift",
        "native/rig/RigMetalShaders.swift",
        "native/rig/limits.json",
        "native/rig/build-decoder.mjs",
        "native/rig/build-limits.mjs",
        "native/rig/decoder.ts",
        "native/rig/override-validation.ts",
        "native/rig/NOTICE.txt",
        "scripts/build-rig-native.mjs",
        "web/rig/vendor",
        "web/rig/package.json",
        "web/rig/package-lock.json",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            project_root.join(path).display()
        );
    }
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("macos") {
        return;
    }
    let profile = env::var("PROFILE").expect("PROFILE");
    let runtime =
        env::var_os("DESKTOP_PET_RIG_JS_RUNTIME").unwrap_or_else(|| OsString::from("bun"));
    let status = Command::new(&runtime)
        .arg(project_root.join("scripts/build-rig-native.mjs"))
        .current_dir(project_root)
        .env("DESKTOP_PET_RIG_BUILD_PROFILE", &profile)
        .env_remove("RIG_PROBE_FAULTS")
        .stdin(Stdio::null())
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "cannot execute {} for native rig build: {error}",
                Path::new(&runtime).display()
            )
        });
    assert!(status.success(), "native rig build failed with {status}");
    let output = native_root
        .join("target")
        .join(distribution)
        .join("rig-native")
        .join(profile);
    println!(
        "cargo:rustc-env=DESKTOP_PET_RIG_LIMITS_RS={}",
        output.join("Resources/rig/RigLimits.rs").display()
    );
    let dylib = output.join("libherdr_rig.dylib");
    let worker = output.join("rig-decode-worker");
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=dylib=herdr_rig");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", output.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!(
        "cargo:rustc-env=DESKTOP_PET_RIG_NATIVE_DYLIB={}",
        dylib.display()
    );
    println!(
        "cargo:rustc-env=DESKTOP_PET_RIG_DECODE_WORKER={}",
        worker.display()
    );
}
