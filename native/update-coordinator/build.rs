fn main() {
    println!("cargo:rerun-if-env-changed=DESKTOP_PET_DISTRIBUTION");
    println!("cargo:rustc-check-cfg=cfg(desktop_pet_legacy)");
    match std::env::var("DESKTOP_PET_DISTRIBUTION").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("neutral") => {}
        Ok("legacy-0.3.12") => {
            println!("cargo:rustc-cfg=desktop_pet_legacy");
            println!("cargo:rerun-if-env-changed=DESKTOP_PET_HERDR_MIGRATION_REV");
            let revision = std::env::var("DESKTOP_PET_HERDR_MIGRATION_REV")
                .expect("legacy bridge requires the published immutable Herdr integration-only migration revision");
            assert!(
                revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "DESKTOP_PET_HERDR_MIGRATION_REV must be a 40-hex published immutable revision"
            );
            println!("cargo:rustc-env=DESKTOP_PET_COMPILED_HERDR_MIGRATION_REVISION={revision}");
        }
        _ => panic!("DESKTOP_PET_DISTRIBUTION must be neutral or legacy-0.3.12"),
    }
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .canonicalize()
        .expect("core repository root");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("reading core revision");
        assert!(output.status.success(), "cannot read core git revision");
        String::from_utf8(output.stdout)
            .expect("UTF-8 core git metadata")
            .trim()
            .to_owned()
    };
    let revision = git(&["rev-parse", "HEAD"]);
    assert!(
        revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid core commit identity"
    );
    println!("cargo:rustc-env=DESKTOP_PET_COMPILED_CORE_REVISION={revision}");
    let dependency = |name: &str| {
        let path = std::path::PathBuf::from(git(&["rev-parse", "--git-path", name]));
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        println!("cargo:rerun-if-changed={}", path.display());
    };
    for file in ["HEAD", "packed-refs"] {
        dependency(file);
    }
    let reference = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&root)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .expect("reading core branch");
    if reference.status.success() {
        let name = String::from_utf8(reference.stdout).expect("UTF-8 core branch");
        dependency(name.trim());
    }
}
