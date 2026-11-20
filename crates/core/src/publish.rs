//! Publishing: create a Modrinth project and upload versions from a local
//! jar, GitHub releases or a CurseForge project.

use crate::curseforge::CurseForge;
use crate::events::Reporter;
use crate::hash::{sha1_hex, sha512_hex};
use crate::model::Loader;
use crate::modrinth::Modrinth;
use crate::providers::jarmeta;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub title: String,
    pub slug: String,
    /// Short description (max 256 chars).
    pub summary: String,
    /// Long description (Markdown).
    pub body: String,
    pub categories: Vec<String>,
    /// `required` | `optional` | `unsupported`
    pub client_side: String,
    pub server_side: String,
    /// SPDX id, e.g. `MIT`, or `LicenseRef-All-Rights-Reserved`.
    pub license_id: String,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileSource {
    Local { path: PathBuf },
    Url { url: String, filename: String },
}

impl FileSource {
    pub fn filename(&self) -> String {
        match self {
            FileSource::Local { path } => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            FileSource::Url { filename, .. } => filename.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseSpec {
    pub name: String,
    pub version_number: String,
    pub changelog: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// `release` | `beta` | `alpha`
    pub version_type: String,
    pub files: Vec<FileSource>,
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseDefaults {
    pub game_versions: Vec<String>,
    pub loaders: Vec<Loader>,
    pub version_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishPlan {
    pub create_project: bool,
    pub project: ProjectSpec,
    pub releases: Vec<ReleaseSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishReport {
    pub project_id: Option<String>,
    pub created_project: bool,
    pub uploaded: Vec<String>,
    pub skipped: Vec<String>,
    pub dry_run: bool,
}

pub fn validate_project(p: &ProjectSpec) -> Result<()> {
    let slug_ok = regex::Regex::new(r#"^[\w!@$()`.+,"\-']{3,64}$"#)
        .unwrap()
        .is_match(&p.slug);
    if !slug_ok {
        return Err(Error::Invalid(format!(
            "invalid slug '{}': 3-64 chars of letters, digits and - _ . ! @ $ ( ) + ,",
            p.slug
        )));
    }
    if !(3..=64).contains(&p.title.chars().count()) {
        return Err(Error::Invalid("title must be 3-64 characters".into()));
    }
    if p.summary.chars().count() > 256 || p.summary.is_empty() {
        return Err(Error::Invalid("summary must be 1-256 characters".into()));
    }
    for s in [&p.client_side, &p.server_side] {
        if !["required", "optional", "unsupported"].contains(&s.as_str()) {
            return Err(Error::Invalid(format!(
                "side must be required|optional|unsupported, got '{s}'"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- sources

/// One release per jar given on the command line.
pub fn releases_from_jars(
    paths: &[PathBuf],
    d: &ReleaseDefaults,
    version_number: &str,
) -> Result<Vec<ReleaseSpec>> {
    let mut out = vec![];
    for p in paths {
        let bytes = std::fs::read(p)?;
        let meta = jarmeta::inspect(&bytes);
        let loaders: Vec<String> = if d.loaders.is_empty() {
            meta.loader
                .map(|l| vec![l.modrinth_id().to_string()])
                .unwrap_or_default()
        } else {
            d.loaders
                .iter()
                .map(|l| l.modrinth_id().to_string())
                .collect()
        };
        out.push(ReleaseSpec {
            name: format!(
                "{} {}",
                meta.name.clone().unwrap_or_else(|| "Release".into()),
                version_number
            ),
            version_number: version_number.to_string(),
            changelog: String::new(),
            game_versions: d.game_versions.clone(),
            loaders,
            version_type: d.version_type.clone().unwrap_or_else(|| "release".into()),
            files: vec![FileSource::Local { path: p.clone() }],
        });
    }
    Ok(out)
}

fn mc_versions_in(s: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\b1\.\d{1,2}(?:\.\d{1,2})?\b").unwrap();
    let mut v: Vec<String> = re.find_iter(s).map(|m| m.as_str().to_string()).collect();
    v.dedup();
    v
}

/// Releases of a GitHub repository (`owner/repo`), using `.jar` assets.
pub async fn releases_from_github(repo: &str, d: &ReleaseDefaults) -> Result<Vec<ReleaseSpec>> {
    let http = reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .build()?;
    let mut req = http.get(format!(
        "https://api.github.com/repos/{repo}/releases?per_page=100"
    ));
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        req = req.bearer_auth(t);
    }
    let r = req.send().await?;
    if !r.status().is_success() {
        let status = r.status().as_u16();
        let body = r.text().await.unwrap_or_default();
        crate::debuglog::log(format!(
            "GitHub releases -> {status}: {}",
            crate::debuglog::snippet(&body)
        ));
        return Err(Error::api("GitHub", status, body));
    }
    let releases: Vec<serde_json::Value> = r.json().await?;
    let mut out = vec![];
    for rel in releases.iter().rev() {
        if rel["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
        let jars: Vec<FileSource> = rel["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| {
                a["name"]
                    .as_str()
                    .map(|n| n.ends_with(".jar") && !n.contains("-sources") && !n.contains("-dev"))
                    .unwrap_or(false)
            })
            .filter_map(|a| {
                Some(FileSource::Url {
                    url: a["browser_download_url"].as_str()?.into(),
                    filename: a["name"].as_str()?.into(),
                })
            })
            .collect();
        if jars.is_empty() {
            continue;
        }
        let game_versions = if !d.game_versions.is_empty() {
            d.game_versions.clone()
        } else {
            let mut g = vec![];
            for j in &jars {
                g.extend(mc_versions_in(&j.filename()));
            }
            g.extend(mc_versions_in(&tag));
            g.sort();
            g.dedup();
            g
        };
        let loaders: Vec<String> = d
            .loaders
            .iter()
            .map(|l| l.modrinth_id().to_string())
            .collect();
        out.push(ReleaseSpec {
            name: rel["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&tag)
                .to_string(),
            version_number: tag.trim_start_matches('v').to_string(),
            changelog: rel["body"].as_str().unwrap_or("").to_string(),
            game_versions,
            loaders,
            version_type: if rel["prerelease"].as_bool().unwrap_or(false) {
                "beta".into()
            } else {
                d.version_type.clone().unwrap_or_else(|| "release".into())
            },
            files: jars,
        });
    }
    Ok(out)
}

/// Files of a CurseForge project. Metadata (`gameVersions`) carries both MC versions and loader names.
pub async fn releases_from_curseforge(
    cf: &CurseForge,
    mod_id: u64,
    d: &ReleaseDefaults,
) -> Result<(ProjectSpec, Vec<ReleaseSpec>)> {
    let info = cf.mod_info(mod_id).await?;
    let files = cf.mod_files(mod_id).await?;
    let mut releases = vec![];
    for f in files.iter().rev() {
        let Some(url) = &f.download_url else { continue }; // author disabled third-party distribution
        let mut game_versions = vec![];
        let mut loaders = vec![];
        for g in &f.game_versions {
            match Loader::parse(g) {
                Some(l) if l != Loader::Vanilla => loaders.push(l.modrinth_id().to_string()),
                _ if g.starts_with("1.") => game_versions.push(g.clone()),
                _ => {}
            }
        }
        if !d.game_versions.is_empty() {
            game_versions = d.game_versions.clone();
        }
        if !d.loaders.is_empty() {
            loaders = d
                .loaders
                .iter()
                .map(|l| l.modrinth_id().to_string())
                .collect();
        }
        releases.push(ReleaseSpec {
            name: f.display_name.clone(),
            version_number: f.file_name.trim_end_matches(".jar").to_string(),
            changelog: String::new(),
            game_versions,
            loaders,
            version_type: match f.release_type {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            }
            .into(),
            files: vec![FileSource::Url {
                url: url.clone(),
                filename: f.file_name.clone(),
            }],
        });
    }
    let project = ProjectSpec {
        title: info.name,
        slug: info.slug,
        summary: info.summary.chars().take(256).collect(),
        body: format!("{}\n\n_Migrated from CurseForge._", info.summary),
        categories: vec![],
        client_side: "optional".into(),
        server_side: "optional".into(),
        license_id: "LicenseRef-All-Rights-Reserved".into(),
        source_url: None,
        issues_url: None,
    };
    Ok((project, releases))
}

// ---------------------------------------------------------------- publish

async fn load_bytes(mr: &Modrinth, f: &FileSource) -> Result<Vec<u8>> {
    match f {
        FileSource::Local { path } => Ok(tokio::fs::read(path).await?),
        FileSource::Url { url, .. } => mr.download(url).await,
    }
}

/// Execute (or, with `dry_run`, only describe) a publish plan.
pub async fn publish(
    plan: &PublishPlan,
    mr: &Modrinth,
    dry_run: bool,
    reporter: &Reporter,
) -> Result<PublishReport> {
    validate_project(&plan.project)?;
    let mut report = PublishReport {
        dry_run,
        ..Default::default()
    };
    for r in &plan.releases {
        if r.game_versions.is_empty() || r.loaders.is_empty() {
            return Err(Error::Invalid(format!(
                "release '{}' needs Minecraft versions and loaders; fill in both",
                r.version_number
            )));
        }
    }

    if dry_run {
        reporter.stage("Dry run: nothing will be uploaded");
        report.created_project = plan.create_project;
        report.uploaded = plan
            .releases
            .iter()
            .map(|r| r.version_number.clone())
            .collect();
        return Ok(report);
    }
    if !mr.has_token() {
        return Err(Error::MissingCredential(
            "Modrinth personal access token (set MODRINTH_TOKEN or run `mrimport auth modrinth`)",
        ));
    }

    let p = &plan.project;
    let project_id = if plan.create_project {
        reporter.stage("Creating project (as draft)");
        let data = json!({
            "title": p.title, "slug": p.slug, "description": p.summary, "body": p.body,
            "categories": p.categories, "client_side": p.client_side, "server_side": p.server_side,
            "license_id": p.license_id, "project_type": "mod", "is_draft": true,
            "source_url": p.source_url, "issues_url": p.issues_url,
        })
        .to_string();
        let v = mr
            .post_multipart("/project", || {
                reqwest::multipart::Form::new().text("data", data.clone())
            })
            .await?;
        report.created_project = true;
        v["id"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Modrinth did not return a project id".into()))?
            .to_string()
    } else {
        mr.project(&p.slug)
            .await?
            .ok_or_else(|| {
                Error::Invalid(format!("project '{}' does not exist on Modrinth", p.slug))
            })?
            .id
    };
    report.project_id = Some(project_id.clone());

    // Existing versions → skip duplicates by version number or file hash.
    let existing = if plan.create_project {
        vec![]
    } else {
        mr.project_versions(&project_id, None, None).await?
    };
    let mut known_numbers: HashSet<String> =
        existing.iter().map(|v| v.version_number.clone()).collect();
    let mut known_hashes: HashSet<String> = existing
        .iter()
        .flat_map(|v| {
            v.files
                .iter()
                .filter_map(|f| f.hashes.get("sha512").cloned())
        })
        .collect();

    let total = plan.releases.len();
    for (n, r) in plan.releases.iter().enumerate() {
        reporter.progress(n + 1, total);
        if known_numbers.contains(&r.version_number) {
            report
                .skipped
                .push(format!("{} (version number exists)", r.version_number));
            continue;
        }
        let mut blobs = vec![];
        for f in &r.files {
            blobs.push((f.filename(), load_bytes(mr, f).await?));
        }
        if blobs
            .iter()
            .any(|(_, b)| known_hashes.contains(&sha512_hex(b)))
        {
            report
                .skipped
                .push(format!("{} (file already uploaded)", r.version_number));
            continue;
        }
        let parts: Vec<String> = (0..blobs.len()).map(|i| format!("file{i}")).collect();
        let data = json!({
            "project_id": project_id, "name": r.name, "version_number": r.version_number,
            "changelog": r.changelog, "dependencies": [], "game_versions": r.game_versions,
            "version_type": r.version_type, "loaders": r.loaders, "featured": false,
            "file_parts": parts, "primary_file": parts[0], "status": "listed",
        })
        .to_string();
        mr.post_multipart("/version", || {
            let mut form = reqwest::multipart::Form::new().text("data", data.clone());
            for (i, (name, bytes)) in blobs.iter().enumerate() {
                let part = reqwest::multipart::Part::bytes(bytes.clone())
                    .file_name(name.clone())
                    .mime_str("application/java-archive")
                    .expect("static mime");
                form = form.part(format!("file{i}"), part);
            }
            form
        })
        .await?;
        for (_, b) in &blobs {
            known_hashes.insert(sha512_hex(b));
            let _ = sha1_hex(b);
        }
        known_numbers.insert(r.version_number.clone());
        report.uploaded.push(r.version_number.clone());
    }
    Ok(report)
}
