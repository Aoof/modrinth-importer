//! Turning CurseForge `(projectID, fileID)` pairs into real file data.
//!
//! A CurseForge modpack manifest only lists numeric IDs. Lookup chain:
//!
//! 1. **Core API** (needs an API key): exact file name, SHA-1, download URL, links.
//! 2. **cursemaven.com** (opt-in, keyless): the jar is downloaded and hashed locally,
//!    so it can still be matched to Modrinth *exactly* by hash.
//! 3. **api.cfwidget.com** (opt-in, keyless, only for what is still unresolved):
//!    project name, slug, page link and the file's exact name.
//!
//! Whatever is still unresolved after this is handed to [`crate::manual`].

use crate::curseforge::CurseForge;
use crate::events::Reporter;
use crate::hash::{sha1_hex, sha512_hex};
use crate::model::{CfRef, LookupSource, ModEntry};
use crate::{debuglog, Error, Result};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const CURSEMAVEN_BASE: &str = "https://cursemaven.com";
pub const CFWIDGET_BASE: &str = "https://api.cfwidget.com";

/// Largest jar we are willing to download for hashing.
const MAX_JAR_BYTES: u64 = 250 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct LookupBases {
    pub cursemaven: String,
    pub cfwidget: String,
}

impl Default for LookupBases {
    fn default() -> Self {
        Self {
            cursemaven: CURSEMAVEN_BASE.into(),
            cfwidget: CFWIDGET_BASE.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfInfo {
    pub project_name: Option<String>,
    pub slug: Option<String>,
    pub project_url: Option<String>,
    pub file_name: Option<String>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: Option<u64>,
    pub download_url: Option<String>,
    pub sources: Vec<LookupSource>,
}

#[derive(Clone)]
pub struct Lookup {
    pub core: Option<CurseForge>,
    pub keyless: bool,
    pub bases: LookupBases,
    pub cache_dir: Option<PathBuf>,
    http: reqwest::Client,
}

#[derive(Debug, Default)]
pub struct LookupOutcome {
    pub infos: HashMap<CfRef, CfInfo>,
    /// Non-fatal problems worth showing to the user.
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CachedHash {
    sha1: String,
    sha512: String,
    size: u64,
}

impl Lookup {
    pub fn new(core: Option<CurseForge>, keyless: bool) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::user_agent())
            .timeout(Duration::from_secs(120))
            .build()
            .expect("http client");
        Self {
            core,
            keyless,
            bases: LookupBases::default(),
            cache_dir: None,
            http,
        }
    }

    pub fn with_bases(mut self, bases: LookupBases) -> Self {
        self.bases = bases;
        self
    }

    pub fn with_cache_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.cache_dir = dir;
        self
    }

    /// Resolve every ref as far as the enabled sources allow.
    pub async fn files(&self, refs: &[CfRef], reporter: &Reporter) -> Result<LookupOutcome> {
        let mut out = LookupOutcome::default();

        // ---- 1. Core API ------------------------------------------------
        match &self.core {
            Some(cf) => {
                reporter.stage("Looking up files on CurseForge");
                match core_lookup(cf, refs).await {
                    Ok(m) => out.infos = m,
                    Err(e) if self.keyless => {
                        let w = format!(
                            "The CurseForge API did not accept the request ({e}); \
                             falling back to keyless lookup."
                        );
                        debuglog::log(format!("lookup: {w}"));
                        out.warnings.push(w);
                    }
                    Err(e) => return Err(e),
                }
            }
            None if !self.keyless => return Err(Error::MissingCredential(
                "CurseForge API key (set CURSEFORGE_API_KEY or run `mrimport auth curseforge`), \
                     or enable keyless lookup (experimental) in Settings or with --keyless",
            )),
            None => {}
        }

        // ---- 2. cursemaven: hash the jar ---------------------------------
        if self.keyless {
            let need: Vec<CfRef> = refs
                .iter()
                .filter(|r| out.infos.get(r).and_then(|i| i.sha1.as_ref()).is_none())
                .copied()
                .collect();
            if !need.is_empty() {
                reporter.stage(format!(
                    "Keyless lookup: hashing {} mods via cursemaven.com",
                    need.len()
                ));
                let mut cache = self.load_cache();
                let total = need.len();
                let done = std::sync::atomic::AtomicUsize::new(0);
                let results: Vec<(CfRef, Option<CachedHash>, bool)> = stream::iter(need)
                    .map(|r| {
                        let cached = cache.get(&cache_key(r)).cloned();
                        let done = &done;
                        async move {
                            let (h, fetched) = match cached {
                                Some(h) => (Some(h), false),
                                None => (self.fetch_hash(r).await, true),
                            };
                            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            reporter.progress(n, total);
                            (r, h, fetched)
                        }
                    })
                    .buffer_unordered(4)
                    .collect()
                    .await;
                for (r, h, fetched) in results {
                    let Some(h) = h else { continue };
                    if fetched {
                        cache.insert(cache_key(r), h.clone());
                    }
                    let i = out.infos.entry(r).or_default();
                    i.sha1 = Some(h.sha1);
                    i.sha512 = Some(h.sha512);
                    i.size = Some(h.size);
                    i.download_url = i.download_url.take().or_else(|| Some(self.maven_url(r)));
                    i.sources.push(LookupSource::CurseMaven);
                }
                self.save_cache(&cache);
            }
        }
        Ok(out)
    }

    /// Fill in names, slug, page link and exact file name for entries that lack them,
    /// using cfwidget. A no-op unless keyless lookup is enabled.
    pub async fn enrich(&self, entries: &mut [ModEntry], reporter: &Reporter) {
        if !self.keyless {
            return;
        }
        let mut by_project: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            if let Some(cf) = e.cf {
                if e.expected_file_name.is_none() || e.name_hint.is_none() || e.cf_url.is_none() {
                    by_project.entry(cf.project_id).or_default().push(i);
                }
            }
        }
        if by_project.is_empty() {
            return;
        }
        reporter.stage(format!(
            "Keyless lookup: reading {} project pages via cfwidget",
            by_project.len()
        ));
        let total = by_project.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let fetched: Vec<(u64, Option<serde_json::Value>)> =
            stream::iter(by_project.keys().copied())
                .map(|pid| {
                    let done = &done;
                    async move {
                        let v = self.fetch_widget(pid).await;
                        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        reporter.progress(n, total);
                        (pid, v)
                    }
                })
                .buffer_unordered(4)
                .collect()
                .await;

        for (pid, widget) in fetched {
            let Some(w) = widget else { continue };
            let url = w["urls"]["curseforge"].as_str().map(String::from);
            let slug = url
                .as_deref()
                .and_then(|u| u.trim_end_matches('/').rsplit('/').next())
                .map(String::from);
            let title = w["title"].as_str().map(String::from);
            let files = w["files"].as_array();
            for &i in &by_project[&pid] {
                let e = &mut entries[i];
                let mut touched = false;
                if e.name_hint.is_none() {
                    e.name_hint = title.clone();
                    touched |= title.is_some();
                }
                if e.slug_hint.is_none() {
                    e.slug_hint = slug.clone();
                    touched |= slug.is_some();
                }
                if e.cf_url.is_none() {
                    e.cf_url = url.clone();
                    touched |= url.is_some();
                }
                if e.expected_file_name.is_none() {
                    let fid = e.cf.map(|c| c.file_id).unwrap_or_default();
                    let name = files.and_then(|fs| {
                        fs.iter()
                            .find(|f| f["id"].as_u64() == Some(fid))
                            .and_then(|f| f["name"].as_str())
                    });
                    if let Some(n) = name {
                        e.expected_file_name = Some(n.to_string());
                        e.file_name = n.to_string();
                        touched = true;
                    }
                }
                if touched && !e.lookup_sources.contains(&LookupSource::CfWidget) {
                    e.lookup_sources.push(LookupSource::CfWidget);
                }
            }
        }
    }

