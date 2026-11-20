mod common;

use common::*;
use mrimport_core::cflookup::{Lookup, LookupBases};
use mrimport_core::curseforge::CurseForge;
use mrimport_core::events::Reporter;
use mrimport_core::manual;
use mrimport_core::model::LookupSource;
use mrimport_core::modrinth::Modrinth;
use mrimport_core::providers::{self, LoadCtx, LoadOptions};
use mrimport_core::resolve::{resolve_with, Confidence, Resolution, ResolveOptions};
use mrimport_core::Error;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "test-core-key";

/// Cursemaven stand-in: serves `jars` (project, file) -> bytes, 404 for the rest.
async fn maven(jars: &[(u64, u64, Vec<u8>)], expect_each: Option<u64>) -> MockServer {
    let s = MockServer::start().await;
    for (p, f, bytes) in jars {
        let m = Mock::given(method("GET"))
            .and(path(format!("/curse/maven/x-{p}/{f}/x-{p}-{f}.jar")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()));
        match expect_each {
            Some(n) => m.expect(n).mount(&s).await,
            None => m.mount(&s).await,
        }
    }
    s
}

/// `(project id, title, page url, [(file id, file name)])`
type WidgetProject<'a> = (u64, &'a str, &'a str, Vec<(u64, &'a str)>);

/// cfwidget stand-in.
async fn widget(projects: &[WidgetProject<'_>], expect_calls: Option<u64>) -> MockServer {
    let s = MockServer::start().await;
    for (pid, title, url, files) in projects {
        let m = Mock::given(method("GET"))
            .and(path(format!("/{pid}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": pid, "title": title, "urls": {"curseforge": url},
                "files": files.iter().map(|(i, n)| json!({"id": i, "name": n})).collect::<Vec<_>>()
            })));
        match expect_calls {
            Some(n) => m.expect(n).mount(&s).await,
            None => m.mount(&s).await,
        }
    }
    s
}

fn bases(maven: &MockServer, widget: &MockServer) -> LookupBases {
    LookupBases {
        cursemaven: maven.uri(),
        cfwidget: widget.uri(),
    }
}

fn ctx(lookup: Lookup) -> LoadCtx {
    LoadCtx::with_lookup(lookup, Reporter::silent())
}

#[tokio::test]
async fn with_a_key_everything_is_exact_and_no_third_party_is_contacted() {
    let tmp = tempfile::tempdir().unwrap();
    let (j1, j2) = (jar("one"), jar("two"));
    let pack = cf_pack(tmp.path(), &[(1, 11), (2, 22)]);

    let cf = MockServer::start().await;
    Mock::given(method("POST")).and(path("/mods/files")).and(header("x-api-key", KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            {"id": 11, "modId": 1, "fileName": "one-1.0.jar", "downloadUrl": null, "fileLength": 5, "hashes": [{"value": sha1(&j1), "algo": 1}]},
            {"id": 22, "modId": 2, "fileName": "two-1.0.jar", "downloadUrl": "https://edge.forgecdn.net/files/0/22/two-1.0.jar", "fileLength": 5, "hashes": [{"value": sha1(&j2), "algo": 1}]},
        ]}))).mount(&cf).await;
    Mock::given(method("POST")).and(path("/mods")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
        {"id": 1, "name": "One", "slug": "one", "links": {"websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/one"}},
        {"id": 2, "name": "Two", "slug": "two"},
    ]}))).mount(&cf).await;

    // keyless is OFF: the third-party mocks must see zero requests (verified on drop)
    let m = maven(&[(1, 11, j1.clone()), (2, 22, j2.clone())], Some(0)).await;
    let w = widget(
        &[(
            1,
            "One",
            "https://www.curseforge.com/minecraft/mc-mods/one",
            vec![],
        )],
        Some(0),
    )
    .await;

    let lookup =
        Lookup::new(Some(CurseForge::with_base(cf.uri(), KEY)), false).with_bases(bases(&m, &w));
    let pack = providers::load(&pack, &LoadOptions::default(), &ctx(lookup))
        .await
        .unwrap();

    assert_eq!(pack.mods.len(), 2);
    let one = &pack.mods[0];
    assert_eq!(one.expected_file_name.as_deref(), Some("one-1.0.jar"));
    assert_eq!(one.sha1.as_deref(), Some(sha1(&j1).as_str()));
    assert_eq!(one.lookup_sources, vec![LookupSource::CoreApi]);
    assert_eq!(
        one.cf_url.as_deref(),
        Some("https://www.curseforge.com/minecraft/mc-mods/one")
    );
    assert!(pack.mods[1].download_url.is_some());

    // exact Modrinth match for mod one only
    let mr = ModrinthMock {
        known: [(sha1(&j1), proj("P1", "one", "One"))].into(),
        search: vec![],
    }
    .start()
    .await;
    let resolved = resolve_with(
        &pack,
        &Modrinth::with_base(mr.uri(), None),
        &Reporter::silent(),
        ResolveOptions::default(),
    )
    .await
    .unwrap();
    assert!(matches!(
        &resolved[0].resolution,
        Resolution::Matched {
            confidence: Confidence::Exact,
            ..
        }
    ));
    // mod two isn't on Modrinth but has a download URL, so it needs no manual step
    assert!(matches!(
        resolved[1].resolution,
        Resolution::Unmatched { .. }
    ));
    assert!(!manual::needs_manual(&resolved[1]));
}

