//! `purge_local_data` must remove only this app's directories. Runs against a
//! throwaway HOME so nothing real is touched.
#![cfg(unix)]

use mrimport_core::config::{local_data_paths, purge_local_data};

#[test]
fn removes_only_our_directories() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_CACHE_HOME");

    let base = directories::BaseDirs::new().unwrap();
    assert_eq!(base.home_dir(), home.path(), "HOME override not honoured");

    let ours = [
        base.data_dir().join("com.aoof.modrinthimporter"),
        base.cache_dir().join("com.aoof.modrinthimporter"),
    ];
    let other = base.data_dir().join("some-other-app");
    for d in ours.iter().chain([&other]) {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("f"), "x").unwrap();
    }

    assert!(!local_data_paths().is_empty());
    let removed = purge_local_data().unwrap();
    assert!(!removed.is_empty());
    for d in &ours {
        assert!(!d.exists(), "{} should be gone", d.display());
    }
    assert!(other.join("f").exists(), "unrelated data must survive");
    assert!(local_data_paths().is_empty());
}
