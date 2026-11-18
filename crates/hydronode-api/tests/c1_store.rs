//! C1: config, profiles, credential store, refresh lock.

use hydronode_api::config::{check_server, Config, Profile};
use hydronode_api::credentials::{Credential, CredentialStore, StoreKind};
use hydronode_api::Error;

#[test]
fn config_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.profiles.insert(
        "int".into(),
        Profile {
            server: "https://int.example.com".into(),
        },
    );
    config.default_profile = Some("int".into());
    config.save(&path).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[profiles.int]"), "{text}");
    assert!(!text.to_lowercase().contains("token"));

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded, config);
}

#[test]
fn missing_config_gives_default_profile() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::load(&dir.path().join("none.toml")).unwrap();
    assert_eq!(config.active_name(None), "default");
    assert_eq!(
        config.profile("default").unwrap().server,
        "https://hydronode.tech"
    );
}

#[test]
fn unknown_profile_is_usage_error() {
    let config = Config::default();
    let err = config.profile("nope").unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn https_enforced_except_loopback() {
    assert!(check_server("https://hydronode.example.com").is_ok());
    assert!(check_server("http://localhost:8080").is_ok());
    assert!(check_server("http://127.0.0.1:8080/").is_ok());
    assert!(check_server("http://[::1]:8080").is_ok());
    for bad in [
        "http://hydronode.example.com",
        "http://127.0.0.1.example.com",
        "ftp://example.com",
        "not a url",
        "https://user@example.com",
    ] {
        let err = check_server(bad).unwrap_err();
        assert!(matches!(err, Error::InsecureServer(_)), "{bad}: {err:?}");
    }
    // trailing slash and path are dropped
    assert_eq!(
        check_server("https://example.com/").unwrap().as_str(),
        "https://example.com/"
    );
}

#[test]
fn file_store_round_trip_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(dir.path(), StoreKind::File);
    assert!(store.get("default").unwrap().is_none());

    let cred = Credential::Session {
        access: "eyJ.fake.jwt".into(),
        refresh: "hrt_fake_refresh".into(),
        expires_at: 1_900_000_000,
    };
    store.set("default", &cred).unwrap();
    store
        .set(
            "int",
            &Credential::Pat {
                token: "hrt_fake_read".into(),
            },
        )
        .unwrap();
    assert_eq!(store.get("default").unwrap(), Some(cred));
    assert!(store.get("int").unwrap().unwrap().is_read_only());

    store.delete("default").unwrap();
    assert!(store.get("default").unwrap().is_none());
    assert!(store.get("int").unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn file_store_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(dir.path(), StoreKind::File);
    store
        .set(
            "default",
            &Credential::Pat {
                token: "hat_fake".into(),
            },
        )
        .unwrap();
    let mode = std::fs::metadata(dir.path().join("credentials.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn credential_kinds() {
    assert!(Credential::Pat {
        token: "hrt_x".into()
    }
    .is_read_only());
    assert!(!Credential::Pat {
        token: "hat_x".into()
    }
    .is_read_only());
    let session = Credential::Session {
        access: "a".into(),
        refresh: "hrt_r".into(),
        expires_at: 0,
    };
    assert!(!session.is_read_only());
    // Debug never shows the secret part
    let debug = format!("{session:?}");
    assert!(!debug.contains("hrt_r"), "{debug}");
}

#[test]
fn refresh_lock_is_exclusive_and_rereads() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CredentialStore::new(dir.path(), StoreKind::File));
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut threads = Vec::new();
    for i in 0..2 {
        let store = store.clone();
        let order = order.clone();
        threads.push(std::thread::spawn(move || {
            store
                .with_lock_blocking("default", || {
                    order.lock().unwrap().push(format!("start{i}"));
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    order.lock().unwrap().push(format!("end{i}"));
                })
                .unwrap();
        }));
    }
    for t in threads {
        t.join().unwrap();
    }
    let order = order.lock().unwrap();
    // never interleaved
    assert!(order[0].starts_with("start") && order[1].starts_with("end"));
    assert_eq!(order[0][5..], order[1][3..]);
}
