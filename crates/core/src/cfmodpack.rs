//! Importing a modpack straight from CurseForge: a pasted link, a project ID, or a search hit.
//!
//! Needs a CurseForge Core API key. The pack's `.zip` is downloaded (and checked against
//! the SHA-1 CurseForge lists) and then goes through the normal import pipeline.

use crate::curseforge::{CfModpack, CfProjectFile, CurseForge, MODPACK_CLASS_ID};
use crate::events::Reporter;
use crate::{debuglog, Error, Result};
use futures::StreamExt;
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hosts a modpack link may point at.
const HOSTS: &[&str] = &[
    "www.curseforge.com",
    "curseforge.com",
    "legacy.curseforge.com",
];
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// What a pasted link or ID points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModpackRef {
    Slug {
        slug: String,
        file_id: Option<u64>,
    },
    Id {
        project_id: u64,
        file_id: Option<u64>,
    },
}

impl ModpackRef {
    pub fn file_id(&self) -> Option<u64> {
        match self {
            ModpackRef::Slug { file_id, .. } | ModpackRef::Id { file_id, .. } => *file_id,
        }
    }
}

/// True when the text looks like something [`parse_ref`] should be asked about (as opposed
/// to a file path).
pub fn looks_like_ref(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("curseforge://")
        || t.starts_with("cf:")
        || (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

fn num(s: &str) -> Option<u64> {
    s.parse::<u64>().ok().filter(|n| *n > 0)
}

/// Understand a CurseForge modpack link:
///
/// * `https://www.curseforge.com/minecraft/modpacks/<slug>`
/// * `https://www.curseforge.com/minecraft/modpacks/<slug>/files/<fileId>` (also `/download/<id>`)
/// * `https://www.curseforge.com/projects/<projectId>`
/// * `curseforge://install?addonId=<projectId>&fileId=<fileId>` (the CurseForge app)
/// * `cf:<projectId>` or `cf:<projectId>:<fileId>`, or a bare project ID
pub fn parse_ref(input: &str) -> Result<ModpackRef> {
    let t = input
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
    let lower = t.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("cf:") {
        let mut it = rest.split(':');
        let project_id = it
            .next()
            .and_then(num)
            .ok_or_else(|| invalid("expected cf:<projectId> or cf:<projectId>:<fileId>"))?;
        let file_id = match it.next() {
            Some(f) => Some(
                num(f)
                    .ok_or_else(|| invalid("the file id after cf:<projectId>: must be a number"))?,
            ),
            None => None,
        };
        return Ok(ModpackRef::Id {
            project_id,
            file_id,
        });
    }
    if let Some(n) = num(&lower) {
        return Ok(ModpackRef::Id {
            project_id: n,
            file_id: None,
        });
    }

    let url = reqwest::Url::parse(t).map_err(|_| {
        invalid(format!(
            "'{t}' is not a CurseForge modpack link, project id or cf:<id>"
        ))
    })?;

    if url.scheme() == "curseforge" {
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let project_id = q
            .get("addonId")
            .and_then(|v| num(v))
            .ok_or_else(|| invalid("this curseforge:// link has no addonId"))?;
        return Ok(ModpackRef::Id {
            project_id,
            file_id: q.get("fileId").and_then(|v| num(v)),
        });
    }

    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid(
            "only https://www.curseforge.com links are supported",
        ));
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if !HOSTS.contains(&host.as_str()) || url.port().is_some_and(|p| p != 443 && p != 80) {
        return Err(invalid(format!(
            "'{host}' is not curseforge.com; paste a link from www.curseforge.com"
        )));
    }

    let seg: Vec<String> = url
        .path_segments()
        .map(|s| {
            s.filter(|x| !x.is_empty())
                .map(|x| x.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let s: Vec<&str> = seg.iter().map(String::as_str).collect();
    // The id after `files` / `download` (ignoring `files/all` and trailing `/download`).
    let file_after = |from: usize| -> Option<u64> {
        match s.get(from..)? {
            ["files", id, ..] | ["download", id, ..] => num(id),
            _ => None,
        }
    };
    match s.as_slice() {
        ["projects", id, ..] => num(id)
            .map(|project_id| ModpackRef::Id { project_id, file_id: file_after(2) })
            .ok_or_else(|| invalid("that link's project id is not a number")),
        ["minecraft", "modpacks", slug, ..] => Ok(ModpackRef::Slug {
            slug: (*slug).to_string(),
            file_id: file_after(3),
        }),
        ["minecraft", class, ..] => Err(invalid(format!(
            "that link is a '{class}' page, not a modpack. Paste a link that contains /minecraft/modpacks/"
        ))),
        _ => Err(invalid(
            "couldn't find a modpack in that link; it should look like \
             https://www.curseforge.com/minecraft/modpacks/<name>",
        )),
    }
}

/// A project and the exact file chosen for it.
#[derive(Debug, Clone)]
pub struct Selected {
    pub project: CfModpack,
    pub file: CfProjectFile,
}

/// The project's page on CurseForge.
pub fn page_url(p: &CfModpack) -> String {
    p.links
        .as_ref()
        .and_then(|l| l.website_url.clone())
        .filter(|u| u.starts_with("https://www.curseforge.com/"))
        .unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", p.id))
}

pub fn file_page_url(p: &CfModpack, file_id: u64) -> String {
    format!("{}/files/{file_id}", page_url(p).trim_end_matches('/'))
}

/// Prefer releases over betas/alphas, then the newest, never a server pack.
pub fn pick_file(files: &[CfProjectFile], preferred: Option<u64>) -> Option<&CfProjectFile> {
    let client = |f: &&CfProjectFile| f.is_server_pack != Some(true);
    if let Some(id) = preferred {
        if let Some(f) = files.iter().filter(client).find(|f| f.id == id) {
            return Some(f);
        }
    }
    files.iter().filter(client).min_by(|a, b| {
        let rank = |f: &CfProjectFile| (f.release_type != 1) as u8;
        rank(a)
            .cmp(&rank(b))
            .then_with(|| b.file_date.cmp(&a.file_date))
            .then_with(|| b.id.cmp(&a.id))
    })
}

/// Turn a parsed link into a project and a file. Without an explicit file the project's main
/// file is used, falling back to the newest release.
pub async fn resolve(cf: &CurseForge, r: &ModpackRef) -> Result<Selected> {
    let project = match r {
        ModpackRef::Id { project_id, .. } => cf.project(*project_id).await?,
        ModpackRef::Slug { slug, .. } => cf
            .project_by_slug(slug)
            .await?
            .ok_or_else(|| invalid(format!("CurseForge has no project called '{slug}'")))?,
    };
    if project.class_id.is_some_and(|c| c != MODPACK_CLASS_ID) {
        return Err(invalid(format!(
            "'{}' is not a modpack; paste a link to a modpack",
            project.name
        )));
    }
    let file = match r.file_id() {
        Some(fid) => cf.file(project.id, fid).await?,
        None => {
            let mut files = project.latest_files.clone();
            for f in cf.recent_files(project.id, 30).await.unwrap_or_default() {
                if !files.iter().any(|x| x.id == f.id) {
                    files.push(f);
                }
            }
            pick_file(&files, project.main_file_id)
                .cloned()
                .ok_or_else(|| invalid(format!("'{}' has no downloadable files", project.name)))?
        }
    };
    debuglog::log(format!(
        "cfmodpack: '{}' ({}) -> file {} '{}'",
        project.name, project.id, file.id, file.file_name
    ));
    Ok(Selected { project, file })
}

/// Download the pack's `.zip` into `dir`. Errors with [`Error::NeedsManualDownload`] when
/// CurseForge won't give out a download URL, and refuses anything that isn't the exact
/// file CurseForge describes (not a zip, or a SHA-1 mismatch).
pub async fn download(
    cf: &CurseForge,
    sel: &Selected,
    dir: &Path,
    reporter: &Reporter,
) -> Result<PathBuf> {
    let url = match sel.file.download_url.clone() {
        Some(u) => Some(u),
        None => cf.download_url(sel.project.id, sel.file.id).await,
    };
    let Some(url) = url else {
        return Err(Error::NeedsManualDownload {
            name: sel.project.name.clone(),
            url: file_page_url(&sel.project, sel.file.id),
        });
    };
    std::fs::create_dir_all(dir)?;
    reporter.stage(format!("Downloading {}", sel.file.display_name));
    let http = reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .timeout(Duration::from_secs(600))
        .build()?;
    let resp = http.get(&url).send().await?;
    let status = resp.status();
    debuglog::log(format!("cfmodpack GET {url} -> {}", status.as_u16()));
    if !status.is_success() {
        return Err(Error::Api {
            provider: "CurseForge download",
            status: status.as_u16(),
            body: String::new(),
        });
    }
    let total = resp.content_length().or(sel.file.file_length).unwrap_or(0);
    if total > MAX_BYTES {
        return Err(invalid("the modpack file is unreasonably large"));
    }
    let safe: String = sel
        .file
        .file_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.+ ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(safe);
    let mut file = std::fs::File::create(&path)?;
    let mut hasher = Sha1::new();
    let (mut size, mut first) = (0u64, true);
    let mut stream = resp.bytes_stream();
    let cleanup = |p: &Path| {
        let _ = std::fs::remove_file(p);
    };
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if first {
            first = false;
            if chunk.len() < 2 || &chunk[..2] != b"PK" {
                cleanup(&path);
                return Err(invalid("CurseForge did not return a .zip file"));
            }
        }
        size += chunk.len() as u64;
        if size > MAX_BYTES {
            cleanup(&path);
            return Err(invalid("the modpack file is unreasonably large"));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        if total > 0 {
            reporter.progress((size / 1024) as usize, (total / 1024).max(1) as usize);
        }
    }
    if let Some(want) = sel.file.sha1() {
        let got = hex::encode(hasher.finalize());
        if !want.eq_ignore_ascii_case(&got) {
            cleanup(&path);
            return Err(invalid(
                "the downloaded modpack doesn't match the checksum CurseForge lists; not using it",
            ));
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(s: &str, f: Option<u64>) -> ModpackRef {
        ModpackRef::Slug {
            slug: s.into(),
            file_id: f,
        }
    }
    fn id(p: u64, f: Option<u64>) -> ModpackRef {
        ModpackRef::Id {
            project_id: p,
            file_id: f,
        }
    }

    #[test]
    fn accepts_the_link_shapes_people_paste() {
        let base = "https://www.curseforge.com/minecraft/modpacks/all-the-mods-10";
        assert_eq!(parse_ref(base).unwrap(), slug("all-the-mods-10", None));
        assert_eq!(
            parse_ref(&format!("{base}/")).unwrap(),
            slug("all-the-mods-10", None)
        );
        assert_eq!(
            parse_ref(&format!("{base}/files")).unwrap(),
            slug("all-the-mods-10", None)
        );
        assert_eq!(
            parse_ref(&format!("{base}/files/all?page=2")).unwrap(),
            slug("all-the-mods-10", None)
        );
        assert_eq!(
            parse_ref(&format!("{base}/files/6000123")).unwrap(),
            slug("all-the-mods-10", Some(6000123))
        );
        assert_eq!(
            parse_ref(&format!("{base}/files/6000123/download")).unwrap(),
            slug("all-the-mods-10", Some(6000123))
        );
        assert_eq!(
            parse_ref(&format!("{base}/download/6000123")).unwrap(),
            slug("all-the-mods-10", Some(6000123))
        );
        assert_eq!(
            parse_ref(&format!("  <{base}?utm_source=x#readme>  ")).unwrap(),
            slug("all-the-mods-10", None)
        );
        assert_eq!(
            parse_ref("HTTPS://WWW.CURSEFORGE.COM/Minecraft/Modpacks/All-The-Mods-10").unwrap(),
            slug("all-the-mods-10", None)
        );
        assert_eq!(
            parse_ref("https://legacy.curseforge.com/minecraft/modpacks/x").unwrap(),
            slug("x", None)
        );
        assert_eq!(
            parse_ref("https://curseforge.com/minecraft/modpacks/x").unwrap(),
            slug("x", None)
        );
    }

    #[test]
    fn accepts_ids_and_the_app_protocol() {
        assert_eq!(
            parse_ref("https://www.curseforge.com/projects/925200").unwrap(),
            id(925200, None)
        );
        assert_eq!(
            parse_ref("https://www.curseforge.com/projects/925200/files/1").unwrap(),
            id(925200, Some(1))
        );
        assert_eq!(parse_ref("925200").unwrap(), id(925200, None));
        assert_eq!(parse_ref("cf:925200").unwrap(), id(925200, None));
        assert_eq!(
            parse_ref("CF:925200:6000123").unwrap(),
            id(925200, Some(6000123))
        );
        assert_eq!(
            parse_ref("curseforge://install?addonId=925200&fileId=6000123").unwrap(),
            id(925200, Some(6000123))
        );
        assert_eq!(
            parse_ref("curseforge://install?addonId=925200").unwrap(),
            id(925200, None)
        );
    }

    #[test]
    fn rejects_everything_that_is_not_a_curseforge_modpack() {
        for bad in [
            "",
            "hello",
            "cf:",
            "cf:abc",
            "cf:1:xyz",
            "0",
            "https://evil.example/minecraft/modpacks/x",
            "https://www.curseforge.com.evil.example/minecraft/modpacks/x",
            "https://evil.example/www.curseforge.com/minecraft/modpacks/x",
            "https://user@evil.example/minecraft/modpacks/x",
            "https://www.curseforge.com:8443/minecraft/modpacks/x",
            "ftp://www.curseforge.com/minecraft/modpacks/x",
            "https://www.curseforge.com/",
            "https://www.curseforge.com/minecraft",
            "https://www.curseforge.com/projects/notanumber",
            "curseforge://install?fileId=5",
            "file:///etc/passwd",
        ] {
            assert!(parse_ref(bad).is_err(), "should reject {bad:?}");
        }
        let e = parse_ref("https://www.curseforge.com/minecraft/mc-mods/jei")
            .unwrap_err()
            .to_string();
        assert!(e.contains("mc-mods") && e.contains("not a modpack"), "{e}");
    }

    #[test]
    fn tells_links_from_paths() {
        assert!(looks_like_ref("https://www.curseforge.com/x"));
        assert!(looks_like_ref("curseforge://install?addonId=1"));
        assert!(looks_like_ref("cf:5"));
        assert!(looks_like_ref("12345"));
        assert!(!looks_like_ref("pack.zip"));
        assert!(!looks_like_ref("/Users/me/Downloads/pack.zip"));
        assert!(!looks_like_ref(""));
    }

    fn file(id: u64, release: u8, date: &str, server: Option<bool>) -> CfProjectFile {
        CfProjectFile {
            id,
            file_name: format!("f{id}.zip"),
            display_name: format!("f{id}"),
            download_url: None,
            release_type: release,
            game_versions: vec![],
            file_date: Some(date.into()),
            file_length: None,
            hashes: vec![],
            is_server_pack: server,
        }
    }

    #[test]
    fn picks_the_main_file_else_the_newest_release_never_a_server_pack() {
        let files = vec![
            file(1, 1, "2024-01-01", None),
            file(2, 2, "2024-06-01", None),       // newer but a beta
            file(3, 1, "2024-03-01", None),       // newest release
            file(4, 1, "2024-09-01", Some(true)), // newest of all, but a server pack
        ];
        assert_eq!(pick_file(&files, None).unwrap().id, 3);
        assert_eq!(
            pick_file(&files, Some(2)).unwrap().id,
            2,
            "main file wins when present"
        );
        assert_eq!(
            pick_file(&files, Some(4)).unwrap().id,
            3,
            "a server pack is never chosen"
        );
        assert_eq!(
            pick_file(&files, Some(99)).unwrap().id,
            3,
            "unknown main file falls back"
        );
        assert!(pick_file(&[file(5, 1, "x", Some(true))], None).is_none());
        // only betas: still picks the newest
        let betas = vec![
            file(7, 2, "2024-01-01", None),
            file(8, 2, "2024-02-01", None),
        ];
        assert_eq!(pick_file(&betas, None).unwrap().id, 8);
    }
}
