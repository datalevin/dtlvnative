fn main() -> Result<(), Box<dyn std::error::Error>> {
    use dtlvnative::dlmdb::{DatabaseOptions, Environment, EnvironmentOptions};

    let directory = tempfile::tempdir()?;
    // SAFETY: this example exclusively owns a fresh directory and drops every
    // database resource before the temporary directory is deleted.
    let env = unsafe { Environment::open(directory.path(), EnvironmentOptions::default())? };
    let db = env.create_database("example", DatabaseOptions::default())?;
    let mut writer = env.write_txn()?;
    writer.put(&db, b"hello", b"Rust")?;
    writer.commit()?;

    let mut reader = env.read_txn()?;
    assert_eq!(reader.get(&db, b"hello")?, Some(b"Rust".to_vec()));
    let mut cursor = reader.cursor(&db)?;
    while let Some(entry) = cursor.next_entry()? {
        println!(
            "{}: {}",
            String::from_utf8_lossy(entry.key),
            String::from_utf8_lossy(entry.value)
        );
    }
    Ok(())
}