    pub fn maven_url(&self, r: CfRef) -> String {
        format!(
            "{}/curse/maven/x-{p}/{f}/x-{p}-{f}.jar",
            self.bases.cursemaven.trim_end_matches('/'),
            p = r.project_id,
            f = r.file_id
        )
    }

    async fn fetch_hash(&self, r: CfRef) -> Option<CachedHash> {
        let url = self.maven_url(r);
        let resp = match self.http.get(&url).send().await {
            Ok(x) => x,
            Err(e) => {
                debuglog::log(format!("cursemaven GET {url} -> request failed: {e}"));
                return None;
            }
        };
        let status = resp.status();
        debuglog::log(format!(
            "cursemaven GET {url} -> {} (content-length {})",
            status.as_u16(),
            resp.content_length()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".into())
        ));
        if !status.is_success() {
            return None;
        }
        if resp.content_length().is_some_and(|n| n > MAX_JAR_BYTES) {
            debuglog::log("cursemaven: file too large to hash, skipping");
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        // A jar is a zip archive; anything else (an HTML error page served with 200) is rejected.
        if bytes.len() < 4 || &bytes[..2] != b"PK" {
            debuglog::log("cursemaven: response is not a jar/zip file, ignoring");
            return None;
        }
        Some(CachedHash {
            sha1: sha1_hex(&bytes),
            sha512: sha512_hex(&bytes),
            size: bytes.len() as u64,
        })
    }

    async fn fetch_widget(&self, pid: u64) -> Option<serde_json::Value> {
        let url = format!("{}/{pid}", self.bases.cfwidget.trim_end_matches('/'));
        for attempt in 0..4 {
            let resp = match self.http.get(&url).send().await {
                Ok(x) => x,
                Err(e) => {
                    debuglog::log(format!("cfwidget GET {url} -> request failed: {e}"));
                    return None;
                }
            };
            let status = resp.status();
            debuglog::log(format!("cfwidget GET {url} -> {}", status.as_u16()));
            // 202: the service is still fetching this project; try again shortly.
            if status.as_u16() == 202 && attempt < 3 {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            if !status.is_success() || status.as_u16() == 202 {
                return None;
            }
            return resp.json().await.ok();
        }
        None
    }

    fn cache_file(&self) -> Option<PathBuf> {
        self.cache_dir.as_ref().map(|d| d.join("cf-hashes.json"))
    }

    fn load_cache(&self) -> HashMap<String, CachedHash> {
        self.cache_file()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_cache(&self, cache: &HashMap<String, CachedHash>) {
        let Some(p) = self.cache_file() else { return };
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(b) = serde_json::to_vec(cache) {
            let _ = std::fs::write(p, b);
        }
    }
}

fn cache_key(r: CfRef) -> String {
    format!("{}:{}", r.project_id, r.file_id)
}

async fn core_lookup(cf: &CurseForge, refs: &[CfRef]) -> Result<HashMap<CfRef, CfInfo>> {
    let file_ids: Vec<u64> = refs.iter().map(|r| r.file_id).collect();
    let mut files = HashMap::new();
    for chunk in file_ids.chunks(400) {
        for f in cf.files(chunk).await? {
            files.insert(f.id, f);
        }
    }
    let mut project_ids: Vec<u64> = refs.iter().map(|r| r.project_id).collect();
    project_ids.sort_unstable();
    project_ids.dedup();
    let mut mods = HashMap::new();
    for chunk in project_ids.chunks(400) {
        for m in cf.mods(chunk).await? {
            mods.insert(m.id, m);
        }
    }
    let mut out = HashMap::new();
    for r in refs {
        let file = files.get(&r.file_id);
        let m = mods.get(&r.project_id);
        let mut i = CfInfo {
            project_name: m.map(|m| m.name.clone()),
            slug: m.map(|m| m.slug.clone()),
            project_url: m.and_then(|m| m.links.as_ref().and_then(|l| l.website_url.clone())),
            ..Default::default()
        };
        if let Some(f) = file {
            i.file_name = Some(f.file_name.clone());
            i.sha1 = f.sha1().map(String::from);
            i.size = Some(f.file_length);
            i.download_url = f.download_url.clone();
        }
        if file.is_some() || m.is_some() {
            i.sources.push(LookupSource::CoreApi);
            out.insert(*r, i);
        }
    }
    Ok(out)
}
