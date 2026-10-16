//! Retain actual owning-manager image provenance before legacy retirement.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LegacySourceAncestor {
    pub image: ExecutableIdentity,
    pub receipt_path: PathBuf,
    pub receipt_sha256: String,
}
pub(super) fn capture_source_ancestry(
    origin: &LegacyOrigin,
    current: &ExecutableIdentity,
) -> Result<Vec<LegacySourceAncestor>, String> {
    let LegacyOrigin::Homebrew {
        cellar_formula_root,
        formula,
        ..
    } = origin
    else {
        return Ok(Vec::new());
    };
    let formula_name = formula
        .rsplit('/')
        .next()
        .ok_or("legacy formula has no name")?;
    let formula_file = format!("{formula_name}.rb");
    owned_directory(cellar_formula_root)?;
    let mut ancestry = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(cellar_formula_root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        count += 1;
        if count > 64 {
            return Err("legacy owning-manager image ancestry exceeds bound".into());
        }
        let keg = entry.path();
        owned_directory(&keg)?;
        let receipt_path = keg.join("INSTALL_RECEIPT.json");
        let image_path = keg.join("libexec/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet");
        if image_path == current.path
            || !image_path.try_exists().map_err(|e| e.to_string())?
            || !receipt_path.try_exists().map_err(|e| e.to_string())?
        {
            continue;
        }
        let receipt = crate::owned_file(&receipt_path, 65536)?;
        let value: serde_json::Value =
            serde_json::from_slice(&receipt).map_err(|e| e.to_string())?;
        let source_path = value["source"]["path"].as_str().map(Path::new);
        if value["source"]["tap"].as_str() != Some("hanbong5938/tap")
            || source_path
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                != Some(formula_file.as_str())
        {
            continue;
        }
        let image = executable_identity(&image_path)?;
        let bundle = keg.join("libexec/HerdrDesktopPet.app");
        command_output(
            Path::new("/usr/bin/codesign"),
            &[
                "--verify".into(),
                "--deep".into(),
                "--strict".into(),
                bundle.to_string_lossy().into_owned(),
            ],
        )?;
        let output = command_output(&image.path, &["--version".into()])?;
        let version = semver::Version::parse(
            output
                .trim()
                .strip_prefix("herdr-desktop-pet ")
                .ok_or("legacy owner ancestry names another product")?,
        )
        .map_err(|e| e.to_string())?;
        if version.major != 0 || version >= semver::Version::new(0, 3, 13) {
            continue;
        }
        ancestry.push(LegacySourceAncestor {
            image,
            receipt_path,
            receipt_sha256: format!("{:x}", Sha256::digest(receipt)),
        });
    }
    ancestry.sort_by(|left, right| left.image.path.cmp(&right.image.path));
    Ok(ancestry)
}
pub(super) fn validate_source_ancestry(plan: &MigrationPlan) -> Result<(), String> {
    if plan.source_ancestry.len() > 64 {
        return Err("legacy source ancestry exceeds bound".into());
    }
    for (index, ancestor) in plan.source_ancestry.iter().enumerate() {
        image_valid(&ancestor.image)?;
        crate::ensure_absolute(&ancestor.receipt_path)?;
        let LegacyOrigin::Homebrew {
            cellar_formula_root,
            ..
        } = &plan.source_origin
        else {
            return Err("legacy source ancestry lacks owning Brew proof".into());
        };
        let relative = ancestor
            .image
            .path
            .strip_prefix(cellar_formula_root)
            .map_err(|_| "legacy ancestor escaped its approved owner root")?;
        let mut components = relative.components();
        let keg = components
            .next()
            .ok_or("legacy ancestor has no owning keg")?;
        if !matches!(keg, std::path::Component::Normal(_))
            || components.as_path()
                != Path::new("libexec/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet")
            || ancestor.receipt_path
                != cellar_formula_root
                    .join(keg.as_os_str())
                    .join("INSTALL_RECEIPT.json")
            || !hex(&ancestor.receipt_sha256, 64)
            || plan.source_ancestry[..index]
                .iter()
                .any(|previous| previous.image.path == ancestor.image.path)
        {
            return Err(
                "legacy source ancestry is not an exact approved owning-manager image/receipt pair"
                    .into(),
            );
        }
    }
    Ok(())
}
pub(super) fn verify_source_ancestry(plan: &MigrationPlan) -> Result<(), String> {
    validate_source_ancestry(plan)?;
    for ancestor in &plan.source_ancestry {
        if executable_identity(&ancestor.image.path)? != ancestor.image
            || format!(
                "{:x}",
                Sha256::digest(crate::owned_file(&ancestor.receipt_path, 65536)?)
            ) != ancestor.receipt_sha256
        {
            return Err("legacy owning-manager ancestry changed before migration; no route authority was fabricated".into());
        }
    }
    Ok(())
}

/// Resolve only exact retained routes. In particular, a deleted Brew alias is
/// never reconstructed from its basename, prefix convention or another profile.
pub(super) fn retained_source_route(
    plan: &MigrationPlan,
    executable: &Path,
    config: &Path,
    state: &Path,
) -> Result<Option<ExecutableIdentity>, String> {
    if !plan
        .profiles
        .iter()
        .any(|profile| profile.config_dir == config && profile.state_dir == state)
    {
        return Ok(None);
    }
    if executable == plan.source_image.path
        || plan.legacy_aliases.iter().any(|alias| alias == executable)
    {
        return Ok(Some(plan.source_image.clone()));
    }
    if let Some(ancestor) = plan
        .source_ancestry
        .iter()
        .find(|ancestor| ancestor.image.path == executable)
    {
        return Ok(Some(ancestor.image.clone()));
    }
    approval::original_source_route(plan, executable, config, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_brew_alias_requires_exact_retained_route_and_original_pair() {
        let (_temp, mut plan) = crate::migration::tests::fixture();
        let alias = plan.profiles[0]
            .state_dir
            .join("retired/bin/herdr-desktop-pet");
        assert!(!alias.exists());
        plan.legacy_aliases.push(alias.clone());
        let profile = &plan.profiles[0];
        assert_eq!(
            retained_source_route(&plan, &alias, &profile.config_dir, &profile.state_dir).unwrap(),
            Some(plan.source_image.clone())
        );
        assert!(retained_source_route(
            &plan,
            &alias,
            &profile.config_dir,
            &profile.state_dir.join("other")
        )
        .unwrap()
        .is_none());
        assert!(retained_source_route(
            &plan,
            &alias.with_file_name("desktop-pet"),
            &profile.config_dir,
            &profile.state_dir
        )
        .unwrap()
        .is_none());
    }
}
