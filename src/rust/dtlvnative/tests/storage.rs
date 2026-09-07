use dtlvnative::{dlmdb::*, sys};

fn environment() -> (tempfile::TempDir, Environment) {
    let directory = tempfile::tempdir().unwrap();
    // SAFETY: fresh directory with no other openers; tests drop all native
    // resources before the directory is removed.
    let env =
        unsafe { Environment::open(directory.path(), EnvironmentOptions::default()) }.unwrap();
    (directory, env)
}

#[test]
fn commit_abort_delete_and_reopen() {
    let (directory, env) = environment();
    let db = env
        .create_database("records", DatabaseOptions::default())
        .unwrap();
    {
        let mut txn = env.write_txn().unwrap();
        txn.put(&db, b"a", b"committed").unwrap();
        txn.put(&db, b"empty", b"").unwrap();
        txn.commit().unwrap();
    }
    {
        let mut txn = env.write_txn().unwrap();
        txn.put(&db, b"a", b"rolled back").unwrap();
        txn.put(&db, b"b", b"rolled back").unwrap();
    }
    let mut txn = env.read_txn().unwrap();
    assert_eq!(txn.get(&db, b"a").unwrap(), Some(b"committed".to_vec()));
    assert_eq!(txn.get(&db, b"b").unwrap(), None);
    assert_eq!(txn.get(&db, b"empty").unwrap(), Some(vec![]));
    drop(txn);
    let mut txn = env.write_txn().unwrap();
    assert!(!txn.delete(&db, b"missing").unwrap());
    assert!(txn.delete(&db, b"empty").unwrap());
    txn.commit().unwrap();
    drop(db);
    drop(env);
    // SAFETY: all previous handles are dropped and the directory is still owned.
    let env =
        unsafe { Environment::open(directory.path(), EnvironmentOptions::default()) }.unwrap();
    let db = env.open_database("records").unwrap();
    let mut txn = env.read_txn().unwrap();
    assert_eq!(txn.get(&db, b"a").unwrap(), Some(b"committed".to_vec()));
    assert_eq!(txn.get(&db, b"empty").unwrap(), None);
}

#[test]
fn compressed_cursor_keys_and_seek() {
    let (_directory, env) = environment();
    let db = env
        .create_database(
            "compressed",
            DatabaseOptions {
                counted: true,
                prefix_compression: true,
                ..Default::default()
            },
        )
        .unwrap();
    let mut txn = env.write_txn().unwrap();
    for i in 0..1000 {
        txn.put(
            &db,
            format!("a-long-shared-prefix-{i:04}").as_bytes(),
            b"value",
        )
        .unwrap();
    }
    txn.commit().unwrap();
    let mut txn = env.read_txn().unwrap();
    let mut cursor = txn.cursor(&db).unwrap();
    for i in 0..1000 {
        let entry = cursor.next_entry().unwrap().unwrap();
        assert_eq!(entry.key, format!("a-long-shared-prefix-{i:04}").as_bytes());
        assert_eq!(entry.value, b"value");
    }
    assert!(cursor.next_entry().unwrap().is_none());
    let retained = cursor.first().unwrap().unwrap().key.to_vec();
    assert_eq!(
        cursor
            .seek(b"a-long-shared-prefix-0420")
            .unwrap()
            .unwrap()
            .key,
        b"a-long-shared-prefix-0420"
    );
    assert_eq!(
        cursor
            .seek(b"a-long-shared-prefix-0420x")
            .unwrap()
            .unwrap()
            .key,
        b"a-long-shared-prefix-0421"
    );
    assert_eq!(retained, b"a-long-shared-prefix-0000");
    assert!(cursor.seek(b"z").unwrap().is_none());
}

