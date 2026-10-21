//! Bounded, read-only discovery of official macOS release assets.
use crate::{origin::command_output, InstallOrigin};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::time::{Duration, Instant};

const REPOSITORY: &str = crate::identity::REPOSITORY;
const API: &str = crate::identity::RELEASE_API;
const DOWNLOAD: &str = crate::identity::RELEASE_DOWNLOAD;

#[derive(Clone, Copy)]
struct Authority {
    repository: &'static str,
    api: &'static str,
    download: &'static str,
    archive_prefix: &'static str,
    beta_tag_prefix: &'static str,
}
const COMPILED_AUTHORITY: Authority = Authority {
    repository: REPOSITORY,
    api: API,
    download: DOWNLOAD,
    archive_prefix: crate::identity::ARCHIVE_PREFIX,
    beta_tag_prefix: if crate::identity::LEGACY {
        "beta/"
    } else {
        "v"
    },
};
const NEUTRAL_AUTHORITY: Authority = Authority {
    repository: crate::identity::neutral::REPOSITORY,
    api: crate::identity::neutral::RELEASE_API,
    download: crate::identity::neutral::RELEASE_DOWNLOAD,
    archive_prefix: crate::identity::neutral::ARCHIVE_PREFIX,
    beta_tag_prefix: "v",
};
const PAGE_SIZE: usize = 50;
const MAX_PAGES: usize = 4;
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    #[default]
    StableOnly,
    StableAndBeta,
}

impl UpdatePolicy {
    pub fn allows_version(self, version: &str) -> Result<bool, String> {
        let version =
            Version::parse(version).map_err(|e| format!("invalid release version: {e}"))?;
        if version.build.is_empty() && version.pre.is_empty() {
            return Ok(true);
        }
        Ok(self == Self::StableAndBeta && official_beta(&version))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Beta,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseCandidate {
    pub version: String,
    pub channel: ReleaseChannel,
    pub tag: String,
    pub release_id: u64,
    pub asset_name: String,
    pub archive_url: String,
    pub archive_size: u64,
    pub sha256: String,
    pub checksum_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTarget {
    pub policy: UpdatePolicy,
    pub release: ReleaseCandidate,
    pub origin: InstallOrigin,
    pub resolved_commit: Option<String>,
}

fn official_beta(version: &Version) -> bool {
    if !version.build.is_empty() {
        return false;
    }
    let pre = version.pre.as_str();
    let Some(n) = pre.strip_prefix("beta.") else {
        return false;
    };
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0'))
}

fn classify(
    version: &str,
    authority: Authority,
) -> Result<(Version, ReleaseChannel, String), String> {
    let parsed = Version::parse(version).map_err(|e| format!("invalid release version: {e}"))?;
    if parsed.to_string() != version || !parsed.build.is_empty() {
        return Err("release version is not canonical".into());
    }
    let (channel, tag) = if parsed.pre.is_empty() {
        (ReleaseChannel::Stable, format!("v{version}"))
    } else if official_beta(&parsed) {
        (
            ReleaseChannel::Beta,
            format!("{}{version}", authority.beta_tag_prefix),
        )
    } else {
        return Err("not an official stable or beta version".into());
    };
    Ok((parsed, channel, tag))
}

fn trusted_url(authority: Authority, url: &str, tag: &str, asset: &str) -> bool {
    url == format!("{}{tag}/{asset}", authority.download)
        || (authority.beta_tag_prefix == "beta/"
            && tag.starts_with("beta/")
            && url
                == format!(
                    "{}{}/{asset}",
                    authority.download,
                    tag.replacen('/', "%2F", 1)
                ))
}

impl ReleaseCandidate {
    /// Check every portable release identity field, including the exact official download paths.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_for(COMPILED_AUTHORITY)
    }

    /// The bridge stages a real neutral helper using fixed neutral release trust,
    /// not its own paired protocol-3 release authority.
    pub fn validate_neutral(&self) -> Result<(), String> {
        self.validate_for(NEUTRAL_AUTHORITY)
    }

    fn validate_for(&self, authority: Authority) -> Result<(), String> {
        let (_, channel, tag) = classify(&self.version, authority)?;
        if self.channel != channel || self.tag != tag || self.release_id == 0 {
            return Err("release channel, tag or id does not match version".into());
        }
        let asset = format!(
            "{}-v{}-macos-arm64.tar.gz",
            authority.archive_prefix, self.version
        );
        if self.asset_name != asset || self.archive_size == 0 {
            return Err("release archive name or size is invalid".into());
        }
        if !trusted_url(authority, &self.archive_url, &tag, &asset)
            || !trusted_url(
                authority,
                &self.checksum_url,
                &tag,
                &format!("{asset}.sha256"),
            )
        {
            return Err("release asset URL is not the official repository/tag/name".into());
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("release archive SHA-256 is invalid".into());
        }
        Ok(())
    }

    /// Verify the downloaded per-archive checksum before using the catalog digest.
    pub fn validate_checksum(&self, checksum: &str) -> Result<(), String> {
        self.validate()?;
        self.validate_checksum_digest(checksum)
    }

    pub fn validate_neutral_checksum(&self, checksum: &str) -> Result<(), String> {
        self.validate_neutral()?;
        self.validate_checksum_digest(checksum)
    }

    fn validate_checksum_digest(&self, checksum: &str) -> Result<(), String> {
        if checksum.len() > 4096 {
            return Err("release checksum exceeds bound".into());
        }
        let mut matches = 0;
        for line in checksum.lines() {
            let Some((digest, name)) = line.split_once(' ') else {
                continue;
            };
            if name.trim_start_matches([' ', '*']) == self.asset_name {
                if !digest.eq_ignore_ascii_case(&self.sha256) {
                    return Err("release checksum conflicts with catalog digest".into());
                }
                matches += 1;
            }
        }
        if matches != 1 {
            return Err("checksum must contain exactly one matching archive digest".into());
        }
        Ok(())
    }
}

fn asset<'a>(
    authority: Authority,
    assets: &'a [Value],
    name: &str,
    tag: &str,
) -> Option<&'a Value> {
    let mut matching = assets
        .iter()
        .filter(|a| a.get("name").and_then(Value::as_str) == Some(name));
    let result = matching.next()?;
    if matching.next().is_some()
        || result.get("state").and_then(Value::as_str) != Some("uploaded")
        || result
            .get("size")
            .and_then(Value::as_u64)
            .filter(|s| *s > 0)
            .is_none()
        || !result
            .get("browser_download_url")
            .and_then(Value::as_str)
            .is_some_and(|url| trusted_url(authority, url, tag, name))
    {
        return None;
    }
    Some(result)
}

