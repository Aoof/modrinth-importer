mod common;

use common::*;
use mrimport_core::cfmodpack::{self, parse_ref, ModpackRef};
use mrimport_core::curseforge::CurseForge;
use mrimport_core::events::Reporter;
use mrimport_core::providers::{self, LoadCtx, LoadOptions};
use mrimport_core::{cflookup::Lookup, Error};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "test-core-key";

fn file_json(id: u64, name: &str, url: Value, sha1: &str, release: u8, date: &str) -> Value {
    json!({"id": id, "fileName": name, "displayName": name, "downloadUrl": url, "releaseType": release,
           "gameVersions": ["1.20.1", "Fabric"], "fileDate": date, "fileLength": 100, "isServerPack": false,
           "hashes": [{"value": sha1, "algo": 1}]})
}

fn project_json(id: u64, slug: &str, class: u64, main: u64, latest: Vec<Value>) -> Value {
    json!({"id": id, "name": format!("Pack {slug}"), "slug": slug, "summary": "a pack", "downloadCount": 1234567.0,
           "classId": class, "mainFileId": main, "latestFiles": latest,
           "links": {"websiteUrl": format!("https://www.curseforge.com/minecraft/modpacks/{slug}")},
           "logo": {"thumbnailUrl": "https://media.forgecdn.net/x.png"}})
}

async fn cf_client(server: &MockServer) -> CurseForge {
    CurseForge::with_base(server.uri(), KEY)
}

async fn cdn_serving(name: &str, bytes: Vec<u8>) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/files/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn search_asks_for_popular_minecraft_modpacks_and_parses_the_hits() {
    let cf = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mods/search"))
        .and(header("x-api-key", KEY))
        .and(query_param("gameId", "432"))
        .and(query_param("classId", "4471"))
        .and(query_param("searchFilter", "all the mods"))
        .and(query_param("sortField", "2"))
        .and(query_param("sortOrder", "desc"))
        .and(query_param("pageSize", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            project_json(925200, "all-the-mods-10", 4471, 6000123, vec![]),
            project_json(2, "atm-lite", 4471, 7, vec![]),
        ], "pagination": {"index": 0, "pageSize": 5, "resultCount": 2, "totalCount": 2}})))
        .expect(1)
        .mount(&cf)
        .await;
    let hits = cf_client(&cf)
        .await
        .search_modpacks("all the mods", 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].slug, "all-the-mods-10");
    assert_eq!(hits[0].download_count as u64, 1234567);
    assert_eq!(
        hits[0].logo.as_ref().unwrap().thumbnail_url.as_deref(),
        Some("https://media.forgecdn.net/x.png")
    );
    assert_eq!(
        cfmodpack::page_url(&hits[0]),
        "https://www.curseforge.com/minecraft/modpacks/all-the-mods-10"
    );
}

#[tokio::test]
async fn a_link_goes_all_the_way_to_a_loaded_pack() {
    let tmp = tempfile::tempdir().unwrap();
    // the real modpack zip, served by a CDN, with its checksum known to CurseForge
    let zip_path = cf_pack(tmp.path(), &[(1, 11)]);
    let zip_bytes = std::fs::read(&zip_path).unwrap();
    let cdn = cdn_serving("pack.zip", zip_bytes.clone()).await;
    let url = format!("{}/files/pack.zip", cdn.uri());

    let cf = MockServer::start().await;
    // slug lookup must NOT filter by class (slugs are unique per class)
    Mock::given(method("GET")).and(path("/mods/search")).and(query_param("slug", "all-the-mods-10"))
        .and(query_param("gameId", "432")).and(query_param_is_missing("classId"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            project_json(925200, "all-the-mods-10", 4471, 6000123, vec![
                file_json(6000123, "ATM10-1.0.zip", json!(url), &sha1(&zip_bytes), 1, "2024-05-01T00:00:00Z")])]})))
        .expect(1).mount(&cf).await;
    Mock::given(method("GET"))
        .and(path("/mods/925200/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&cf)
        .await;
    // the mods inside the pack are then resolved through the same API
    Mock::given(method("POST")).and(path("/mods/files")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
        {"id": 11, "modId": 1, "fileName": "one-1.0.jar", "downloadUrl": null, "fileLength": 5, "hashes": [{"value": "11".repeat(20), "algo": 1}]}]}))).mount(&cf).await;
    Mock::given(method("POST"))
        .and(path("/mods"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [{"id": 1, "name": "One", "slug": "one"}]})),
        )
        .mount(&cf)
        .await;

    let client = cf_client(&cf).await;
    let r = parse_ref("https://www.curseforge.com/minecraft/modpacks/all-the-mods-10/files/all")
        .unwrap();
    assert_eq!(
        r,
        ModpackRef::Slug {
            slug: "all-the-mods-10".into(),
            file_id: None
        }
    );
    let sel = cfmodpack::resolve(&client, &r).await.unwrap();
    assert_eq!(
        sel.file.id, 6000123,
        "the project's main file is used when the link names none"
    );

    let dl = tmp.path().join("dl");
    let zip = cfmodpack::download(&client, &sel, &dl, &Reporter::silent())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&zip).unwrap(), zip_bytes);
    assert!(zip.starts_with(&dl) && zip.file_name().unwrap() == "ATM10-1.0.zip");

    // and it imports like any pack
    let ctx = LoadCtx::with_lookup(Lookup::new(Some(client), false), Reporter::silent());
    let pack = providers::load(&zip, &LoadOptions::default(), &ctx)
        .await
        .unwrap();
    assert_eq!(pack.name, "Test Pack");
    assert_eq!(pack.mods.len(), 1);
}