#[tokio::test]
async fn without_a_key_and_keyless_off_it_refuses_with_guidance() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = cf_pack(tmp.path(), &[(1, 11)]);
    let err = providers::load(
        &pack,
        &LoadOptions::default(),
        &ctx(Lookup::new(None, false)),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::MissingCredential(_)));
    assert!(err.to_string().contains("keyless lookup"), "{err}");
}

#[tokio::test]
async fn keyless_chain_hash_then_widget_then_manual() {
    let tmp = tempfile::tempdir().unwrap();
    let (j1, j2) = (jar("one"), jar("two"));
    let pack_zip = cf_pack(tmp.path(), &[(1, 11), (2, 22), (3, 33), (4, 44), (5, 55)]);

    // Only mods 1 and 2 are served by cursemaven; 3, 4, 5 are 404.
    let m = maven(&[(1, 11, j1.clone()), (2, 22, j2.clone())], None).await;
    // cfwidget: project 1 must NOT be asked (it matches exactly on Modrinth), the rest are.
    let w = MockServer::start().await;
    Mock::given(method("GET")).and(path("/1")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":1,"title":"One","urls":{"curseforge":"https://www.curseforge.com/minecraft/mc-mods/one"},"files":[]}))).expect(0).mount(&w).await;
    Mock::given(method("GET")).and(path("/2")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":2,"title":"Two","urls":{"curseforge":"https://www.curseforge.com/minecraft/mc-mods/two"},"files":[{"id":22,"name":"two-1.0.jar"}]}))).mount(&w).await;
    Mock::given(method("GET")).and(path("/3")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":3,"title":"Three Mod","urls":{"curseforge":"https://www.curseforge.com/minecraft/mc-mods/three-mod"},"files":[{"id":33,"name":"three-1.0.jar"}]}))).mount(&w).await;
    // 4: widget has never heard of it
    Mock::given(method("GET"))
        .and(path("/4"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&w)
        .await;
    Mock::given(method("GET")).and(path("/5")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":5,"title":"Five","urls":{"curseforge":"https://www.curseforge.com/minecraft/mc-mods/five"},"files":[{"id":55,"name":"five-2.3.jar"}]}))).mount(&w).await;

    let lookup = Lookup::new(None, true)
        .with_bases(bases(&m, &w))
        .with_cache_dir(Some(tmp.path().join("cache")));
    let pack = providers::load(&pack_zip, &LoadOptions::default(), &ctx(lookup.clone()))
        .await
        .unwrap();

    // after load: 1 and 2 are hashed, with the cursemaven URL kept for bundling
    assert_eq!(pack.mods[0].sha1.as_deref(), Some(sha1(&j1).as_str()));
    assert!(pack.mods[1]
        .download_url
        .as_deref()
        .unwrap()
        .contains("/curse/maven/x-2/22/"));
    assert!(pack.mods[2].sha1.is_none() && pack.mods[3].sha1.is_none());

    let mr = ModrinthMock {
        known: [(sha1(&j1), proj("P1", "one", "One"))].into(),
        search: vec![("three".into(), proj("P3", "three-mod", "Three Mod"))],
    }
    .start()
    .await;
    let resolved = resolve_with(
        &pack,
        &Modrinth::with_base(mr.uri(), None),
        &Reporter::silent(),
        ResolveOptions {
            enricher: Some(&lookup),
            fuzzy: true, // this scenario exercises the opt-in by-name stage
        },
    )
    .await
    .unwrap();

    // 1: exact via the jar hash
    assert!(matches!(
        &resolved[0].resolution,
        Resolution::Matched {
            confidence: Confidence::Exact,
            ..
        }
    ));
    // 2: hashed, not on Modrinth, downloadable -> no manual step
    assert!(matches!(
        resolved[1].resolution,
        Resolution::Unmatched { .. }
    ));
    assert!(!manual::needs_manual(&resolved[1]));
    // 3: only the widget knew the name -> by-name match, flagged as approximate
    match &resolved[2].resolution {
        Resolution::Matched {
            confidence: Confidence::Fuzzy,
            candidate,
        } => assert_eq!(candidate.slug, "three-mod"),
        r => panic!("mod 3: {r:?}"),
    }
    // 4: nobody knows it -> manual, can't be verified, link falls back to the project id
    assert!(manual::needs_manual(&resolved[3]));
    let t4 = manual::task(&resolved[3].entry).unwrap();
    assert_eq!(t4.exactness, manual::Exactness::Unverifiable);
    assert_eq!(t4.project_url, "https://www.curseforge.com/projects/4");
    // 5: widget gave the exact file name + page -> manual, verifiable by name, with a file link
    assert!(manual::needs_manual(&resolved[4]));
    let t5 = manual::task(&resolved[4].entry).unwrap();
    assert_eq!(t5.exactness, manual::Exactness::FileName);
    assert_eq!(t5.expected_file_name.as_deref(), Some("five-2.3.jar"));
    assert_eq!(
        t5.file_url.as_deref(),
        Some("https://www.curseforge.com/minecraft/mc-mods/five/files/55")
    );
    assert!(resolved[4]
        .entry
        .lookup_sources
        .contains(&LookupSource::CfWidget));
}

#[tokio::test]
async fn rejected_key_falls_back_only_when_keyless_is_on() {
    let tmp = tempfile::tempdir().unwrap();
    let j1 = jar("one");
    let pack_zip = cf_pack(tmp.path(), &[(1, 11)]);
    let cf = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&cf)
        .await;

    // keyless OFF: the 403 is an error that carries the guidance
    let off = Lookup::new(Some(CurseForge::with_base(cf.uri(), KEY)), false);
    let err = providers::load(&pack_zip, &LoadOptions::default(), &ctx(off))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 403, .. }));
    assert!(err.to_string().contains("Core API key"), "{err}");

    // keyless ON: warns and carries on with cursemaven
    let m = maven(&[(1, 11, j1.clone())], None).await;
    let w = widget(&[], None).await;
    let on =
        Lookup::new(Some(CurseForge::with_base(cf.uri(), KEY)), true).with_bases(bases(&m, &w));
    let out = on
        .files(
            &[mrimport_core::model::CfRef {
                project_id: 1,
                file_id: 11,
            }],
            &Reporter::silent(),
        )
        .await
        .unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].contains("keyless"));
    let info = out.infos.values().next().unwrap();
    assert_eq!(info.sha1.as_deref(), Some(sha1(&j1).as_str()));
}

