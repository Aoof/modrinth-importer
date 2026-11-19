//! Match mods from an imported pack to Modrinth versions.
//!
//! 1. **Exact**: batch lookup by SHA-1 / SHA-512 file hash.
//! 2. **Fuzzy**: search by slug / name hint filtered by loader + Minecraft
//!    version. A slug or title match is accepted; anything else is reported as
//!    [`Resolution::Ambiguous`] so the user can choose.

use crate::events::Reporter;
use crate::model::*;
use crate::modrinth::{Modrinth, Version};
use crate::Result;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Exact,
    Fuzzy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    pub url: String,
    pub filename: String,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub file: FileRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Resolution {
    Matched {
        candidate: Candidate,
        confidence: Confidence,
    },
    Ambiguous {
        candidates: Vec<Candidate>,
    },
    Unmatched {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedMod {
    pub entry: ModEntry,
    pub resolution: Resolution,
}

fn file_ref(v: &Version, prefer_sha1: Option<&str>) -> Option<FileRef> {
    let f = prefer_sha1
        .and_then(|h| {
            v.files
                .iter()
                .find(|f| f.hashes.get("sha1").map(String::as_str) == Some(h))
        })
        .or_else(|| v.primary_file())?;
    Some(FileRef {
        url: f.url.clone(),
        filename: f.filename.clone(),
        sha1: f.hashes.get("sha1").cloned().unwrap_or_default(),
        sha512: f.hashes.get("sha512").cloned().unwrap_or_default(),
        size: f.size,
    })
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Pull a searchable name out of a jar file name: `sodium-fabric-0.5.3+mc1.20.1.jar` → `sodium fabric`.
pub fn query_from_filename(file: &str) -> String {
    let stem = file.trim_end_matches(".jar");
    let re = regex::Regex::new(r"[-_ +]v?\d").unwrap();
    let cut = re.find(stem).map(|m| m.start()).unwrap_or(stem.len());
    stem[..cut].replace(['-', '_'], " ").trim().to_string()
}

pub async fn resolve(
    pack: &ImportedPack,
    mr: &Modrinth,
    reporter: &Reporter,
) -> Result<Vec<ResolvedMod>> {
    let total = pack.mods.len();
    let mut results: Vec<Option<Resolution>> = vec![None; total];

    // ---- 1. exact by hash -------------------------------------------------
    reporter.stage("Matching by file hash");
    for (algo, get) in [
        (
            "sha1",
            (|e: &ModEntry| e.sha1.clone()) as fn(&ModEntry) -> Option<String>,
        ),
        ("sha512", |e: &ModEntry| e.sha512.clone()),
    ] {
        let pending: Vec<(usize, String)> = pack
            .mods
            .iter()
            .enumerate()
            .filter(|(i, _)| results[*i].is_none())
            .filter_map(|(i, e)| get(e).map(|h| (i, h)))
            .collect();
        for chunk in pending.chunks(200) {
            let hashes: Vec<String> = chunk.iter().map(|(_, h)| h.clone()).collect();
            let found = mr.versions_from_hashes(&hashes, algo).await?;
            for (i, h) in chunk {
                if let Some(v) = found.get(h) {
                    if let Some(file) = file_ref(v, Some(h)) {
                        results[*i] = Some(Resolution::Matched {
                            candidate: Candidate {
                                project_id: v.project_id.clone(),
                                slug: String::new(),
                                title: v.name.clone(),
                                version_id: v.id.clone(),
                                version_number: v.version_number.clone(),
                                file,
                            },
                            confidence: Confidence::Exact,
                        });
                    }
                }
            }
        }
    }

    // Fill in slug/title for exact matches.
    let ids: Vec<String> = {
        let mut v: Vec<String> = results
            .iter()
            .flatten()
            .filter_map(|r| match r {
                Resolution::Matched { candidate, .. } => Some(candidate.project_id.clone()),
                _ => None,
            })
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let mut info = HashMap::new();
    for chunk in ids.chunks(100) {
        for p in mr.projects(chunk).await? {
            info.insert(p.id.clone(), p);
        }
    }
    for r in results.iter_mut().flatten() {
        if let Resolution::Matched { candidate, .. } = r {
            if let Some(p) = info.get(&candidate.project_id) {
                candidate.slug = p.slug.clone();
                candidate.title = p.title.clone();
            }
        }
    }

    // ---- 2. fuzzy by search ----------------------------------------------
    let loader = pack.loader.modrinth_id();
    let loader_opt = (pack.loader != Loader::Vanilla).then_some(loader);
    let todo: Vec<usize> = (0..total).filter(|i| results[*i].is_none()).collect();
    reporter.stage(format!(
        "Searching Modrinth for {} unmatched mods",
        todo.len()
    ));
    let done = std::sync::atomic::AtomicUsize::new(0);
    let fuzzy: Vec<(usize, Resolution)> = stream::iter(todo)
        .map(|i| {
            let e = &pack.mods[i];
            let done = &done;
            async move {
                let r = fuzzy_one(mr, e, loader_opt, &pack.mc_version)
                    .await
                    .unwrap_or_else(|err| Resolution::Unmatched {
                        reason: format!("lookup failed: {err}"),
                    });
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                reporter.progress(n, total.max(n));
                (i, r)
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;
    for (i, r) in fuzzy {
        results[i] = Some(r);
    }

    Ok(pack
        .mods
        .iter()
        .cloned()
        .zip(results)
        .map(|(entry, r)| ResolvedMod {
            entry,
            resolution: r.unwrap_or(Resolution::Unmatched {
                reason: "not processed".into(),
            }),
        })
        .collect())
}

async fn candidate_for(
    mr: &Modrinth,
    project_id: &str,
    slug: &str,
    title: &str,
    loader: Option<&str>,
    mc: &str,
) -> Result<Option<Candidate>> {
    let mut versions = mr.project_versions(project_id, loader, Some(mc)).await?;
    // Prefer releases, then newest.
    versions.sort_by(|a, b| {
        let rank = |v: &Version| (v.version_type != "release") as u8;
        rank(a)
            .cmp(&rank(b))
            .then(b.date_published.cmp(&a.date_published))
    });
    let Some(v) = versions.first() else {
        return Ok(None);
    };
    let Some(file) = file_ref(v, None) else {
        return Ok(None);
    };
    Ok(Some(Candidate {
        project_id: project_id.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        version_id: v.id.clone(),
        version_number: v.version_number.clone(),
        file,
    }))
}

async fn fuzzy_one(
    mr: &Modrinth,
    e: &ModEntry,
    loader: Option<&str>,
    mc: &str,
) -> Result<Resolution> {
    let mut queries: Vec<String> = vec![];
    for q in [
        e.slug_hint.clone(),
        e.name_hint.clone(),
        Some(query_from_filename(&e.file_name)),
    ]
    .into_iter()
    .flatten()
    {
        if !q.is_empty() && !queries.contains(&q) {
            queries.push(q);
        }
    }
    let mut seen = vec![];
    for q in &queries {
        let hits = mr.search_mods(q, loader, Some(mc), 5).await?;
        // Accept a hit whose slug or title equals one of our hints.
        let hints: Vec<String> = [&e.slug_hint, &e.name_hint]
            .into_iter()
            .flatten()
            .map(|s| norm(s))
            .collect();
        if let Some(h) = hits
            .iter()
            .find(|h| hints.contains(&norm(&h.slug)) || hints.contains(&norm(&h.title)))
        {
            return Ok(
                match candidate_for(mr, &h.project_id, &h.slug, &h.title, loader, mc).await? {
                    Some(candidate) => Resolution::Matched {
                        candidate,
                        confidence: Confidence::Fuzzy,
                    },
                    None => Resolution::Unmatched {
                        reason: format!(
                            "'{}' has no version for this loader/Minecraft version",
                            h.title
                        ),
                    },
                },
            );
        }
        for h in hits {
            if !seen
                .iter()
                .any(|s: &crate::modrinth::SearchHit| s.project_id == h.project_id)
            {
                seen.push(h);
            }
        }
    }
    let mut candidates = vec![];
    for h in seen.iter().take(3) {
        if let Some(c) = candidate_for(mr, &h.project_id, &h.slug, &h.title, loader, mc).await? {
            candidates.push(c);
        }
    }
    Ok(if candidates.is_empty() {
        Resolution::Unmatched {
            reason: "no matching project on Modrinth".into(),
        }
    } else {
        Resolution::Ambiguous { candidates }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filename_queries() {
        assert_eq!(
            query_from_filename("sodium-fabric-0.5.3+mc1.20.1.jar"),
            "sodium fabric"
        );
        assert_eq!(query_from_filename("create-1.20.1-0.5.1.f.jar"), "create");
    }
}
