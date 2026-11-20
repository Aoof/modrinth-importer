//! Minimal CurseForge Core API client (needs a free API key).

use crate::{debuglog, Error, Result};
use reqwest::Client;
use serde::Deserialize;

const BASE: &str = "https://api.curseforge.com/v1";

#[derive(Clone)]
pub struct CurseForge {
    http: Client,
    key: String,
    base: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfHash {
    pub value: String,
    /// 1 = sha1, 2 = md5
    pub algo: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    pub file_name: String,
    pub display_name: Option<String>,
    pub download_url: Option<String>,
    pub file_length: u64,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
}

impl CfFile {
    pub fn sha1(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLinks {
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub links: Option<CfLinks>,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

impl CurseForge {
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_base(BASE, key)
    }
    pub fn with_base(base: impl Into<String>, key: impl Into<String>) -> Self {
        let http = Client::builder()
            .user_agent(crate::user_agent())
            .build()
            .expect("http client");
        Self {
            http,
            key: key.into(),
            base: base.into(),
        }
    }

    /// Send a request with the API key, logging method, status, timing and (on
    /// failure) a truncated body to the debug log. The key itself is never logged.
    async fn send(&self, rb: reqwest::RequestBuilder, label: &str) -> Result<reqwest::Response> {
        let started = std::time::Instant::now();
        let r = rb
            .header("x-api-key", self.key.trim())
            .send()
            .await
            .map_err(|e| {
                debuglog::log(format!("CurseForge {label} -> request failed: {e}"));
                Error::from(e)
            })?;
        let status = r.status();
        let h = |n: &str| {
            r.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string()
        };
        debuglog::log(format!(
            "CurseForge {label} -> {} ({} ms) server={} cf-ray={} content-type={}",
            status.as_u16(),
            started.elapsed().as_millis(),
            h("server"),
            h("cf-ray"),
            h("content-type")
        ));
        if !status.is_success() {
            let body = r.text().await.unwrap_or_default();
            debuglog::log(format!(
                "CurseForge {label} error body: {}",
                debuglog::snippet(&body)
            ));
            return Err(Error::api("CurseForge", status.as_u16(), body));
        }
        Ok(r)
    }

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let rb = self.http.post(format!("{}{}", self.base, path)).json(&body);
        let r = self.send(rb, &format!("POST {path}")).await?;
        Ok(r.json::<Data<T>>().await?.data)
    }

    /// Cheap authenticated request used to validate the API key.
    pub async fn check_key(&self) -> Result<()> {
        self.get::<serde_json::Value>("/games", &[("pageSize", "1".into())])
            .await
            .map(|_| ())
    }

    pub async fn files(&self, file_ids: &[u64]) -> Result<Vec<CfFile>> {
        if file_ids.is_empty() {
            return Ok(vec![]);
        }
        self.post("/mods/files", serde_json::json!({ "fileIds": file_ids }))
            .await
    }

    pub async fn mods(&self, mod_ids: &[u64]) -> Result<Vec<CfMod>> {
        if mod_ids.is_empty() {
            return Ok(vec![]);
        }
        self.post("/mods", serde_json::json!({ "modIds": mod_ids }))
            .await
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfModInfo {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfProjectFile {
    pub id: u64,
    pub file_name: String,
    pub display_name: String,
    pub download_url: Option<String>,
    /// 1 = release, 2 = beta, 3 = alpha
    pub release_type: u8,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub file_date: Option<String>,
    #[serde(default)]
    pub file_length: Option<u64>,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
    /// Server-side variants of a modpack are not what people want to import.
    #[serde(default)]
    pub is_server_pack: Option<bool>,
}

impl CfProjectFile {
    pub fn sha1(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.as_str())
    }
}

/// Minecraft on CurseForge.
pub const MINECRAFT_GAME_ID: u32 = 432;
/// The "Modpacks" class of Minecraft projects.
pub const MODPACK_CLASS_ID: u64 = 4471;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLogo {
    pub thumbnail_url: Option<String>,
}

/// A CurseForge project as returned by search and "get mod" (used for modpacks).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfModpack {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    #[serde(default)]
    pub logo: Option<CfLogo>,
    #[serde(default)]
    pub links: Option<CfLinks>,
    #[serde(default)]
    pub main_file_id: Option<u64>,
    #[serde(default)]
    pub latest_files: Vec<CfProjectFile>,
    #[serde(default)]
    pub class_id: Option<u64>,
}

impl CurseForge {
    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let rb = self.http.get(format!("{}{}", self.base, path)).query(query);
        let r = self.send(rb, &format!("GET {path}")).await?;
        Ok(r.json::<Data<T>>().await?.data)
    }

    pub async fn mod_info(&self, id: u64) -> Result<CfModInfo> {
        self.get(&format!("/mods/{id}"), &[]).await
    }

    /// All files of a project (paginated).
    pub async fn mod_files(&self, id: u64) -> Result<Vec<CfProjectFile>> {
        let mut all = vec![];
        let mut index = 0usize;
        loop {
            let page: Vec<CfProjectFile> = self
                .get(
                    &format!("/mods/{id}/files"),
                    &[("index", index.to_string()), ("pageSize", "50".into())],
                )
                .await?;
            let n = page.len();
            all.extend(page);
            if n < 50 {
                break;
            }
            index += n;
        }
        Ok(all)
    }

    /// Search Minecraft modpacks by name, most popular first.
    pub async fn search_modpacks(&self, query: &str, limit: u32) -> Result<Vec<CfModpack>> {
        self.get(
            "/mods/search",
            &[
                ("gameId", MINECRAFT_GAME_ID.to_string()),
                ("classId", MODPACK_CLASS_ID.to_string()),
                ("searchFilter", query.to_string()),
                ("sortField", "2".into()), // popularity
                ("sortOrder", "desc".into()),
                ("pageSize", limit.clamp(1, 50).to_string()),
            ],
        )
        .await
    }

    /// Find a project by its URL slug (slugs are unique within a class, so no class filter).
    pub async fn project_by_slug(&self, slug: &str) -> Result<Option<CfModpack>> {
        let hits: Vec<CfModpack> = self
            .get(
                "/mods/search",
                &[
                    ("gameId", MINECRAFT_GAME_ID.to_string()),
                    ("slug", slug.to_string()),
                    ("pageSize", "10".into()),
                ],
            )
            .await?;
        Ok(hits.into_iter().find(|m| m.slug.eq_ignore_ascii_case(slug)))
    }

    pub async fn project(&self, id: u64) -> Result<CfModpack> {
        self.get(&format!("/mods/{id}"), &[]).await
    }

    /// The newest `n` files of a project.
    pub async fn recent_files(&self, id: u64, n: u32) -> Result<Vec<CfProjectFile>> {
        self.get(
            &format!("/mods/{id}/files"),
            &[
                ("index", "0".into()),
                ("pageSize", n.clamp(1, 50).to_string()),
            ],
        )
        .await
    }

    pub async fn file(&self, id: u64, file_id: u64) -> Result<CfProjectFile> {
        self.get(&format!("/mods/{id}/files/{file_id}"), &[]).await
    }

    /// The download URL of a file, or `None` when CurseForge won't hand one out.
    pub async fn download_url(&self, id: u64, file_id: u64) -> Option<String> {
        self.get::<String>(&format!("/mods/{id}/files/{file_id}/download-url"), &[])
            .await
            .ok()
            .filter(|u| !u.is_empty())
    }
}