#[tokio::test]
async fn hashes_are_cached_between_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let j1 = jar("one");
    // the jar endpoint may be hit exactly once across two lookups
    let m = maven(&[(1, 11, j1.clone())], Some(1)).await;
    let w = widget(&[], None).await;
    let l = Lookup::new(None, true)
        .with_bases(bases(&m, &w))
        .with_cache_dir(Some(tmp.path().to_path_buf()));
    let r = [mrimport_core::model::CfRef {
        project_id: 1,
        file_id: 11,
    }];
    let a = l.files(&r, &Reporter::silent()).await.unwrap();
    let b = l.files(&r, &Reporter::silent()).await.unwrap();
    assert_eq!(a.infos[&r[0]].sha1, b.infos[&r[0]].sha1);
    assert!(tmp.path().join("cf-hashes.json").exists());
}

#[tokio::test]
async fn an_html_page_served_with_200_is_not_mistaken_for_a_jar() {
    let m = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>rate limited</html>"))
        .mount(&m)
        .await;
    let w = widget(&[], None).await;
    let l = Lookup::new(None, true).with_bases(bases(&m, &w));
    let r = mrimport_core::model::CfRef {
        project_id: 9,
        file_id: 99,
    };
    let out = l.files(&[r], &Reporter::silent()).await.unwrap();
    assert!(out.infos.get(&r).and_then(|i| i.sha1.as_ref()).is_none());
}

