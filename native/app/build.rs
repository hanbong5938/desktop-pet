use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=DESKTOP_PET_DISTRIBUTION");
    println!("cargo:rustc-check-cfg=cfg(desktop_pet_legacy)");
    let distribution = env::var("DESKTOP_PET_DISTRIBUTION").unwrap_or_else(|_| "neutral".into());
    match distribution.as_str() {
        "neutral" => {}
        "legacy-0.3.8" => println!("cargo:rustc-cfg=desktop_pet_legacy"),
        _ => panic!("DESKTOP_PET_DISTRIBUTION must be neutral or legacy-0.3.8"),
    }
    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("macos") {
        return;
    }
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let native_root = manifest_dir.parent().expect("app manifest parent");
    let profile = env::var("PROFILE").expect("PROFILE");
    let output = native_root
        .join("target")
        .join(distribution)
        .join("rig-native")
        .join(profile);
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", output.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
}
