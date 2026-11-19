//! Minimal CurseForge Core API client (needs a free API key).

use crate::{Error, Result};
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
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
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

    async fn post<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let r = self
            .http
            .post(format!("{}{}", self.base, path))
            .header("x-api-key", &self.key)
            .json(&body)
            .send()
            .await?;
        let status = r.status();
        if !status.is_success() {
            return Err(Error::Api {
                provider: "CurseForge",
                status: status.as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
        Ok(r.json::<Data<T>>().await?.data)
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
}

impl CurseForge {
    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let r = self
            .http
            .get(format!("{}{}", self.base, path))
            .header("x-api-key", &self.key)
            .query(query)
            .send()
            .await?;
        let status = r.status();
        if !status.is_success() {
            return Err(Error::Api {
                provider: "CurseForge",
                status: status.as_u16(),
                body: r.text().await.unwrap_or_default(),
            });
        }
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
}