#[tokio::test]
async fn widget_still_processing_is_retried() {
    let w = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/7"))
        .respond_with(ResponseTemplate::new(202))
        .up_to_n_times(1)
        .mount(&w)
        .await;
    Mock::given(method("GET")).and(path("/7")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":7,"title":"Seven","urls":{"curseforge":"https://www.curseforge.com/minecraft/mc-mods/seven"},"files":[{"id":77,"name":"seven.jar"}]}))).mount(&w).await;
    let m = maven(&[], None).await;
    let l = Lookup::new(None, true).with_bases(bases(&m, &w));
    let mut e = mrimport_core::model::ModEntry::new("cf-7-77.jar");
    e.cf = Some(mrimport_core::model::CfRef {
        project_id: 7,
        file_id: 77,
    });
    let mut v = vec![e];
    l.enrich(&mut v, &Reporter::silent()).await;
    assert_eq!(v[0].expected_file_name.as_deref(), Some("seven.jar"));
}

/// The Core API answers but gives no hash for a file (e.g. distribution restricted).
/// Whether cursemaven is asked as a fallback must depend on the keyless setting alone.
#[tokio::test]
async fn keyless_switch_alone_decides_whether_third_parties_are_contacted() {
    let j1 = jar("one");
    let r = mrimport_core::model::CfRef {
        project_id: 1,
        file_id: 11,
    };
    let cf = MockServer::start().await;
    Mock::given(method("POST")).and(path("/mods/files")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
        {"id": 11, "modId": 1, "fileName": "one-1.0.jar", "downloadUrl": null, "fileLength": 5, "hashes": []}
    ]}))).mount(&cf).await;
    Mock::given(method("POST"))
        .and(path("/mods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            {"id": 1, "name": "One", "slug": "one"}
        ]})))
        .mount(&cf)
        .await;
    let key = || Some(CurseForge::with_base(cf.uri(), KEY));

    // OFF: cursemaven must receive zero requests (checked when the mock server drops)
    {
        let m = maven(&[(1, 11, j1.clone())], Some(0)).await;
        let w = widget(&[], None).await;
        let out = Lookup::new(key(), false)
            .with_bases(bases(&m, &w))
            .files(&[r], &Reporter::silent())
            .await
            .unwrap();
        assert!(
            out.infos[&r].sha1.is_none(),
            "no hash without the keyless fallback"
        );
        assert_eq!(
            out.infos[&r].file_name.as_deref(),
            Some("one-1.0.jar"),
            "Core API data is still used"
        );
    }
    // ON: exactly one request, and the hash is filled in
    {
        let m = maven(&[(1, 11, j1.clone())], Some(1)).await;
        let w = widget(&[], None).await;
        let out = Lookup::new(key(), true)
            .with_bases(bases(&m, &w))
            .files(&[r], &Reporter::silent())
            .await
            .unwrap();
        assert_eq!(out.infos[&r].sha1.as_deref(), Some(sha1(&j1).as_str()));
        assert_eq!(
            out.infos[&r].sources,
            vec![LookupSource::CoreApi, LookupSource::CurseMaven]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Exact-only by default: everything else is downloaded from the source, or awaits the user.
// ---------------------------------------------------------------------------------------------

async fn cdn(files: &[(&str, Option<Vec<u8>>)]) -> MockServer {
    let s = MockServer::start().await;
    for (name, bytes) in files {
        let resp = match bytes {
            Some(b) => ResponseTemplate::new(200).set_body_bytes(b.clone()),
            None => ResponseTemplate::new(404),
        };
        Mock::given(method("GET"))
            .and(path(format!("/files/{name}")))
            .respond_with(resp)
            .mount(&s)
            .await;
    }
    s
}

#[tokio::test]
async fn only_exact_matches_use_modrinth_everything_else_is_downloaded_or_awaits_the_user() {
    use mrimport_core::export::mrpack::{write_mrpack, ExportOptions};
    use mrimport_core::fetch::fetch_unmatched;

    let tmp = tempfile::tempdir().unwrap();
    let (j1, j2, j3) = (jar("one"), jar("two"), jar("three"));
    let pack_zip = cf_pack(tmp.path(), &[(1, 11), (2, 22), (3, 33), (4, 44), (5, 55)]);

    // CDN: mod 2 serves the right bytes; mod 3 serves DIFFERENT bytes than the listed hash;
    // mod 5 is gone (404). Mod 4 has no download URL at all (distribution restricted).
    let cdn = cdn(&[
        ("two.jar", Some(j2.clone())),
        ("three.jar", Some(jar("not-the-listed-build"))),
        ("five.jar", None),
    ])
    .await;
    let u = |n: &str| format!("{}/files/{n}", cdn.uri());

    let cf = MockServer::start().await;
    Mock::given(method("POST")).and(path("/mods/files")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
        {"id": 11, "modId": 1, "fileName": "one-1.0.jar", "downloadUrl": u("one.jar"), "fileLength": 5, "hashes": [{"value": sha1(&j1), "algo": 1}]},
        {"id": 22, "modId": 2, "fileName": "two-1.0.jar", "downloadUrl": u("two.jar"), "fileLength": 5, "hashes": [{"value": sha1(&j2), "algo": 1}]},
        {"id": 33, "modId": 3, "fileName": "three-1.0.jar", "downloadUrl": u("three.jar"), "fileLength": 5, "hashes": [{"value": sha1(&j3), "algo": 1}]},
        {"id": 44, "modId": 4, "fileName": "four-1.0.jar", "downloadUrl": null, "fileLength": 5, "hashes": [{"value": "44".repeat(20), "algo": 1}]},
        {"id": 55, "modId": 5, "fileName": "five-1.0.jar", "downloadUrl": u("five.jar"), "fileLength": 5, "hashes": [{"value": "55".repeat(20), "algo": 1}]},
    ]}))).mount(&cf).await;
    Mock::given(method("POST"))
        .and(path("/mods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            {"id": 1, "name": "One", "slug": "one"}, {"id": 2, "name": "Two", "slug": "two"},
            {"id": 3, "name": "Three", "slug": "three"}, {"id": 4, "name": "Four", "slug": "four"},
            {"id": 5, "name": "Five", "slug": "five"},
        ]})))
        .mount(&cf)
        .await;

    let lookup = Lookup::new(Some(CurseForge::with_base(cf.uri(), KEY)), false);
    let pack = providers::load(&pack_zip, &LoadOptions::default(), &ctx(lookup))
        .await
        .unwrap();

    // Modrinth knows mod 1 exactly, and ALSO has a similarly named project for "two"/"three"
    // that a by-name search would happily pick. It must not be used.
    let mr_server = ModrinthMock {
        known: [(sha1(&j1), proj("P1", "one", "One"))].into(),
        search: vec![
            ("two".into(), proj("P2", "two", "Two")),
            ("three".into(), proj("P3", "three", "Three")),
        ],
    }
    .start()
    .await;
    let mr = Modrinth::with_base(mr_server.uri(), None);
    let mut resolved = resolve_with(&pack, &mr, &Reporter::silent(), ResolveOptions::default())
        .await
        .unwrap();

    assert!(matches!(
        &resolved[0].resolution,
        Resolution::Matched {
            confidence: Confidence::Exact,
            ..
        }
    ));
    for (i, r) in resolved.iter().enumerate().skip(1) {
        assert!(
            matches!(r.resolution, Resolution::Unmatched { .. }),
            "mod {} must not be matched by name",
            i + 1
        );
    }
    let searched = mr_server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|r| r.url.path() == "/search");
    assert!(
        !searched,
        "no by-name search is made unless it is turned on"
    );

    // Download the rest from the source.
    let dl = tmp.path().join("dl");
    let rep = fetch_unmatched(&mut resolved, &dl, &mr, &Reporter::silent())
        .await
        .unwrap();
    assert_eq!(rep.fetched, 1, "only mod 2 downloads and verifies");
    assert_eq!(
        rep.failed.len(),
        2,
        "mod 3 (wrong bytes) and mod 5 (404) fail: {:?}",
        rep.failed
    );
    assert!(rep
        .failed
        .iter()
        .any(|(i, m)| *i == 2 && m.contains("checksum mismatch")));
    assert!(rep.failed.iter().any(|(i, m)| *i == 4 && m.contains("404")));
    assert!(resolved[1].entry.origin.is_some() && !manual::needs_manual(&resolved[1]));
    // 3, 4, 5 now await the user; 3 left no file behind
    for i in [2, 3, 4] {
        assert!(
            manual::needs_manual(&resolved[i]),
            "mod {} should await manual resolution",
            i + 1
        );
    }
    assert!(resolved[2].entry.origin.is_none());

    // Export: 1 from Modrinth, 2 bundled, 3-5 listed with links.
    let out = tmp.path().join("o.mrpack");
    let mut p = pack.clone();
    p.mods = resolved.iter().map(|r| r.entry.clone()).collect();
    let r = write_mrpack(
        &p,
        &resolved,
        &out,
        &ExportOptions::default(),
        &mr,
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(r.matched_exact, 1);
    assert_eq!(r.matched_fuzzy, 0);
    assert_eq!(r.bundled, vec!["two-1.0.jar".to_string()]);
    assert_eq!(r.skipped.len(), 3);
    assert!(r
        .skipped
        .iter()
        .all(|s| s.link.is_some() && s.reason.starts_with("needs a manual download")));
}

