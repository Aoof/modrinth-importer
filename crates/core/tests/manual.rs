mod common;

use common::*;
use mrimport_core::events::Reporter;
use mrimport_core::export::mrpack::{write_mrpack, ExportOptions};
use mrimport_core::manual::{apply_files, collect_jars, needs_manual, ManualOptions, Verdict};
use mrimport_core::model::*;
use mrimport_core::modrinth::Modrinth;
use mrimport_core::resolve::{Resolution, ResolvedMod};
use std::path::PathBuf;

fn waiting(project: u64, file: u64, expected: Option<&str>, sha1: Option<String>) -> ResolvedMod {
    let mut e = ModEntry::new(
        expected
            .map(String::from)
            .unwrap_or_else(|| format!("cf-{project}-{file}.jar")),
    );
    e.cf = Some(CfRef {
        project_id: project,
        file_id: file,
    });
    e.expected_file_name = expected.map(String::from);
    e.sha1 = sha1;
    e.name_hint = Some(format!("Mod {project}"));
    ResolvedMod {
        entry: e,
        resolution: Resolution::Unmatched {
            reason: "not found".into(),
        },
    }
}

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

async fn no_modrinth_matches() -> (wiremock::MockServer, Modrinth) {
    let s = ModrinthMock {
        known: Default::default(),
        search: vec![],
    }
    .start()
    .await;
    let mr = Modrinth::with_base(s.uri(), None);
    (s, mr)
}

fn pack() -> ImportedPack {
    ImportedPack {
        source: SourceKind::CurseForge,
        name: "T".into(),
        version: "1".into(),
        summary: None,
        mc_version: "1.20.1".into(),
        loader: Loader::Fabric,
        loader_version: Some("0.15.0".into()),
        mods: vec![],
        overrides: vec![],
    }
}

