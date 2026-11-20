#![allow(dead_code)]
//! Shared helpers for integration tests: fake jars, a fake CurseForge pack zip,
//! and a configurable Modrinth mock.

use mrimport_core::hash::sha1_hex;
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// A tiny valid jar whose bytes differ per `tag`.
pub fn jar(tag: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(vec![]);
    let mut z = zip::ZipWriter::new(&mut buf);
    z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    z.write_all(format!(r#"{{"id":"{tag}","name":"{tag}"}}"#).as_bytes())
        .unwrap();
    z.finish().unwrap();
    buf.into_inner()
}

/// Write a CurseForge modpack zip listing `files` as `(projectID, fileID)`.
pub fn cf_pack(dir: &Path, files: &[(u64, u64)]) -> PathBuf {
    let path = dir.join("pack.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let o = zip::write::SimpleFileOptions::default();
    let manifest = json!({
        "minecraft": {"version": "1.20.1", "modLoaders": [{"id": "fabric-0.15.0", "primary": true}]},
        "manifestType": "minecraftModpack", "name": "Test Pack", "version": "1.0",
        "files": files.iter().map(|(p, f)| json!({"projectID": p, "fileID": f, "required": true})).collect::<Vec<_>>(),
        "overrides": "overrides",
    });
    z.start_file("manifest.json", o).unwrap();
    z.write_all(manifest.to_string().as_bytes()).unwrap();
    z.start_file("overrides/config/x.toml", o).unwrap();
    z.write_all(b"a=1").unwrap();
    z.finish().unwrap();
    path
}

/// Modrinth stand-in. `known` maps SHA-1 -> (project id, slug, title).
/// `search` maps a lowercase query fragment -> (project id, slug, title).
pub struct ModrinthMock {
    pub known: HashMap<String, (String, String, String)>,
    pub search: Vec<(String, (String, String, String))>,
}

struct HashesResponder(HashMap<String, (String, String, String)>);
impl Respond for HashesResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        let mut out = serde_json::Map::new();
        for h in body["hashes"].as_array().unwrap() {
            let h = h.as_str().unwrap();
            if let Some((pid, _slug, title)) = self.0.get(h) {
                out.insert(h.to_string(), version(pid, title, h));
            }
        }
        ResponseTemplate::new(200).set_body_json(out)
    }
}

fn version(pid: &str, title: &str, sha1: &str) -> serde_json::Value {
    json!({
        "id": format!("v-{pid}"), "project_id": pid, "name": title, "version_number": "1.0",
        "version_type": "release", "date_published": "2024-01-01T00:00:00Z",
        "files": [{"hashes": {"sha1": sha1, "sha512": "00"}, "url": format!("https://cdn.modrinth.com/{pid}.jar"),
                   "filename": format!("{pid}.jar"), "primary": true, "size": 10}]
    })
}

struct SearchResponder(Vec<(String, (String, String, String))>);
impl Respond for SearchResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let q = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "query")
            .map(|(_, v)| v.to_lowercase())
            .unwrap_or_default();
        let hits: Vec<_> = self
            .0
            .iter()
            .filter(|(frag, _)| q.contains(frag.as_str()))
            .map(|(_, (pid, slug, title))| json!({"project_id": pid, "slug": slug, "title": title, "description": ""}))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({"hits": hits}))
    }
}

struct ProjectsResponder(HashMap<String, (String, String, String)>);
impl Respond for ProjectsResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let all: Vec<_> = self
            .0
            .values()
            .map(|(pid, slug, title)| json!({"id": pid, "slug": slug, "title": title}))
            .collect();
        ResponseTemplate::new(200).set_body_json(all)
    }
}

struct VersionsResponder(Vec<(String, (String, String, String))>);
impl Respond for VersionsResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let pid = req.url.path().split('/').nth(2).unwrap_or("").to_string();
        let title = self
            .0
            .iter()
            .find(|(_, (p, _, _))| *p == pid)
            .map(|(_, (_, _, t))| t.clone())
            .unwrap_or_default();
        ResponseTemplate::new(200).set_body_json(vec![version(&pid, &title, "ffff")])
    }
}

impl ModrinthMock {
    pub async fn start(self) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/version_files"))
            .respond_with(HashesResponder(self.known.clone()))
            .mount(&s)
            .await;
        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/projects"))
            .respond_with(ProjectsResponder(self.known.clone()))
            .mount(&s)
            .await;
        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search"))
            .respond_with(SearchResponder(self.search.clone()))
            .mount(&s)
            .await;
        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path_regex(r"^/project/[^/]+/version$"))
            .respond_with(VersionsResponder(self.search.clone()))
            .mount(&s)
            .await;
        s
    }
}

pub fn proj(pid: &str, slug: &str, title: &str) -> (String, String, String) {
    (pid.into(), slug.into(), title.into())
}

pub fn sha1(b: &[u8]) -> String {
    sha1_hex(b)
}