#[tokio::test]
async fn a_download_whose_hash_was_unknown_can_still_turn_out_to_be_on_modrinth() {
    use mrimport_core::fetch::fetch_unmatched;
    let tmp = tempfile::tempdir().unwrap();
    let bytes = jar("surprise");
    let cdn = cdn(&[("s.jar", Some(bytes.clone()))]).await;
    let cf = MockServer::start().await;
    // Core API knows the name and URL but gives no hash
    Mock::given(method("POST")).and(path("/mods/files")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
        {"id": 11, "modId": 1, "fileName": "s-1.0.jar", "downloadUrl": format!("{}/files/s.jar", cdn.uri()), "fileLength": 5, "hashes": []}]}))).mount(&cf).await;
    Mock::given(method("POST"))
        .and(path("/mods"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [{"id": 1, "name": "S", "slug": "s"}]})),
        )
        .mount(&cf)
        .await;
    let pack = providers::load(
        &cf_pack(tmp.path(), &[(1, 11)]),
        &LoadOptions::default(),
        &ctx(Lookup::new(
            Some(CurseForge::with_base(cf.uri(), KEY)),
            false,
        )),
    )
    .await
    .unwrap();
    let mr_server = ModrinthMock {
        known: [(sha1(&bytes), proj("PS", "s", "S"))].into(),
        search: vec![],
    }
    .start()
    .await;
    let mr = Modrinth::with_base(mr_server.uri(), None);
    let mut resolved = resolve_with(&pack, &mr, &Reporter::silent(), ResolveOptions::default())
        .await
        .unwrap();
    assert!(
        matches!(resolved[0].resolution, Resolution::Unmatched { .. }),
        "no hash yet, so not matched"
    );
    let rep = fetch_unmatched(
        &mut resolved,
        &tmp.path().join("dl"),
        &mr,
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(rep.found_on_modrinth, 1);
    assert!(matches!(
        &resolved[0].resolution,
        Resolution::Matched {
            confidence: Confidence::Exact,
            ..
        }
    ));
}

