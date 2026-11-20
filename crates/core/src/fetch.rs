//! Downloading the mods that are not an exact match on Modrinth.
//!
//! A modpack is assumed not to exist on Modrinth: only exact (hash) matches are used from
//! there. Every other mod is downloaded from its original source (the CurseForge download
//! URL, or the cursemaven mirror in keyless mode) so it can be bundled in the pack. What
//! cannot be downloaded stays unresolved and is handed to [`crate::manual`].

use crate::events::Reporter;
use crate::model::{ModEntry, Origin};
use crate::modrinth::Modrinth;
use crate::resolve::{exact_by_hash, Resolution, ResolvedMod};
use crate::{debuglog, Result};
use futures::stream::{self, StreamExt};
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_BYTES: u64 = 250 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct FetchReport {
    pub fetched: usize,
    /// Of those, how many turned out to be exact matches on Modrinth after all.
    pub found_on_modrinth: usize,
    /// `(index, reason)` for downloads that failed or didn't verify.
    pub failed: Vec<(usize, String)>,
}

struct Downloaded {
    path: PathBuf,
    sha1: String,
    sha512: String,
    size: u64,
}

fn is_candidate(r: &ResolvedMod) -> bool {
    !matches!(r.resolution, Resolution::Matched { .. })
        && r.entry.origin.is_none()
        && r.entry.download_url.is_some()
}

/// Download every unmatched mod that has a download URL into `dir` and point its `origin` at
/// the file. A SHA-1 known in advance is verified. On any problem the mod's download URL is
/// cleared so it shows up as needing a manual file.
pub async fn fetch_unmatched(
    resolved: &mut [ResolvedMod],
    dir: &Path,
    mr: &Modrinth,
    reporter: &Reporter,
) -> Result<FetchReport> {
    let todo: Vec<usize> = resolved
        .iter()
        .enumerate()
        .filter(|(_, r)| is_candidate(r))
        .map(|(i, _)| i)
        .collect();
    let mut report = FetchReport::default();
    if todo.is_empty() {
        return Ok(report);
    }
    std::fs::create_dir_all(dir)?;
    reporter.stage(format!(
        "Downloading {} mods that aren't on Modrinth",
        todo.len()
    ));
    let http = reqwest::Client::builder()
        .user_agent(crate::user_agent())
        .timeout(Duration::from_secs(300))
        .build()?;
    let total = todo.len();
    let done = std::sync::atomic::AtomicUsize::new(0);

    let results: Vec<(usize, std::result::Result<Downloaded, String>)> = stream::iter(todo)
        .map(|i| {
            let e = &resolved[i].entry;
            let (http, done) = (&http, &done);
            async move {
                let r = download_one(http, e, dir, i).await;
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                reporter.progress(n, total);
                (i, r)
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let mut newly_hashed: Vec<usize> = vec![];
    for (i, r) in results {
        let item = &mut resolved[i];
        match r {
            Ok(d) => {
                if item.entry.sha1.is_none() {
                    newly_hashed.push(i);
                }
                item.entry.sha1 = Some(d.sha1);
                item.entry.sha512 = Some(d.sha512);
                item.entry.size = Some(d.size);
                item.entry.origin = Some(Origin::File { path: d.path });
                report.fetched += 1;
            }
            Err(msg) => {
                debuglog::log(format!(
                    "fetch: {} failed: {msg}",
                    item.entry.display_name()
                ));
                item.entry.download_url = None;
                item.resolution = Resolution::Unmatched {
                    reason: format!("download failed: {msg}"),
                };
                report.failed.push((i, msg));
            }
        }
    }

    // A file we only just learned the hash of may be on Modrinth after all (exact match).
    if !newly_hashed.is_empty() {
        let entries: Vec<ModEntry> = newly_hashed
            .iter()
            .map(|i| resolved[*i].entry.clone())
            .collect();
        if let Ok(found) = exact_by_hash(mr, &entries).await {
            for (k, i) in newly_hashed.iter().enumerate() {
                if let Some(r) = found[k].clone() {
                    resolved[*i].resolution = r;
                    report.found_on_modrinth += 1;
                }
            }
        }
    }
    Ok(report)
}

async fn download_one(
    http: &reqwest::Client,
    e: &ModEntry,
    dir: &Path,
    index: usize,
) -> std::result::Result<Downloaded, String> {
    let url = e.download_url.clone().unwrap_or_default();
    let resp = http.get(&url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    debuglog::log(format!("fetch GET {url} -> {}", status.as_u16()));
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Err("file is too large".into());
    }
    let safe: String = e
        .file_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{index}-{safe}"));
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let (mut h1, mut h512) = (Sha1::new(), Sha512::new());
    let (mut size, mut first) = (0u64, true);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if first {
            first = false;
            if chunk.len() < 2 || &chunk[..2] != b"PK" {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err("the server did not return a jar file".into());
            }
        }
        size += chunk.len() as u64;
        if size > MAX_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err("file is too large".into());
        }
        h1.update(&chunk);
        h512.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    let sha1 = hex::encode(h1.finalize());
    if let Some(want) = &e.sha1 {
        if !want.eq_ignore_ascii_case(&sha1) {
            let _ = std::fs::remove_file(&path);
            return Err(
                "checksum mismatch: the download is not the exact file the pack lists".into(),
            );
        }
    }
    Ok(Downloaded {
        path,
        sha1,
        sha512: hex::encode(h512.finalize()),
        size,
    })
}