#[tokio::test]
async fn an_explicit_file_in_the_link_wins_over_the_main_file() {
    let cf = MockServer::start().await;
    Mock::given(method("GET")).and(path("/mods/925200")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
        project_json(925200, "p", 4471, 100, vec![file_json(100, "main.zip", json!(null), "aa", 1, "2024-05-01")])}))).mount(&cf).await;
    Mock::given(method("GET"))
        .and(path("/mods/925200/files/55"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
        file_json(55, "older.zip", json!(null), "bb", 1, "2023-01-01")})))
        .expect(1)
        .mount(&cf)
        .await;
    let sel = cfmodpack::resolve(
        &cf_client(&cf).await,
        &parse_ref("curseforge://install?addonId=925200&fileId=55").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(sel.file.id, 55);
}

#[tokio::test]
async fn only_modpacks_are_accepted_and_unknown_slugs_are_explained() {
    let cf = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/mods/238222"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
        project_json(238222, "jei", 6, 1, vec![])})))
        .mount(&cf)
        .await;
    Mock::given(method("GET"))
        .and(path("/mods/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&cf)
        .await;
    let c = cf_client(&cf).await;
    let e = cfmodpack::resolve(&c, &parse_ref("238222").unwrap())
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("not a modpack"), "{e}");
    let e = cfmodpack::resolve(
        &c,
        &parse_ref("https://www.curseforge.com/minecraft/modpacks/nope").unwrap(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(e.contains("no project called 'nope'"), "{e}");
}

async fn selected_with(cf: &MockServer, file: Value) -> cfmodpack::Selected {
    Mock::given(method("GET"))
        .and(path("/mods/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":
        project_json(9, "pk", 4471, file["id"].as_u64().unwrap(), vec![file])})))
        .mount(cf)
        .await;
    Mock::given(method("GET"))
        .and(path("/mods/9/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(cf)
        .await;
    cfmodpack::resolve(&cf_client(cf).await, &parse_ref("cf:9").unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_download_that_is_not_the_exact_file_is_refused_and_leaves_nothing_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let good = std::fs::read(cf_pack(tmp.path(), &[(1, 1)])).unwrap();
    let cdn = cdn_serving("p.zip", good.clone()).await;
    let url = format!("{}/files/p.zip", cdn.uri());
    let cf = MockServer::start().await;

    // CurseForge lists a DIFFERENT checksum than the bytes we are served
    let sel = selected_with(
        &cf,
        file_json(1, "p.zip", json!(url), &"00".repeat(20), 1, "2024-01-01"),
    )
    .await;
    let dir = tmp.path().join("dl");
    let e = cfmodpack::download(&cf_client(&cf).await, &sel, &dir, &Reporter::silent())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("checksum"), "{e}");
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "the bad file was deleted"
    );
}

#[tokio::test]
async fn an_html_page_is_not_mistaken_for_a_modpack() {
    let tmp = tempfile::tempdir().unwrap();
    let cdn = cdn_serving("p.zip", b"<html>Access denied</html>".to_vec()).await;
    let cf = MockServer::start().await;
    let sel = selected_with(
        &cf,
        file_json(
            1,
            "p.zip",
            json!(format!("{}/files/p.zip", cdn.uri())),
            "ab",
            1,
            "2024-01-01",
        ),
    )
    .await;
    let e = cfmodpack::download(
        &cf_client(&cf).await,
        &sel,
        &tmp.path().join("dl"),
        &Reporter::silent(),
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("did not return a .zip"), "{e}");
}

#[tokio::test]
async fn the_download_url_endpoint_is_used_when_the_file_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = std::fs::read(cf_pack(tmp.path(), &[(1, 1)])).unwrap();
    let cdn = cdn_serving("p.zip", bytes.clone()).await;
    let cf = MockServer::start().await;
    let sel = selected_with(
        &cf,
        file_json(1, "p.zip", json!(null), &sha1(&bytes), 1, "2024-01-01"),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/mods/9/files/1/download-url"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": format!("{}/files/p.zip", cdn.uri())})),
        )
        .expect(1)
        .mount(&cf)
        .await;
    let p = cfmodpack::download(
        &cf_client(&cf).await,
        &sel,
        &tmp.path().join("dl"),
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(p).unwrap(), bytes);
}

#[tokio::test]
async fn when_curseforge_gives_no_download_the_person_gets_the_file_page() {
    let tmp = tempfile::tempdir().unwrap();
    let cf = MockServer::start().await;
    let sel = selected_with(
        &cf,
        file_json(1, "p.zip", json!(null), "ab", 1, "2024-01-01"),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/mods/9/files/1/download-url"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&cf)
        .await;
    let e = cfmodpack::download(
        &cf_client(&cf).await,
        &sel,
        &tmp.path().join("dl"),
        &Reporter::silent(),
    )
    .await
    .unwrap_err();
    match &e {
        Error::NeedsManualDownload { name, url } => {
            assert_eq!(name, "Pack pk");
            assert_eq!(
                url,
                "https://www.curseforge.com/minecraft/modpacks/pk/files/1"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(e.to_string().contains("Download it in your browser"));
}