#[test]
fn duplicate_counts_and_errors_are_distinct() {
    let (_directory, env) = environment();
    let db = env
        .create_database(
            "duplicates",
            DatabaseOptions {
                duplicates: true,
                fixed_duplicates: true,
                counted: true,
                prefix_compression: true,
            },
        )
        .unwrap();
    let plain = env
        .create_database("plain", DatabaseOptions::default())
        .unwrap();
    let mut txn = env.write_txn().unwrap();
    txn.put(&plain, b"k", b"v").unwrap();
    for value in [b"03", b"01", b"02"] {
        txn.put(&db, b"k", value).unwrap();
    }
    txn.commit().unwrap();
    let mut txn = env.read_txn().unwrap();
    {
        let mut cursor = txn.cursor(&db).unwrap();
        assert_eq!(cursor.value_count(b"missing").unwrap(), 0);
        assert_eq!(cursor.value_count(b"k").unwrap(), 3);
        assert_eq!(cursor.first().unwrap().unwrap().value, b"01");
        assert_eq!(cursor.next_entry().unwrap().unwrap().value, b"02");
        assert_eq!(cursor.next_entry().unwrap().unwrap().value, b"03");
        assert!(cursor.next_entry().unwrap().is_none());
    }
    let error = txn.cursor(&plain).unwrap().value_count(b"k").unwrap_err();
    assert!(matches!(error, Error::Native(sys::MDB_INCOMPATIBLE)));
}

#[test]
fn resources_retain_environment_and_reject_foreign_database() {
    let (_directory, env) = environment();
    let (_other_directory, other) = environment();
    let db = env
        .create_database("owned", DatabaseOptions::default())
        .unwrap();
    let mut wrong = other.write_txn().unwrap();
    assert!(matches!(
        wrong.get(&db, b"key"),
        Err(Error::WrongEnvironment)
    ));
    assert!(matches!(
        wrong.put(&db, b"key", b"value"),
        Err(Error::WrongEnvironment)
    ));
    assert!(matches!(wrong.cursor(&db), Err(Error::WrongEnvironment)));
    let mut txn = env.write_txn().unwrap();
    drop(env);
    txn.put(&db, b"key", b"value").unwrap();
    drop(db);
    txn.commit().unwrap();
}

#[test]
fn single_writer_guard_and_reader_snapshot() {
    let (_directory, env) = environment();
    let db = env
        .create_database("snapshot", DatabaseOptions::default())
        .unwrap();
    let mut reader = env.read_txn().unwrap();
    let mut writer = env.write_txn().unwrap();
    assert!(matches!(env.write_txn(), Err(Error::WriterActive)));
    writer.put(&db, b"key", b"new").unwrap();
    writer.commit().unwrap();
    assert_eq!(reader.get(&db, b"key").unwrap(), None);
    assert_eq!(
        env.read_txn().unwrap().get(&db, b"key").unwrap(),
        Some(b"new".to_vec())
    );
    env.write_txn().unwrap().abort();
    drop(env.write_txn().unwrap());
    env.write_txn().unwrap().commit().unwrap();
}

#[test]
fn map_full_failed_commit_releases_writer() {
    let directory = tempfile::tempdir().unwrap();
    // SAFETY: fresh, exclusively owned directory; all handles drop before cleanup.
    let env = unsafe {
        Environment::open(
            directory.path(),
            EnvironmentOptions {
                map_size: 1024 * 1024,
                ..Default::default()
            },
        )
    }
    .unwrap();
    let db = env
        .create_database("small", DatabaseOptions::default())
        .unwrap();
    let mut txn = env.write_txn().unwrap();
    assert!(matches!(
        txn.put(&db, b"huge", &vec![0; 2 * 1024 * 1024]),
        Err(Error::Native(sys::MDB_MAP_FULL))
    ));
    assert!(txn.commit().is_err());
    let mut txn = env.write_txn().unwrap();
    txn.put(&db, b"small", b"ok").unwrap();
    txn.commit().unwrap();
    assert_eq!(env.read_txn().unwrap().get(&db, b"huge").unwrap(), None);
}

#[test]
fn invalid_options_and_missing_database() {
    let (_directory, env) = environment();
    assert!(matches!(
        env.open_database("missing"),
        Err(Error::Native(sys::MDB_NOTFOUND))
    ));
    assert!(matches!(
        env.create_database(
            "invalid",
            DatabaseOptions {
                fixed_duplicates: true,
                ..Default::default()
            }
        ),
        Err(Error::InvalidOptions(_))
    ));
    assert!(matches!(
        env.open_database("with\0nul"),
        Err(Error::InteriorNul(_))
    ));
}