#[tokio::test]
async fn keyless_downloads_come_from_the_mirror_and_are_verified_against_the_hashed_jar() {
    use mrimport_core::fetch::fetch_unmatched;
    let tmp = tempfile::tempdir().unwrap();
    let j = jar("mirror-mod");
    let m = maven(&[(7, 77, j.clone())], None).await;
    let w = widget(
        &[(
            7,
            "Mirror Mod",
            "https://www.curseforge.com/minecraft/mc-mods/mirror-mod",
            vec![(77, "mirror-mod-1.2.jar")],
        )],
        None,
    )
    .await;
    let lookup = Lookup::new(None, true).with_bases(bases(&m, &w));
    let pack = providers::load(
        &cf_pack(tmp.path(), &[(7, 77)]),
        &LoadOptions::default(),
        &ctx(lookup.clone()),
    )
    .await
    .unwrap();
    let mr_server = ModrinthMock {
        known: Default::default(),
        search: vec![],
    }
    .start()
    .await;
    let mr = Modrinth::with_base(mr_server.uri(), None);
    let mut resolved = resolve_with(
        &pack,
        &mr,
        &Reporter::silent(),
        ResolveOptions {
            enricher: Some(&lookup),
            fuzzy: false,
        },
    )
    .await
    .unwrap();
    // the widget supplied the real file name even though the jar came from the mirror
    assert_eq!(resolved[0].entry.file_name, "mirror-mod-1.2.jar");
    let rep = fetch_unmatched(
        &mut resolved,
        &tmp.path().join("dl"),
        &mr,
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(rep.fetched, 1);
    let Some(mrimport_core::model::Origin::File { path }) = &resolved[0].entry.origin else {
        panic!("not downloaded")
    };
    assert_eq!(std::fs::read(path).unwrap(), j);
}

#[tokio::test]
async fn an_html_error_page_is_never_bundled_as_a_mod() {
    use mrimport_core::fetch::fetch_unmatched;
    let tmp = tempfile::tempdir().unwrap();
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/files/x.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>Access denied</html>"))
        .mount(&s)
        .await;
    let mut e = mrimport_core::model::ModEntry::new("x.jar");
    e.cf = Some(mrimport_core::model::CfRef {
        project_id: 1,
        file_id: 2,
    });
    e.download_url = Some(format!("{}/files/x.jar", s.uri()));
    let mut resolved = vec![mrimport_core::resolve::ResolvedMod {
        entry: e,
        resolution: Resolution::Unmatched {
            reason: "not on Modrinth".into(),
        },
    }];
    let mr_server = ModrinthMock {
        known: Default::default(),
        search: vec![],
    }
    .start()
    .await;
    let rep = fetch_unmatched(
        &mut resolved,
        &tmp.path().join("dl"),
        &Modrinth::with_base(mr_server.uri(), None),
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(rep.fetched, 0);
    assert!(
        rep.failed[0].1.contains("did not return a jar"),
        "{:?}",
        rep.failed
    );
    assert!(resolved[0].entry.origin.is_none());
    assert!(manual::needs_manual(&resolved[0]), "it now awaits the user");
    assert!(
        std::fs::read_dir(tmp.path().join("dl"))
            .unwrap()
            .next()
            .is_none(),
        "no partial file is left behind"
    );
}
