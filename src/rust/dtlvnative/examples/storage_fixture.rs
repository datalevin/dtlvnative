//! Counterpart to tests/storage_fixture.c; run through script/test-rust.
use dtlvnative::dlmdb::{DatabaseOptions, Environment, EnvironmentOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    assert!(args.len() == 3 && (args[1] == "write" || args[1] == "read"));
    let writing = args[1] == "write";
    // SAFETY: script/test-rust owns this fixture directory, runs one process at
    // a time, and deletes it only after both native and Rust handles are closed.
    let env = unsafe { Environment::open(&args[2], EnvironmentOptions::default())? };
    let options = DatabaseOptions {
        counted: true,
        prefix_compression: true,
        ..Default::default()
    };
    let (plain, duplicates) = if writing {
        (
            env.create_database("plain", options)?,
            env.create_database(
                "duplicates",
                DatabaseOptions {
                    duplicates: true,
                    fixed_duplicates: true,
                    ..options
                },
            )?,
        )
    } else {
        (
            env.open_database("plain")?,
            env.open_database("duplicates")?,
        )
    };
    if writing {
        let mut txn = env.write_txn()?;
        for i in 0..128 {
            let key = format!("shared-prefix-{i:04}");
            txn.put(&plain, key.as_bytes(), format!("value-{i:04}").as_bytes())?;
            for j in 0..3 {
                txn.put(
                    &duplicates,
                    key.as_bytes(),
                    format!("dup-{j:04}").as_bytes(),
                )?;
            }
        }
        txn.commit()?;
    } else {
        let mut txn = env.read_txn()?;
        for i in 0..128 {
            assert_eq!(
                txn.get(&plain, format!("shared-prefix-{i:04}").as_bytes())?,
                Some(format!("value-{i:04}").into_bytes())
            );
        }
        {
            let mut cursor = txn.cursor(&plain)?;
            for i in 0..128 {
                let entry = cursor.next_entry()?.unwrap();
                assert_eq!(entry.key, format!("shared-prefix-{i:04}").as_bytes());
                assert_eq!(entry.value, format!("value-{i:04}").as_bytes());
            }
            assert!(cursor.next_entry()?.is_none());
        }
        let mut cursor = txn.cursor(&duplicates)?;
        for i in 0..128 {
            for j in 0..3 {
                let entry = cursor.next_entry()?.unwrap();
                assert_eq!(entry.key, format!("shared-prefix-{i:04}").as_bytes());
                assert_eq!(entry.value, format!("dup-{j:04}").as_bytes());
            }
        }
        assert!(cursor.next_entry()?.is_none());
        for i in 0..128 {
            assert_eq!(
                cursor.value_count(format!("shared-prefix-{i:04}").as_bytes())?,
                3
            );
        }
    }
    Ok(())
}
