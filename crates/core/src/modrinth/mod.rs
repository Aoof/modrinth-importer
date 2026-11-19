//! Minimal Modrinth API v2 client.

use crate::{debuglog, Error, Result};
use reqwest::{header, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

pub const DEFAULT_BASE: &str = "https://api.modrinth.com/v2";

#[derive(Clone)]
pub struct Modrinth {
    http: Client,
    base: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: HashMap<String, String>,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub version_type: String,
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub date_published: String,
}

impl Version {
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
    #[serde(default)]
    pub project_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub downloads: u64,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

impl Modrinth {
    pub fn new(token: Option<String>) -> Self {
        Self::with_base(DEFAULT_BASE, token)
    }

    pub fn with_base(base: impl Into<String>, token: Option<String>) -> Self {
        let http = Client::builder()
            .user_agent(crate::user_agent())
            .timeout(Duration::from_secs(60))
            .build()
            .expect("http client");
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            token,
        }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    fn req(&self, m: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut r = self.http.request(m, format!("{}{}", self.base, path));
        if let Some(t) = &self.token {
            r = r.header(header::AUTHORIZATION, t);
        }
        r
    }

    /// Send with retry on 429 (honouring `X-Ratelimit-Reset`) and transient 5xx.
    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<Response> {
        let mut attempt = 0;
        loop {
            let req = build().build()?;
            let label = format!("{} {}", req.method(), req.url());
            let started = std::time::Instant::now();
            let resp = self.http.execute(req).await.map_err(|e| {
                debuglog::log(format!("Modrinth {label} -> request failed: {e}"));
                Error::from(e)
            })?;
            let status = resp.status();
            debuglog::log(format!(
                "Modrinth {label} -> {} ({} ms) ratelimit-remaining={}",
                status.as_u16(),
                started.elapsed().as_millis(),
                resp.headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-")
            ));
            if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()) && attempt < 5
            {
                let wait = resp
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .clamp(1, 60);
                tokio::time::sleep(Duration::from_secs(wait)).await;
                attempt += 1;
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                debuglog::log(format!(
                    "Modrinth {label} error body: {}",
                    debuglog::snippet(&body)
                ));
                return Err(Error::api("Modrinth", status.as_u16(), body));
            }
            return Ok(resp);
        }
    }

    /// Batch lookup of versions by file hash. `algorithm` is `sha1` or `sha512`.
    pub async fn versions_from_hashes(
        &self,
        hashes: &[String],
        algorithm: &str,
    ) -> Result<HashMap<String, Version>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let body = serde_json::json!({ "hashes": hashes, "algorithm": algorithm });
        let r = self
            .send(|| {
                self.req(reqwest::Method::POST, "/version_files")
                    .json(&body)
            })
            .await?;
        Ok(r.json().await?)
    }

    pub async fn project(&self, id_or_slug: &str) -> Result<Option<Project>> {
        match self
            .send(|| self.req(reqwest::Method::GET, &format!("/project/{id_or_slug}")))
            .await
        {
            Ok(r) => Ok(Some(r.json().await?)),
            Err(Error::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn projects(&self, ids: &[String]) -> Result<Vec<Project>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let ids_json = serde_json::to_string(ids)?;
        let r = self
            .send(|| {
                self.req(reqwest::Method::GET, "/projects")
                    .query(&[("ids", &ids_json)])
            })
            .await?;
        Ok(r.json().await?)
    }

    /// Versions of a project, optionally filtered by loader and Minecraft version.
    pub async fn project_versions(
        &self,
        project: &str,
        loader: Option<&str>,
        mc_version: Option<&str>,
    ) -> Result<Vec<Version>> {
        let mut q: Vec<(&str, String)> = vec![];
        if let Some(l) = loader {
            q.push(("loaders", serde_json::to_string(&[l])?));
        }
        if let Some(v) = mc_version {
            q.push(("game_versions", serde_json::to_string(&[v])?));
        }
        let r = self
            .send(|| {
                self.req(reqwest::Method::GET, &format!("/project/{project}/version"))
                    .query(&q)
            })
            .await?;
        Ok(r.json().await?)
    }

    /// Search mods filtered by loader and Minecraft version.
    pub async fn search_mods(
        &self,
        query: &str,
        loader: Option<&str>,
        mc_version: Option<&str>,
        limit: u32,
    ) -> Result<Vec<SearchHit>> {
        let mut facets: Vec<Vec<String>> = vec![vec!["project_type:mod".into()]];
        if let Some(l) = loader {
            facets.push(vec![format!("categories:{l}")]);
        }
        if let Some(v) = mc_version {
            facets.push(vec![format!("versions:{v}")]);
        }
        let facets = serde_json::to_string(&facets)?;
        let limit = limit.to_string();
        let r = self
            .send(|| {
                self.req(reqwest::Method::GET, "/search").query(&[
                    ("query", query),
                    ("facets", &facets),
                    ("limit", &limit),
                ])
            })
            .await?;
        Ok(r.json::<SearchResponse>().await?.hits)
    }

    /// Reachability check (a tiny, cacheable public endpoint).
    pub async fn ping(&self) -> Result<()> {
        self.send(|| self.req(reqwest::Method::GET, "/tag/loader"))
            .await
            .map(|_| ())
    }

    pub async fn current_user_id(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct U {
            id: String,
        }
        let r = self
            .send(|| self.req(reqwest::Method::GET, "/user"))
            .await?;
        Ok(r.json::<U>().await?.id)
    }

    /// Raw multipart POST used by publishing (`/project`, `/version`).
    pub(crate) async fn post_multipart(
        &self,
        path: &str,
        build_form: impl Fn() -> reqwest::multipart::Form,
    ) -> Result<serde_json::Value> {
        if self.token.is_none() {
            return Err(Error::MissingCredential("Modrinth personal access token"));
        }
        let r = self
            .send(|| {
                self.req(reqwest::Method::POST, path)
                    .multipart(build_form())
            })
            .await?;
        Ok(r.json().await?)
    }

    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let r = self.send(|| self.http.get(url)).await?;
        Ok(r.bytes().await?.to_vec())
    }
}
