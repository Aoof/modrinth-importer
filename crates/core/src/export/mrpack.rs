//! Writes Modrinth modpack (`.mrpack`) files.
//! Format: <https://docs.modrinth.com/docs/modpacks/format/>

use super::loaders::latest_loader_version;
use crate::events::Reporter;
use crate::model::*;
use crate::modrinth::Modrinth;
use crate::resolve::{Resolution, ResolvedMod};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Put jars that Modrinth doesn't host into `overrides/mods`.
    pub bundle_unmatched: bool,
    /// Also download unmatched mods that only have a URL (e.g. CurseForge CDN) and bundle them.
    pub download_remote: bool,
    /// Override the loader version recorded in the pack.
    pub loader_version: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            bundle_unmatched: true,
            download_remote: true,
            loader_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skipped {
    pub file_name: String,
    pub reason: String,
    /// Where to download the file manually (CurseForge page), when applicable.
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportReport {
    pub output: String,
    pub matched_exact: usize,
    pub matched_fuzzy: usize,
    pub bundled: Vec<String>,
    pub skipped: Vec<Skipped>,
    pub overrides: usize,
}

impl ExportReport {
    pub fn to_markdown(&self) -> String {
        let mut s = format!(
            "# Import report\n\n- Output: `{}`\n- Matched by hash: {}\n- Matched by name: {}\n- Bundled in overrides: {}\n- Skipped: {}\n- Config/override files: {}\n",
            self.output, self.matched_exact, self.matched_fuzzy, self.bundled.len(), self.skipped.len(), self.overrides
        );
        if !self.skipped.is_empty() {
            s.push_str("\n## Skipped mods\n\n");
            for x in &self.skipped {
                s.push_str(&format!("- `{}` — {}", x.file_name, x.reason));
                if let Some(l) = &x.link {
                    s.push_str(&format!(" — [download page]({l})"));
                }
                s.push('\n');
            }
        }
        if !self.bundled.is_empty() {
            s.push_str("\n## Bundled (not on Modrinth)\n\n");
            for x in &self.bundled {
                s.push_str(&format!("- `{x}`\n"));
            }
        }
        s
    }
}

pub async fn write_mrpack(
    pack: &ImportedPack,
    resolved: &[ResolvedMod],
    out: &Path,
    opts: &ExportOptions,
    mr: &Modrinth,
    reporter: &Reporter,
) -> Result<ExportReport> {
    reporter.stage("Building .mrpack");
    let mut report = ExportReport {
        output: out.display().to_string(),
        ..Default::default()
    };

    let mut deps: BTreeMap<String, String> = BTreeMap::new();
    deps.insert("minecraft".into(), pack.mc_version.clone());
    if let Some(key) = pack.loader.mrpack_key() {
        let v = match opts.loader_version.clone().or(pack.loader_version.clone()) {
            Some(v) => v,
            None => latest_loader_version(pack.loader, &pack.mc_version)
                .await?
                .ok_or_else(|| Error::Invalid("no loader version available".into()))?,
        };
        deps.insert(key.into(), v);
    }

    let mut files = vec![];
    let mut bundled: Vec<(String, Vec<u8>)> = vec![];
    let mut used_paths = HashSet::new();
    let total = resolved.len();

    for (n, r) in resolved.iter().enumerate() {
        reporter.progress(n + 1, total);
        let e = &r.entry;
        match &r.resolution {
            Resolution::Matched {
                candidate,
                confidence,
            } => {
                let path = format!("mods/{}", candidate.file.filename);
                if !used_paths.insert(path.clone()) {
                    continue;
                }
                match confidence {
                    crate::resolve::Confidence::Exact => report.matched_exact += 1,
                    crate::resolve::Confidence::Fuzzy => report.matched_fuzzy += 1,
                }
                let mut env = serde_json::Map::new();
                env.insert(
                    "client".into(),
                    json!(if e.client_required {
                        "required"
                    } else {
                        "unsupported"
                    }),
                );
                env.insert(
                    "server".into(),
                    json!(if e.server_required {
                        "required"
                    } else {
                        "unsupported"
                    }),
                );
                files.push(json!({
                    "path": path,
                    "hashes": { "sha1": candidate.file.sha1, "sha512": candidate.file.sha512 },
                    "env": env,
                    "downloads": [candidate.file.url],
                    "fileSize": candidate.file.size,
                }));
            }
            other => {
                let reason = match other {
                    Resolution::Ambiguous { .. } => {
                        "several possible matches; none chosen".to_string()
                    }
                    Resolution::Unmatched { reason } => reason.clone(),
                    Resolution::Matched { .. } => unreachable!(),
                };
                let bytes = if !opts.bundle_unmatched {
                    None
                } else if let Some(o) = &e.origin {
                    Some(o.read()?)
                } else if opts.download_remote {
                    match &e.download_url {
                        Some(u) => mr.download(u).await.ok(),
                        None => None,
                    }
                } else {
                    None
                };
                match bytes {
                    Some(b) => {
                        let name = bundle_name(e, &b);
                        if used_paths.insert(format!("mods/{name}")) {
                            report.bundled.push(name.clone());
                            bundled.push((format!("overrides/mods/{name}"), b));
                        }
                    }
                    None => {
                        // A CurseForge mod we could not fetch: point at the page to download it from.
                        let manual = crate::manual::task(e).filter(|_| e.origin.is_none());
                        let (reason, link) = match manual {
                            Some(t) => (
                                format!("needs a manual download ({reason})"),
                                Some(t.file_url.unwrap_or(t.project_url)),
                            ),
                            None => (reason, None),
                        };
                        report.skipped.push(Skipped {
                            file_name: e.display_name(),
                            reason,
                            link,
                        })
                    }
                }
            }
        }
    }

    let mut index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": pack.version,
        "name": pack.name,
        "files": files,
        "dependencies": deps,
    });

    if let Some(sum) = &pack.summary {
        index["summary"] = json!(sum);
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write to a side file and rename when complete, so a failure or an interrupted run never
    // leaves a truncated archive (no end-of-central-directory record) at the final path.
    let part = {
        let mut n = out.as_os_str().to_owned();
        n.push(".part");
        std::path::PathBuf::from(n)
    };
    let written = write_archive(&part, &index, bundled, pack, &mut report, reporter);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    if out.exists() {
        std::fs::remove_file(out)?;
    }
    if let Err(e) = std::fs::rename(&part, out) {
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    Ok(report)
}

fn write_archive(
    part: &Path,
    index: &serde_json::Value,
    bundled: Vec<(String, Vec<u8>)>,
    pack: &ImportedPack,
    report: &mut ExportReport,
    reporter: &Reporter,
) -> Result<()> {
    reporter.stage("Writing archive");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(part)?);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // Jars are already compressed; deflating them again only burns time.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let total = bundled.len() + pack.overrides.len() + 1;
    let mut done = 1;
    zip.start_file("modrinth.index.json", deflated)?;
    zip.write_all(&serde_json::to_vec_pretty(index)?)?;
    reporter.progress(done, total);
    for (name, bytes) in bundled {
        zip.start_file(name, stored)?;
        zip.write_all(&bytes)?;
        done += 1;
        reporter.progress(done, total);
    }
    let mut seen = HashSet::new();
    for o in &pack.overrides {
        done += 1;
        reporter.progress(done.min(total), total);
        // Never write paths that could escape the instance directory.
        if o.path.starts_with('/')
            || o.path.split('/').any(|c| c == ".." || c.is_empty())
            || !seen.insert(o.path.clone())
        {
            continue;
        }
        let opt = if o.path.ends_with(".jar") {
            stored
        } else {
            deflated
        };
        zip.start_file(format!("overrides/{}", o.path), opt)?;
        zip.write_all(&o.origin.read()?)?;
        report.overrides += 1;
    }
    zip.finish()?;
    Ok(())
}

/// Open a file with the OS default handler (the Modrinth App registers `.mrpack`).
pub fn open_with_default_app(path: &Path) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).status()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(path).status()
    }?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Invalid(
            "could not open the file with the default application".into(),
        ))
    }
}

/// File name for a bundled jar. CurseForge entries whose real name is unknown only have a
/// `cf-<project>-<file>.jar` placeholder; replace it with something descriptive.
fn bundle_name(e: &ModEntry, bytes: &[u8]) -> String {
    match e.cf {
        Some(cf) if e.expected_file_name.is_none() && e.file_name.starts_with("cf-") => {
            let id = crate::providers::jarmeta::inspect(bytes)
                .mod_id
                .unwrap_or_else(|| "mod".into());
            format!("{id}-cf{}.jar", cf.file_id)
        }
        _ => e.file_name.clone(),
    }
}