#[tokio::test]
async fn verified_file_is_accepted_bundled_and_exported() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let mut list = vec![waiting(5, 55, Some("five-2.3.jar"), None)];
    assert!(needs_manual(&list[0]));

    // the browser saved it as "five-2.3 (1).jar": still the exact file
    let f = write(tmp.path(), "five-2.3 (1).jar", &jar("five"));
    let res = apply_files(
        &mut list,
        &[(f, None, false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert_eq!(res.len(), 1);
    assert!(res[0].applied && !res[0].on_modrinth, "{:?}", res[0]);
    assert_eq!(res[0].verdict, Some(Verdict::ExactFileName));
    assert!(!needs_manual(&list[0]), "it no longer needs a manual file");
    assert_eq!(
        list[0].entry.file_name, "five-2.3.jar",
        "stored under the exact expected name"
    );

    let out = tmp.path().join("o.mrpack");
    let mut p = pack();
    p.mods = vec![list[0].entry.clone()];
    let rep = write_mrpack(
        &p,
        &list,
        &out,
        &ExportOptions::default(),
        &mr,
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(rep.bundled, vec!["five-2.3.jar".to_string()]);
    assert!(rep.skipped.is_empty());
    let mut z = zip::ZipArchive::new(std::fs::File::open(out).unwrap()).unwrap();
    assert!(z.by_name("overrides/mods/five-2.3.jar").is_ok());
}

#[tokio::test]
async fn a_different_version_is_rejected_unless_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let mut list = vec![waiting(5, 55, Some("five-2.3.jar"), None)];
    let f = write(tmp.path(), "five-2.4.jar", &jar("five"));

    let res = apply_files(
        &mut list,
        &[(f.clone(), Some(0), false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert!(!res[0].applied);
    assert!(matches!(res[0].verdict, Some(Verdict::Mismatch { .. })));
    assert!(res[0].message.contains("five-2.3.jar") && res[0].message.contains("five-2.4.jar"));
    assert!(needs_manual(&list[0]), "still waiting");
    assert!(list[0].entry.origin.is_none(), "nothing was changed");

    let res = apply_files(
        &mut list,
        &[(f, Some(0), false)],
        &mr,
        ManualOptions {
            force: true,
            ..Default::default()
        },
    )
    .await;
    assert!(res[0].applied && res[0].message.starts_with("accepted anyway"));
}

#[tokio::test]
async fn unverifiable_files_need_an_explicit_accept() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let mut list = vec![waiting(4, 44, None, None)];
    let f = write(tmp.path(), "anything.jar", &jar("four"));

    let res = apply_files(
        &mut list,
        &[(f.clone(), Some(0), false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert!(!res[0].applied);
    assert!(matches!(res[0].verdict, Some(Verdict::Unverified { .. })));
    assert!(needs_manual(&list[0]));

    let res = apply_files(
        &mut list,
        &[(f, Some(0), false)],
        &mr,
        ManualOptions {
            accept_unverified: true,
            ..Default::default()
        },
    )
    .await;
    assert!(res[0].applied && res[0].message.contains("accepted unverified"));
    assert!(!needs_manual(&list[0]));
}

#[tokio::test]
async fn hash_is_checked_exactly_and_matches_regardless_of_file_name() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let (right, wrong) = (jar("same-name"), jar("other-build"));
    let mut list = vec![waiting(2, 22, Some("two-1.0.jar"), Some(sha1(&right)))];

    // same name as expected but different bytes -> a different build: refused
    let bad = write(tmp.path(), "two-1.0.jar", &wrong);
    let res = apply_files(
        &mut list,
        &[(bad, Some(0), false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert!(
        !res[0].applied && res[0].message.contains("SHA-1"),
        "{:?}",
        res[0]
    );

    // right bytes under a random name, no explicit target -> found by hash alone
    let good = write(tmp.path(), "download-123.jar", &right);
    let res = apply_files(
        &mut list,
        &[(good, None, false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert!(res[0].applied);
    assert_eq!(res[0].verdict, Some(Verdict::ExactHash));
    assert_eq!(res[0].index, Some(0));
}

#[tokio::test]
async fn a_supplied_file_that_is_on_modrinth_becomes_an_exact_modrinth_match() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = jar("on-modrinth");
    let s = ModrinthMock {
        known: [(sha1(&bytes), proj("PM", "on-modrinth", "On Modrinth"))].into(),
        search: vec![],
    }
    .start()
    .await;
    let mr = Modrinth::with_base(s.uri(), None);
    let mut list = vec![waiting(8, 88, Some("m.jar"), None)];
    let f = write(tmp.path(), "m.jar", &bytes);
    let res = apply_files(
        &mut list,
        &[(f, None, false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert!(res[0].applied && res[0].on_modrinth);
    match &list[0].resolution {
        Resolution::Matched {
            confidence,
            candidate,
        } => {
            assert_eq!(*confidence, mrimport_core::resolve::Confidence::Exact);
            assert_eq!(candidate.slug, "on-modrinth");
        }
        r => panic!("{r:?}"),
    }
}

#[tokio::test]
async fn scanning_a_folder_applies_only_what_belongs_and_ignores_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let mut list = vec![
        waiting(5, 55, Some("five-2.3.jar"), None),
        waiting(6, 66, Some("six-1.0.jar"), None),
    ];
    write(tmp.path(), "five-2.3.jar", &jar("five"));
    write(tmp.path(), "unrelated-holiday-photos.jar", &jar("zzz"));
    write(tmp.path(), "notes.txt", b"not a jar");
    write(tmp.path(), "six-9.9.jar", &jar("six")); // wrong version of a wanted mod: not matched by name

    let files = collect_jars(&[tmp.path().to_path_buf()]);
    assert_eq!(files.len(), 3, "only .jar files are collected");
    let triples: Vec<_> = files.into_iter().map(|(p, d)| (p, None, d)).collect();
    let res = apply_files(&mut list, &triples, &mr, ManualOptions::default()).await;

    assert_eq!(
        res.len(),
        1,
        "unrelated files from a folder scan are skipped silently: {res:?}"
    );
    assert!(res[0].applied && res[0].file_name == "five-2.3.jar");
    assert!(!needs_manual(&list[0]) && needs_manual(&list[1]));

    // the same wrong-version file given explicitly (not from a folder) is reported
    let explicit = apply_files(
        &mut list,
        &[(tmp.path().join("six-9.9.jar"), None, false)],
        &mr,
        ManualOptions::default(),
    )
    .await;
    assert_eq!(explicit.len(), 1);
    assert!(!explicit[0].applied && explicit[0].message.contains("doesn't match any mod"));
}

#[tokio::test]
async fn export_report_labels_what_is_missing_with_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    let mut a = waiting(5, 55, Some("five-2.3.jar"), None);
    a.entry.cf_url = Some("https://www.curseforge.com/minecraft/mc-mods/five".into());
    let b = waiting(4, 44, None, None);
    let list = vec![a, b];
    let mut p = pack();
    p.mods = list.iter().map(|r| r.entry.clone()).collect();

    let rep = write_mrpack(
        &p,
        &list,
        &tmp.path().join("o.mrpack"),
        &ExportOptions::default(),
        &mr,
        &Reporter::silent(),
    )
    .await
    .unwrap();
    assert_eq!(rep.skipped.len(), 2);
    assert!(rep
        .skipped
        .iter()
        .all(|s| s.reason.starts_with("needs a manual download")));
    assert_eq!(
        rep.skipped[0].link.as_deref(),
        Some("https://www.curseforge.com/minecraft/mc-mods/five/files/55")
    );
    assert_eq!(rep.skipped[0].file_name, "five-2.3.jar");
    assert_eq!(
        rep.skipped[1].link.as_deref(),
        Some("https://www.curseforge.com/projects/4")
    );
    assert_eq!(
        rep.skipped[1].file_name, "Mod 4",
        "falls back to the project name, not the cf-4-44.jar placeholder"
    );
    let md = rep.to_markdown();
    assert!(
        md.contains("[download page](https://www.curseforge.com/projects/4)"),
        "{md}"
    );
}

#[tokio::test]
async fn jar_metadata_can_pair_a_file_with_a_mod_but_never_verifies_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (_s, mr) = no_modrinth_matches().await;
    // we know the slug but neither a hash nor an exact file name
    let mut known = waiting(9, 99, None, None);
    known.entry.slug_hint = Some("five".into());
    // we know nothing at all
    let blank = waiting(10, 100, None, None);
    let mut list = vec![known, blank];

    write(tmp.path(), "download.jar", &jar("five")); // fabric id "five" matches the slug
    write(tmp.path(), "other.jar", &jar("something-else"));
    let files: Vec<_> = collect_jars(&[tmp.path().to_path_buf()])
        .into_iter()
        .map(|(p, d)| (p, None, d))
        .collect();

    let res = apply_files(&mut list, &files, &mr, ManualOptions::default()).await;
    assert_eq!(
        res.len(),
        1,
        "only the jar whose id matches is paired: {res:?}"
    );
    assert_eq!(res[0].index, Some(0));
    assert!(
        !res[0].applied,
        "paired by metadata is never trusted as exact"
    );
    assert!(matches!(res[0].verdict, Some(Verdict::Unverified { .. })));
    assert!(needs_manual(&list[0]) && needs_manual(&list[1]));

    let res = apply_files(
        &mut list,
        &files,
        &mr,
        ManualOptions {
            accept_unverified: true,
            ..Default::default()
        },
    )
    .await;
    assert!(res[0].applied);
    assert!(!needs_manual(&list[0]));
    assert!(
        needs_manual(&list[1]),
        "the mod we know nothing about is never auto-paired"
    );
}