/// Parse one official GitHub releases API page; unqualified releases are ignored.
pub fn parse_catalog_page(json: &str) -> Result<Vec<ReleaseCandidate>, String> {
    parse_catalog_for(json, COMPILED_AUTHORITY)
}

pub fn parse_neutral_catalog_page(json: &str) -> Result<Vec<ReleaseCandidate>, String> {
    parse_catalog_for(json, NEUTRAL_AUTHORITY)
}

fn parse_catalog_for(json: &str, authority: Authority) -> Result<Vec<ReleaseCandidate>, String> {
    let api = authority.api;
    let repository = authority.repository;
    if json.len() > MAX_PAGE_BYTES {
        return Err("release catalog page exceeds bound".into());
    }
    let page: Vec<Value> =
        serde_json::from_str(json).map_err(|e| format!("invalid release catalog: {e}"))?;
    if page.len() > PAGE_SIZE {
        return Err("release catalog page exceeds entry bound".into());
    }
    let mut candidates = Vec::new();
    for release in &page {
        if release.get("draft").and_then(Value::as_bool) != Some(false)
            || release
                .get("published_at")
                .and_then(Value::as_str)
                .is_none()
        {
            continue;
        }
        let Some(tag) = release.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let version = tag.strip_prefix('v').or_else(|| tag.strip_prefix("beta/"));
        let Some(version) = version else { continue };
        let Ok((_, channel, expected_tag)) = classify(version, authority) else {
            continue;
        };
        let Some(id) = release
            .get("id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
        else {
            continue;
        };
        if tag != expected_tag
            || release.get("prerelease").and_then(Value::as_bool)
                != Some(channel == ReleaseChannel::Beta)
            || release.get("url").and_then(Value::as_str) != Some(format!("{api}/{id}").as_str())
            || !release
                .get("html_url")
                .and_then(Value::as_str)
                .is_some_and(|url| {
                    url == format!("https://github.com/{repository}/releases/tag/{tag}")
                        || (authority.beta_tag_prefix == "beta/"
                            && channel == ReleaseChannel::Beta
                            && url
                                == format!(
                                    "https://github.com/{repository}/releases/tag/{}",
                                    tag.replace('/', "%2F")
                                ))
                })
        {
            continue;
        }
        let Some(assets) = release
            .get("assets")
            .and_then(Value::as_array)
            .filter(|a| a.len() <= 100)
        else {
            continue;
        };
        let name = format!("{}-v{version}-macos-arm64.tar.gz", authority.archive_prefix);
        let Some(archive) = asset(authority, assets, &name, tag) else {
            continue;
        };
        let Some(checksum) = asset(authority, assets, &format!("{name}.sha256"), tag) else {
            continue;
        };
        if asset(authority, assets, "SHA256SUMS", tag).is_none() {
            continue;
        }
        let Some(sha256) = archive
            .get("digest")
            .and_then(Value::as_str)
            .and_then(|d| d.strip_prefix("sha256:"))
        else {
            continue;
        };
        let candidate = ReleaseCandidate {
            version: version.into(),
            channel,
            tag: tag.into(),
            release_id: id,
            asset_name: name,
            archive_url: archive["browser_download_url"]
                .as_str()
                .unwrap_or_default()
                .into(),
            archive_size: archive["size"].as_u64().unwrap_or_default(),
            sha256: sha256.into(),
            checksum_url: checksum["browser_download_url"]
                .as_str()
                .unwrap_or_default()
                .into(),
        };
        if candidate.validate_for(authority).is_ok() {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

/// Perform an anonymous read-only catalog request. Call this from a background check worker,
/// never from the AppKit main thread. Four pages and one overall deadline bound the operation.
pub fn fetch_candidates() -> Result<Vec<ReleaseCandidate>, String> {
    fetch_for(COMPILED_AUTHORITY)
}

pub fn fetch_neutral_candidates() -> Result<Vec<ReleaseCandidate>, String> {
    fetch_for(NEUTRAL_AUTHORITY)
}

fn fetch_for(authority: Authority) -> Result<Vec<ReleaseCandidate>, String> {
    let api = authority.api;
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut result = Vec::new();
    for page in 1..=MAX_PAGES {
        // Reserve a second for process shutdown inside command_output's own deadline.
        let remaining = deadline.saturating_duration_since(Instant::now()).as_secs();
        let seconds = remaining.saturating_sub(1).min(10);
        if seconds == 0 {
            return Err("release catalog deadline exceeded".into());
        }
        let url = format!("{api}?per_page={PAGE_SIZE}&page={page}");
        let text = command_output(
            Path::new("/usr/bin/curl"),
            &[
                "--disable",
                "--fail",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "5",
                "--max-time",
                &seconds.to_string(),
                "--header",
                "Accept: application/vnd.github+json",
                "--header",
                &format!("User-Agent: {}", crate::identity::HELPER),
                &url,
            ],
            MAX_PAGE_BYTES,
            seconds + 1,
        )?;
        let entries: Vec<Value> =
            serde_json::from_str(&text).map_err(|e| format!("invalid release catalog: {e}"))?;
        let count = entries.len();
        result.extend(parse_catalog_for(&text, authority)?);
        if count < PAGE_SIZE {
            return Ok(result);
        }
    }
    Ok(result)
}

/// Operation B has one approved stable target, independent of this bridge's
/// own release stream. Missing or ambiguous published bytes are not fallback.
pub fn fetch_neutral_migration_candidate() -> Result<ReleaseCandidate, String> {
    select_neutral_migration_candidate(fetch_neutral_candidates()?)
}

fn select_neutral_migration_candidate(
    candidates: Vec<ReleaseCandidate>,
) -> Result<ReleaseCandidate, String> {
    let mut candidates = candidates.into_iter().filter(|candidate| {
        candidate.version == crate::identity::neutral::MIGRATION_TARGET_VERSION
            && candidate.channel == ReleaseChannel::Stable
    });
    let candidate = candidates
        .next()
        .ok_or("approved neutral migration release is unavailable")?;
    if candidates.next().is_some() {
        return Err("approved neutral migration release is ambiguous".into());
    }
    candidate.validate_neutral()?;
    Ok(candidate)
}

/// Select the greatest valid SemVer precedence above the running version; equal-precedence
/// duplicates use stable first, then canonical tag and release id for repeatable ordering.
pub fn select_candidate(
    candidates: &[ReleaseCandidate],
    running_version: &str,
    policy: UpdatePolicy,
) -> Result<Option<ReleaseCandidate>, String> {
    let running =
        Version::parse(running_version).map_err(|e| format!("invalid running version: {e}"))?;
    let mut selected: Option<(&ReleaseCandidate, Version)> = None;
    for candidate in candidates {
        candidate.validate()?;
        if !policy.allows_version(&candidate.version)? {
            continue;
        }
        let version = Version::parse(&candidate.version).map_err(|e| e.to_string())?;
        if version.cmp_precedence(&running) != Ordering::Greater {
            continue;
        }
        if selected.as_ref().is_none_or(|(best, previous)| {
            version.cmp_precedence(previous) == Ordering::Greater
                || (version.cmp_precedence(previous) == Ordering::Equal
                    && (candidate.tag.as_str(), candidate.release_id)
                        < (best.tag.as_str(), best.release_id))
        }) {
            selected = Some((candidate, version));
        }
    }
    Ok(selected.map(|(candidate, _)| candidate.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "a69e52476cc26d14575714dee2b1d1f2513ebd7364a928d3a246e0bfc79dcef4";

    fn fixture_for(authority: Authority, version: &str, tag: &str, prerelease: bool) -> Value {
        let name = format!("{}-v{version}-macos-arm64.tar.gz", authority.archive_prefix);
        let asset = |name: &str, size: u64| {
            json!({
                "name": name, "size": size, "state": "uploaded",
                "browser_download_url": format!("{}{tag}/{name}", authority.download),
                "digest": format!("sha256:{SHA}")
            })
        };
        json!({
            "tag_name": tag, "id": 17, "draft": false,
            "prerelease": prerelease,
            "published_at": "2026-10-09T05:12:01Z",
            "url": format!("{}/17", authority.api),
            "html_url": format!("https://github.com/{}/releases/tag/{tag}", authority.repository),
            "assets": [
                asset(&name, 123), asset(&format!("{name}.sha256"), 108),
                asset("SHA256SUMS", 108)
            ]
        })
    }

    fn parse(releases: &[Value]) -> Vec<ReleaseCandidate> {
        parse_catalog_page(&serde_json::to_string(releases).unwrap()).unwrap()
    }

    #[test]
    fn catalog_requires_complete_uploaded_official_assets() {
        let stable = fixture_for(COMPILED_AUTHORITY, "0.3.3", "v0.3.3", false);
        assert_eq!(parse(&[stable.clone()]).len(), 1);
        let mut cases = Vec::new();
        let mut altered = stable.clone();
        altered["draft"] = json!(true);
        cases.push(altered);
        let mut altered = stable.clone();
        altered["url"] = json!("https://api.github.com/repos/attacker/repo/releases/17");
        cases.push(altered);
        let mut altered = stable.clone();
        altered["assets"][0]["browser_download_url"] = json!(format!("https://github.com.evil.test/{REPOSITORY}/releases/download/v0.3.3/{}-v0.3.3-macos-arm64.tar.gz", crate::identity::ARCHIVE_PREFIX));
        cases.push(altered);
        let mut altered = stable.clone();
        altered["assets"][0]["browser_download_url"] = json!(format!(
            "{DOWNLOAD}v0.3.3/../{}-v0.3.3-macos-arm64.tar.gz",
            crate::identity::ARCHIVE_PREFIX
        ));
        cases.push(altered);
        let mut altered = stable.clone();
        altered["assets"][0]["digest"] = json!("sha256:not-a-digest");
        cases.push(altered);
        let mut altered = stable.clone();
        altered["assets"][0]["state"] = json!("new");
        cases.push(altered);
        let mut altered = stable.clone();
        altered["assets"].as_array_mut().unwrap().pop();
        cases.push(altered);
        let mut altered = stable.clone();
        let duplicate = altered["assets"][0].clone();
        altered["assets"].as_array_mut().unwrap().push(duplicate);
        cases.push(altered);
        let mut altered = stable.clone();
        altered["tag_name"] = json!("v0.3.3+unofficial");
        cases.push(altered);
        let mut altered = stable.clone();
        altered["prerelease"] = json!(true);
        cases.push(altered);
        assert!(parse(&cases).is_empty());
    }

    #[test]
    fn neutral_public_beta2_preserves_catalog_identity_and_checksum() {
        // Semantic fields captured from the anonymous public beta.2 release API.
        let release = json!({
            "id": 416799707, "tag_name": "v0.4.13-beta.2",
            "draft": false, "prerelease": true,
            "published_at": "2026-10-19T00:39:38Z",
            "url": "https://api.github.com/repos/hanbong5938/desktop-pet/releases/416799707",
            "html_url": "https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.13-beta.2",
            "assets": [
                {
                    "name": "DesktopPet-v0.4.13-beta.2-macos-arm64.tar.gz",
                    "size": 24291210, "state": "uploaded",
                    "browser_download_url": "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.13-beta.2/DesktopPet-v0.4.13-beta.2-macos-arm64.tar.gz",
                    "digest": "sha256:2dd87c0670674c351a45f8159d2fcb9f8b32992de79a26ed52966d2818bc1cbf"
                },
                {
                    "name": "DesktopPet-v0.4.13-beta.2-macos-arm64.tar.gz.sha256",
                    "size": 111, "state": "uploaded",
                    "browser_download_url": "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.13-beta.2/DesktopPet-v0.4.13-beta.2-macos-arm64.tar.gz.sha256"
                },
                {
                    "name": "SHA256SUMS", "size": 323, "state": "uploaded",
                    "browser_download_url": "https://github.com/hanbong5938/desktop-pet/releases/download/v0.4.13-beta.2/SHA256SUMS"
                }
            ]
        });
        let candidates = parse_neutral_catalog_page(&json!([release]).to_string()).unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.tag, "v0.4.13-beta.2");
        assert_eq!(candidate.version, "0.4.13-beta.2");
        assert_eq!(candidate.release_id, 416799707);
        assert_eq!(candidate.archive_size, 24291210);
        candidate.validate_neutral().unwrap();
        let checksum = format!("{}  {}", candidate.sha256, candidate.asset_name);
        candidate.validate_neutral_checksum(&checksum).unwrap();
        assert!(candidate
            .validate_neutral_checksum(&format!("{}  {}", "0".repeat(64), candidate.asset_name))
            .is_err());
        assert!(candidate
            .validate_neutral_checksum(&format!("{checksum}\n{checksum}"))
            .is_err());
        if !crate::identity::LEGACY {
            let selected =
                select_candidate(&candidates, "0.4.13-beta.1", UpdatePolicy::StableAndBeta)
                    .unwrap()
                    .unwrap();
            assert_eq!(selected, *candidate);
            selected.validate_checksum(&checksum).unwrap();
            assert!(
                select_candidate(&candidates, "0.4.13-beta.1", UpdatePolicy::StableOnly)
                    .unwrap()
                    .is_none()
            );
            assert!(
                select_candidate(&candidates, "0.4.13-beta.2", UpdatePolicy::StableAndBeta)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn beta_authorities_reject_aliases_and_double_encoded_paths() {
        const LEGACY: Authority = Authority {
            repository: "hanbong5938/herdr-desktop-pet",
            api: "https://api.github.com/repos/hanbong5938/herdr-desktop-pet/releases",
            download: "https://github.com/hanbong5938/herdr-desktop-pet/releases/download/",
            archive_prefix: "HerdrDesktopPet",
            beta_tag_prefix: "beta/",
        };
        for (authority, version, tag, alias) in [
            (
                NEUTRAL_AUTHORITY,
                "0.4.13-beta.2",
                "v0.4.13-beta.2",
                "beta/0.4.13-beta.2",
            ),
            (LEGACY, "0.3.3-beta.1", "beta/0.3.3-beta.1", "v0.3.3-beta.1"),
        ] {
            let parse = |release: Value| {
                parse_catalog_for(&json!([release]).to_string(), authority).unwrap()
            };
            let release = fixture_for(authority, version, tag, true);
            let candidate = parse(release.clone()).pop().unwrap();
            candidate.validate_for(authority).unwrap();
            candidate
                .validate_checksum_digest(&format!("{SHA}  {}", candidate.asset_name))
                .unwrap();
            assert!(candidate
                .validate_checksum_digest(&format!("{}  {}", "0".repeat(64), candidate.asset_name))
                .is_err());
            assert!(parse(fixture_for(authority, version, alias, true)).is_empty());
            let mut alias_candidate = candidate.clone();
            alias_candidate.tag = alias.into();
            assert!(alias_candidate.validate_for(authority).is_err());
            if authority.beta_tag_prefix == "beta/" {
                for encoding in ["%2F", "%252F"] {
                    let mut encoded = release.clone();
                    encoded["html_url"] = json!(format!(
                        "https://github.com/{}/releases/tag/{}",
                        authority.repository,
                        tag.replace('/', encoding)
                    ));
                    for asset in encoded["assets"].as_array_mut().unwrap() {
                        let url = asset["browser_download_url"]
                            .as_str()
                            .unwrap()
                            .replace(tag, &tag.replace('/', encoding));
                        asset["browser_download_url"] = json!(url);
                    }
                    let accepted = parse(encoded);
                    if encoding == "%252F" {
                        assert!(accepted.is_empty());
                    } else {
                        let candidate = accepted.into_iter().next().unwrap();
                        candidate.validate_for(authority).unwrap();
                    }
                }
                for index in 0..3 {
                    for encoding in ["%2F", "%252F"] {
                        let mut altered = release.clone();
                        let name = altered["assets"][index]["name"]
                            .as_str()
                            .unwrap()
                            .to_owned();
                        altered["assets"][index]["browser_download_url"] = json!(format!(
                            "{}{}/{name}",
                            authority.download,
                            tag.replace('/', encoding)
                        ));
                        let accepted = parse(altered);
                        assert_eq!(accepted.len(), usize::from(encoding == "%2F"));
                    }
                }
                for field in ["archive_url", "checksum_url"] {
                    let mut wire = serde_json::to_value(&candidate).unwrap();
                    wire[field] =
                        json!(wire[field].as_str().unwrap().replace("beta/", "beta%252F"));
                    let invalid: ReleaseCandidate = serde_json::from_value(wire).unwrap();
                    assert!(invalid.validate_for(authority).is_err());
                }
            }
            for field in ["archive_url", "checksum_url"] {
                let mut wire = serde_json::to_value(&candidate).unwrap();
                wire[field] = json!(wire[field].as_str().unwrap().replace(tag, alias));
                let invalid: ReleaseCandidate = serde_json::from_value(wire).unwrap();
                assert!(invalid.validate_for(authority).is_err());
            }
        }
        for version in ["0.4.13-rc.1", "0.4.13+build", "0.4.13-beta.01"] {
            let tag = format!("v{version}");
            let release = fixture_for(NEUTRAL_AUTHORITY, version, &tag, true);
            assert!(parse_neutral_catalog_page(&json!([release]).to_string())
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn selection_uses_semver_precedence_without_downgrades() {
        let candidates = parse(&[
            fixture_for(
                COMPILED_AUTHORITY,
                "0.3.3-beta.10",
                if crate::identity::LEGACY {
                    "beta/0.3.3-beta.10"
                } else {
                    "v0.3.3-beta.10"
                },
                true,
            ),
            fixture_for(
                COMPILED_AUTHORITY,
                "0.3.3-beta.2",
                if crate::identity::LEGACY {
                    "beta/0.3.3-beta.2"
                } else {
                    "v0.3.3-beta.2"
                },
                true,
            ),
            fixture_for(COMPILED_AUTHORITY, "0.3.3", "v0.3.3", false),
            fixture_for(
                COMPILED_AUTHORITY,
                "0.4.0-beta.1",
                if crate::identity::LEGACY {
                    "beta/0.4.0-beta.1"
                } else {
                    "v0.4.0-beta.1"
                },
                true,
            ),
        ]);
        let chosen = |running, policy| {
            select_candidate(&candidates, running, policy)
                .unwrap()
                .map(|candidate| candidate.version)
        };
        assert_eq!(
            chosen("0.3.2", UpdatePolicy::StableOnly).as_deref(),
            Some("0.3.3")
        );
        assert_eq!(
            chosen("0.3.2", UpdatePolicy::StableAndBeta).as_deref(),
            Some("0.4.0-beta.1")
        );
        assert_eq!(
            chosen("0.3.3-beta.2", UpdatePolicy::StableOnly).as_deref(),
            Some("0.3.3")
        );
        let only_betas = parse(&[
            fixture_for(
                COMPILED_AUTHORITY,
                "0.3.3-beta.2",
                if crate::identity::LEGACY {
                    "beta/0.3.3-beta.2"
                } else {
                    "v0.3.3-beta.2"
                },
                true,
            ),
            fixture_for(
                COMPILED_AUTHORITY,
                "0.3.3-beta.10",
                if crate::identity::LEGACY {
                    "beta/0.3.3-beta.10"
                } else {
                    "v0.3.3-beta.10"
                },
                true,
            ),
        ]);
        assert_eq!(
            select_candidate(&only_betas, "0.3.2", UpdatePolicy::StableAndBeta)
                .unwrap()
                .unwrap()
                .version,
            "0.3.3-beta.10"
        );
        let same_base = parse(&[
            fixture_for(
                COMPILED_AUTHORITY,
                "0.3.3-beta.10",
                if crate::identity::LEGACY {
                    "beta/0.3.3-beta.10"
                } else {
                    "v0.3.3-beta.10"
                },
                true,
            ),
            fixture_for(COMPILED_AUTHORITY, "0.3.3", "v0.3.3", false),
        ]);
        assert_eq!(
            select_candidate(&same_base, "0.3.2", UpdatePolicy::StableAndBeta)
                .unwrap()
                .unwrap()
                .version,
            "0.3.3"
        );
        assert_eq!(
            chosen("0.4.0-beta.1+local", UpdatePolicy::StableAndBeta),
            None
        );
        assert_eq!(chosen("0.4.0-beta.1", UpdatePolicy::StableOnly), None);
        assert_eq!(
            UpdatePolicy::StableOnly
                .allows_version("0.3.3-beta.2")
                .unwrap(),
            false
        );
        assert_eq!(
            UpdatePolicy::StableAndBeta
                .allows_version("0.3.3-rc.1")
                .unwrap(),
            false
        );
        assert_eq!(
            UpdatePolicy::StableAndBeta
                .allows_version("0.3.3-beta.10")
                .unwrap(),
            true
        );
        assert_eq!(
            UpdatePolicy::StableAndBeta
                .allows_version("0.3.3+build")
                .unwrap(),
            false
        );
    }

    #[test]
    fn bridge_neutral_staging_uses_fixed_neutral_authority() {
        let mut release = fixture_for(NEUTRAL_AUTHORITY, "0.4.12", "v0.4.12", false);
        let page = serde_json::to_string(&vec![release.clone()]).unwrap();
        let candidate = parse_neutral_catalog_page(&page).unwrap().pop().unwrap();
        candidate.validate_neutral().unwrap();
        candidate
            .validate_neutral_checksum(&format!("{SHA}  {}", candidate.asset_name))
            .unwrap();
        if crate::identity::LEGACY {
            assert!(candidate.validate().is_err());
        }
        release["url"] = json!("https://api.github.com/repos/attacker/repo/releases/17");
        assert!(
            parse_neutral_catalog_page(&serde_json::to_string(&vec![release]).unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn beta_product_release_does_not_authorize_a_new_migration_target() {
        let stable = fixture_for(NEUTRAL_AUTHORITY, "0.4.12", "v0.4.12", false);
        let beta = fixture_for(NEUTRAL_AUTHORITY, "0.4.13-beta.1", "v0.4.13-beta.1", true);
        let newer_stable = fixture_for(NEUTRAL_AUTHORITY, "0.4.13", "v0.4.13", false);
        let parse = |releases: Vec<Value>| {
            parse_neutral_catalog_page(&serde_json::to_string(&releases).unwrap()).unwrap()
        };
        let candidates = parse(vec![beta.clone(), newer_stable.clone(), stable.clone()]);
        assert_eq!(candidates.len(), 3);
        let direct = select_candidate(&candidates, "0.4.12", UpdatePolicy::StableAndBeta);
        let beta_only = parse(vec![beta.clone()]);
        let beta_direct = select_candidate(&beta_only, "0.4.12", UpdatePolicy::StableAndBeta);
        if crate::identity::LEGACY {
            assert!(direct.is_err());
            assert!(beta_direct.is_err());
        } else {
            assert_eq!(direct.unwrap().unwrap().version, "0.4.13");
            assert_eq!(beta_direct.unwrap().unwrap().version, "0.4.13-beta.1");
        }
        let target = select_neutral_migration_candidate(candidates).unwrap();
        assert_eq!(target.version, "0.4.12");
        assert_eq!(target.channel, ReleaseChannel::Stable);
        assert!(select_neutral_migration_candidate(parse(vec![beta, newer_stable])).is_err());
        assert!(select_neutral_migration_candidate(parse(vec![stable.clone(), stable])).is_err());
    }
}
