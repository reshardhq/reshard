use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use owo_colors::OwoColorize;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// Check for or install a GitHub release. Returns true only when the running
/// executable was replaced.
pub async fn run(requested: Option<&str>, repo: &str, check: bool) -> Result<bool> {
    validate_repo(repo)?;
    let target = release_target()?;
    let client = reqwest::Client::builder()
        .user_agent(format!("rebeam/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let tag = match requested {
        Some(version) => normalize_version(version)?,
        None => latest_tag(&client, repo).await?,
    };
    let current = env!("CARGO_PKG_VERSION");

    if tag.trim_start_matches('v') == current {
        println!("{} rebeam {current} is current", "✓".green());
        return Ok(false);
    }

    if check {
        println!(
            "{} rebeam {} is available (installed: {current})",
            "↑".cyan(),
            tag.trim_start_matches('v')
        );
        return Ok(false);
    }

    let artifact = format!("rebeam-{target}.tar.gz");
    let base = format!("https://github.com/{repo}/releases/download/{tag}");
    let checksums = download_text(&client, &format!("{base}/checksums.txt")).await?;
    let expected = checksum_for(&checksums, &artifact)
        .with_context(|| format!("{artifact} is missing from checksums.txt"))?;
    let archive = download_bytes(&client, &format!("{base}/{artifact}")).await?;
    let actual = format!("{:x}", Sha256::digest(&archive));
    if actual != expected.to_ascii_lowercase() {
        bail!("checksum verification failed for {artifact}");
    }

    let temp = TempDir::new()?;
    let decoder = GzDecoder::new(archive.as_slice());
    tar::Archive::new(decoder)
        .unpack(temp.path())
        .context("extracting the Rebeam release")?;
    let downloaded = temp.path().join("rebeam");
    if !downloaded.is_file() {
        bail!("release archive does not contain the rebeam binary");
    }

    replace_current_exe(&downloaded)?;
    println!(
        "{} updated rebeam {current} → {}",
        "✓".green(),
        tag.trim_start_matches('v')
    );
    Ok(true)
}

async fn latest_tag(client: &reqwest::Client, repo: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("checking the latest release for {repo}"))?
        .error_for_status()
        .with_context(|| format!("GitHub has no published release for {repo}"))?;
    Ok(response.json::<Release>().await?.tag_name)
}

async fn download_text(client: &reqwest::Client, url: &str) -> Result<String> {
    Ok(client
        .get(url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("release asset is unavailable: {url}"))?
        .text()
        .await?)
}

async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(client
        .get(url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("release asset is unavailable: {url}"))?
        .bytes()
        .await?
        .to_vec())
}

fn validate_repo(repo: &str) -> Result<()> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    let valid = |value: &str| {
        !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !valid(owner) || !valid(name) || parts.next().is_some() {
        bail!("invalid GitHub repository {repo:?}; expected owner/name");
    }
    Ok(())
}

fn normalize_version(version: &str) -> Result<String> {
    let version = version.trim();
    let bare = version.strip_prefix('v').unwrap_or(version);
    if bare.is_empty()
        || !bare
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        bail!("invalid release version {version:?}");
    }
    Ok(format!("v{bare}"))
}

fn release_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        (os, arch) => bail!("Rebeam updates are not published for {os}/{arch}"),
    }
}

fn checksum_for<'a>(document: &'a str, artifact: &str) -> Option<&'a str> {
    document.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == artifact && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then_some(hash)
    })
}

#[cfg(unix)]
fn replace_current_exe(downloaded: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let current = std::env::current_exe().context("locating the installed Rebeam binary")?;
    let parent = current
        .parent()
        .context("the installed Rebeam binary has no parent directory")?;
    let candidate = parent.join(format!(".rebeam-update-{}", std::process::id()));
    std::fs::copy(downloaded, &candidate)
        .with_context(|| format!("writing update beside {}", current.display()))?;
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755))?;
    if let Err(error) = std::fs::rename(&candidate, &current) {
        let _ = std::fs::remove_file(&candidate);
        return Err(error).with_context(|| format!("replacing {}", current.display()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn replace_current_exe(_: &Path) -> Result<()> {
    bail!("self-update is currently supported on macOS and Linux")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "rebeam-update-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gnu_and_bsd_checksum_lines() {
        let hash = "a".repeat(64);
        let document = format!(
            "{hash}  rebeam-aarch64-apple-darwin.tar.gz\n{hash} *rebeam-x86_64-unknown-linux-gnu.tar.gz\n"
        );
        assert_eq!(
            checksum_for(&document, "rebeam-aarch64-apple-darwin.tar.gz"),
            Some(hash.as_str())
        );
        assert_eq!(
            checksum_for(&document, "rebeam-x86_64-unknown-linux-gnu.tar.gz"),
            Some(hash.as_str())
        );
    }

    #[test]
    fn rejects_unsafe_release_inputs() {
        assert!(validate_repo("T31K/rebeam").is_ok());
        assert!(validate_repo("https://github.com/T31K/rebeam").is_err());
        assert!(normalize_version("0.2.0").is_ok());
        assert!(normalize_version("../../main").is_err());
    }
}
