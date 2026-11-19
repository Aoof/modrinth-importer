//! `mrimport self-update`: replace this executable with the latest release binary,
//! after verifying its SHA-256 against the release's `SHA256SUMS`.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

const REPO: &str = "Aoof/modrinth-importer";

pub async fn run(check_only: bool) -> Result<()> {
    let target = env!("MRIMPORT_TARGET");
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let asset = format!("mrimport-{target}{ext}");
    let http = reqwest::Client::builder()
        .user_agent(mrimport_core::user_agent())
        .build()?;

    // Releases are pre-releases, which `/releases/latest` skips; take the newest entry.
    let releases: Vec<serde_json::Value> = http
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases?per_page=1"
        ))
        .send()
        .await?
        .error_for_status()
        .context("could not fetch the latest release")?
        .json()
        .await?;
    let rel = releases.into_iter().next().context("no releases found")?;
    let tag = rel["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let current = env!("CARGO_PKG_VERSION");
    if tag == current {
        println!("Already up to date ({current}).");
        return Ok(());
    }
    println!("New version available: {tag} (current {current})");
    if check_only {
        return Ok(());
    }

    let find = |name: &str| {
        rel["assets"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["name"] == name))
            .and_then(|x| x["browser_download_url"].as_str().map(String::from))
    };
    let url = find(&asset).with_context(|| format!("release has no asset named {asset}"))?;
    let sums_url = find("SHA256SUMS").context("release has no SHA256SUMS")?;
    let sums = http
        .get(sums_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = sums
        .lines()
        .find_map(|l| {
            let (h, n) = l.split_once(char::is_whitespace)?;
            (n.trim().trim_start_matches('*') == asset).then(|| h.to_string())
        })
        .context("asset missing from SHA256SUMS")?;

    let bytes = http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        bail!("checksum mismatch for {asset}; refusing to install");
    }

    let exe = std::env::current_exe()?;
    let tmp = exe.with_extension("new");
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) {
        // A running .exe can be renamed but not overwritten.
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
    }
    std::fs::rename(&tmp, &exe)
        .context("could not replace the executable (try with sufficient permissions)")?;
    println!("Updated to {tag}.");
    Ok(())
}
