use mrimport_core::events::Reporter;
use mrimport_core::export::mrpack::{write_mrpack, ExportOptions};
use mrimport_core::model::*;
use mrimport_core::modrinth::Modrinth;
use mrimport_core::providers::{self, LoadCtx, LoadOptions};
use mrimport_core::resolve::{resolve, Resolution};
use std::io::Write;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fake_jar(dir: &std::path::Path, name: &str, fabric_id: &str) -> Vec<u8> {
    let p = dir.join(name);
    let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
    z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    z.write_all(format!(r#"{{"id":"{fabric_id}","name":"{fabric_id} mod"}}"#).as_bytes())
        .unwrap();
    z.finish().unwrap();
    std::fs::read(p).unwrap()
}

#[tokio::test]
async fn prism_instance_to_mrpack() {
    let tmp = tempfile::tempdir().unwrap();
    let inst = tmp.path().join("My Pack");
    std::fs::create_dir_all(inst.join(".minecraft/mods")).unwrap();
    std::fs::create_dir_all(inst.join(".minecraft/config")).unwrap();
    std::fs::write(inst.join("instance.cfg"), "name=Cool Pack\n").unwrap();
    std::fs::write(
        inst.join("mmc-pack.json"),
        r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.15.0"}]}"#,
    )
    .unwrap();
    std::fs::write(inst.join(".minecraft/config/a.toml"), "x=1").unwrap();
    let known = fake_jar(&inst.join(".minecraft/mods"), "known-1.0.jar", "known");
    fake_jar(&inst.join(".minecraft/mods"), "custom-1.0.jar", "custom");

    assert_eq!(providers::detect(&inst).unwrap(), SourceKind::Prism);
    let pack = providers::load(&inst, &LoadOptions::default(), &LoadCtx::new(None))
        .await
        .unwrap();
    assert_eq!(pack.name, "Cool Pack");
    assert_eq!(pack.loader, Loader::Fabric);
    assert_eq!(pack.mods.len(), 2);
    assert_eq!(pack.overrides.len(), 1);

    let server = MockServer::start().await;
    let sha1 = mrimport_core::hash::sha1_hex(&known);
    Mock::given(method("POST")).and(path("/version_files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            sha1.clone(): {"id":"v1","project_id":"P1","name":"Known","version_number":"1.0","files":[
                {"hashes":{"sha1":sha1,"sha512":"ab"},"url":"https://cdn.modrinth.com/data/P1/versions/v1/known-1.0.jar","filename":"known-1.0.jar","primary":true,"size":10}]}
        })))
        .mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([{"id":"P1","slug":"known","title":"Known"}])),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"hits":[]})))
        .mount(&server)
        .await;

    let mr = Modrinth::with_base(server.uri(), None);
    let resolved = resolve(&pack, &mr, &Reporter::silent()).await.unwrap();
    assert!(matches!(
        resolved
            .iter()
            .find(|r| r.entry.file_name == "known-1.0.jar")
            .unwrap()
            .resolution,
        Resolution::Matched { .. }
    ));

    let out = tmp.path().join("out.mrpack");
    let opts = ExportOptions {
        loader_version: None,
        ..Default::default()
    };
    let rep = write_mrpack(&pack, &resolved, &out, &opts, &mr, &Reporter::silent())
        .await
        .unwrap();
    assert_eq!(rep.matched_exact, 1);
    assert_eq!(rep.bundled, vec!["custom-1.0.jar".to_string()]);
    assert_eq!(rep.overrides, 1);

    let mut z = zip::ZipArchive::new(std::fs::File::open(out).unwrap()).unwrap();
    let idx: serde_json::Value =
        serde_json::from_reader(z.by_name("modrinth.index.json").unwrap()).unwrap();
    assert_eq!(idx["dependencies"]["minecraft"], "1.20.1");
    assert_eq!(idx["dependencies"]["fabric-loader"], "0.15.0");
    assert_eq!(idx["files"][0]["path"], "mods/known-1.0.jar");
    assert!(z.by_name("overrides/mods/custom-1.0.jar").is_ok());
    assert!(z.by_name("overrides/config/a.toml").is_ok());
}

#[tokio::test]
async fn bare_mods_folder_needs_mc_version() {
    let tmp = tempfile::tempdir().unwrap();
    fake_jar(tmp.path(), "a.jar", "a");
    let err = providers::load(tmp.path(), &LoadOptions::default(), &LoadCtx::new(None))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Minecraft version"));
    let opts = LoadOptions {
        mc_version: Some("1.21".into()),
        ..Default::default()
    };
    let pack = providers::load(tmp.path(), &opts, &LoadCtx::new(None))
        .await
        .unwrap();
    assert_eq!(pack.loader, Loader::Fabric);
}
