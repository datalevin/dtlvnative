//! Byte-oriented access to the DLMDB fork bundled with dtlvnative.
//!
//! Handles are confined to their creating thread. Reads through a transaction
//! return owned bytes; cursor entries borrow the cursor because compressed keys
//! can live in scratch memory that the next cursor operation replaces.
//!
//! A cursor entry cannot survive an advance:
//! ```compile_fail
//! use dtlvnative::dlmdb::Cursor;
//! fn invalid(cursor: &mut Cursor<'_>) {
//!     let entry = cursor.next_entry().unwrap().unwrap();
//!     cursor.next_entry().unwrap();
//!     println!("{:?}", entry.key);
//! }
//! ```
//! A transaction cannot commit while its cursor is still in use:
//! ```compile_fail
//! use dtlvnative::dlmdb::{Database, WriteTransaction};
//! fn invalid(mut txn: WriteTransaction, db: &Database) {
//!     let mut cursor = txn.cursor(db).unwrap();
//!     txn.commit().unwrap();
//!     cursor.next_entry().unwrap();
//! }
//! ```

use crate::sys;
use std::{
    cell::Cell,
    ffi::{CStr, CString, NulError},
    fmt,
    marker::PhantomData,
    path::Path,
    ptr::{self, NonNull},
    rc::Rc,
    slice,
};

/// Native failures retain their original error code.
#[derive(Debug)]
pub enum Error {
    Native(i32),
    InvalidOptions(&'static str),
    InteriorNul(NulError),
    WrongEnvironment,
    WriterActive,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(code) => {
                // SAFETY: mdb_strerror accepts any error code and returns a C string.
                let message = unsafe { CStr::from_ptr(sys::mdb_strerror(*code)) };
                write!(f, "{} ({code})", message.to_string_lossy())
            }
            Self::InvalidOptions(message) => f.write_str(message),
            Self::InteriorNul(error) => error.fmt(f),
            Self::WrongEnvironment => f.write_str("database belongs to another environment"),
            Self::WriterActive => f.write_str("a write transaction is already active"),
        }
    }
}

impl std::error::Error for Error {}

impl From<NulError> for Error {
    fn from(error: NulError) -> Self {
        Self::InteriorNul(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn check(code: i32) -> Result<()> {
    if code == sys::MDB_SUCCESS as i32 {
        Ok(())
    } else {
        Err(Error::Native(code))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnvironmentOptions {
    pub map_size: usize,
    pub max_databases: u32,
    pub max_readers: u32,
}

impl Default for EnvironmentOptions {
    fn default() -> Self {
        Self {
            map_size: 64 * 1024 * 1024,
            max_databases: 32,
            max_readers: 126,
        }
    }
}

/// Flags for a newly created named database. Existing databases retain their flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct DatabaseOptions {
    pub duplicates: bool,
    pub fixed_duplicates: bool,
    pub counted: bool,
    pub prefix_compression: bool,
}

impl DatabaseOptions {
    fn flags(self) -> Result<u32> {
        if self.fixed_duplicates && !self.duplicates {
            return Err(Error::InvalidOptions(
                "fixed duplicates require duplicate sorting",
            ));
        }
        Ok(if self.duplicates { sys::MDB_DUPSORT } else { 0 }
            | if self.fixed_duplicates {
                sys::MDB_DUPFIXED
            } else {
                0
            }
            | if self.counted { sys::MDB_COUNTED } else { 0 }
            | if self.prefix_compression {
                sys::MDB_PREFIX_COMPRESSION
            } else {
                0
            })
    }
}

struct EnvironmentInner {
    ptr: NonNull<sys::MDB_env>,
    writer_active: Cell<bool>,
}

impl Drop for EnvironmentInner {
    fn drop(&mut self) {
        // SAFETY: this is the last owner; every transaction and database held an Rc.
        unsafe { sys::mdb_env_close(self.ptr.as_ptr()) };
    }
}

pub struct Environment {
    inner: Rc<EnvironmentInner>,
}

impl Environment {
    /// Open an environment in an existing directory, using MDB_NOTLS.
    ///
    /// # Safety
    /// The caller must ensure this process opens this environment only once,
    /// including through other wrappers or the raw API. Its files must contain
    /// valid DLMDB data and must not be deleted, replaced, truncated, or modified
    /// except by compatible DLMDB operations while any handle remains alive.
    /// External processes must follow DLMDB's locking and map-size rules. These
    /// requirements apply until all derived transactions and database handles
    /// have also been dropped. Raw calls must not invalidate managed resources.
    pub unsafe fn open(path: impl AsRef<Path>, options: EnvironmentOptions) -> Result<Self> {
        if options.map_size > isize::MAX as usize {
            return Err(Error::InvalidOptions(
                "map size exceeds Rust's maximum slice size",
            ));
        }
        if options.map_size == 0 || options.max_databases == 0 || options.max_readers == 0 {
            return Err(Error::InvalidOptions("environment limits must be nonzero"));
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            CString::new(path.as_ref().as_os_str().as_bytes())?
        };
        #[cfg(not(unix))]
        let path = CString::new(path.as_ref().to_str().ok_or(Error::InvalidOptions(
            "environment path must be UTF-8 on this platform",
        ))?)?;

        let mut raw = ptr::null_mut();
        // SAFETY: raw is an initialized, writable output slot.
        check(unsafe { sys::mdb_env_create(&mut raw) })?;
        let inner = Rc::new(EnvironmentInner {
            ptr: NonNull::new(raw).expect("mdb_env_create returned a null environment"),
            writer_active: Cell::new(false),
        });
        // SAFETY: this new environment is exclusively owned and not open yet.
        // The guard closes it on any failure. The path stays alive through open.
        unsafe {
            check(sys::mdb_env_set_mapsize(raw, options.map_size))?;
            check(sys::mdb_env_set_maxdbs(raw, options.max_databases))?;
            check(sys::mdb_env_set_maxreaders(raw, options.max_readers))?;
            check(sys::mdb_env_open(raw, path.as_ptr(), sys::MDB_NOTLS, 0o600))?;
        }
        Ok(Self { inner })
    }

    pub fn read_txn(&self) -> Result<ReadTransaction> {
        Transaction::begin(&self.inner, false).map(ReadTransaction)
    }

    /// A second simultaneous writer returns an error instead of blocking this thread.
    pub fn write_txn(&self) -> Result<WriteTransaction> {
        Transaction::begin(&self.inner, true).map(WriteTransaction)
    }

    pub fn create_database(&self, name: &str, options: DatabaseOptions) -> Result<Database> {
        self.database(name, options.flags()? | sys::MDB_CREATE, true)
    }

    pub fn open_database(&self, name: &str) -> Result<Database> {
        self.database(name, 0, false)
    }

    fn database(&self, name: &str, flags: u32, write: bool) -> Result<Database> {
        let name = CString::new(name)?;
        let txn = Transaction::begin(&self.inner, write)?;
        let mut dbi = 0;
        // SAFETY: the transaction is live and exclusive, and name/output are valid.
        check(unsafe { sys::mdb_dbi_open(txn.raw(), name.as_ptr(), flags, &mut dbi) })?;
        // Publish the handle only after commit. Aborted DBIs must never escape.
        txn.commit()?;
        Ok(Database {
            env: Rc::clone(&self.inner),
            dbi,
        })
    }
}

/// An environment-owned named database. Dropping it does not close the shared DBI.
#[derive(Clone)]
pub struct Database {
    env: Rc<EnvironmentInner>,
    dbi: sys::MDB_dbi,
}

struct Transaction {
    ptr: Option<NonNull<sys::MDB_txn>>,
    env: Rc<EnvironmentInner>,
    write: bool,
}

impl Transaction {
    fn begin(env: &Rc<EnvironmentInner>, write: bool) -> Result<Self> {
        if write && env.writer_active.replace(true) {
            return Err(Error::WriterActive);
        }
        let mut raw = ptr::null_mut();
        // SAFETY: env is live, transactions are thread confined, and this API
        // allows only one writer. MDB_NOTLS permits independent reader slots.
        let code = unsafe {
            sys::mdb_txn_begin(
                env.ptr.as_ptr(),
                ptr::null_mut(),
                if write { 0 } else { sys::MDB_RDONLY },
                &mut raw,
            )
        };
        if let Err(error) = check(code) {
            if write {
                env.writer_active.set(false);
            }
            return Err(error);
        }
        Ok(Self {
            ptr: Some(NonNull::new(raw).expect("mdb_txn_begin returned a null transaction")),
            env: Rc::clone(env),
            write,
        })
    }

    fn raw(&self) -> *mut sys::MDB_txn {
        self.ptr.expect("transaction already completed").as_ptr()
    }

    fn database(&self, db: &Database) -> Result<sys::MDB_dbi> {
        if Rc::ptr_eq(&self.env, &db.env) {
            Ok(db.dbi)
        } else {
            Err(Error::WrongEnvironment)
        }
    }

    fn commit(mut self) -> Result<()> {
        // Commit frees the native transaction even when it reports an error.
        let raw = self.ptr.take().unwrap();
        // SAFETY: exclusive ownership; the borrowing API prevents live cursors.
        let code = unsafe { sys::mdb_txn_commit(raw.as_ptr()) };
        if self.write {
            self.env.writer_active.set(false);
        }
        check(code)
    }

    fn get(&mut self, db: &Database, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let dbi = self.database(db)?;
        let mut key = input(key);
        let mut value = input(&[]);
        // SAFETY: live transaction/DBI; input and output holders are valid for the call.
        let code = unsafe { sys::mdb_get(self.raw(), dbi, &mut key, &mut value) };
        if code == sys::MDB_NOTFOUND {
            return Ok(None);
        }
        check(code)?;
        // SAFETY: successful get initializes value, and we copy before any native call.
        Ok(Some(unsafe { bytes(value) }.to_vec()))
    }

    fn cursor(&mut self, db: &Database) -> Result<Cursor<'_>> {
        let dbi = self.database(db)?;
        let mut raw = ptr::null_mut();
        // SAFETY: live transaction and DBI; the output slot is writable.
        check(unsafe { sys::mdb_cursor_open(self.raw(), dbi, &mut raw) })?;
        Ok(Cursor {
            ptr: NonNull::new(raw).expect("mdb_cursor_open returned a null cursor"),
            _txn: PhantomData,
            seek_key: Vec::new(),
        })
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if let Some(raw) = self.ptr.take() {
            // SAFETY: this transaction is live and exclusively owned; all borrowed
            // cursors have been closed (or forgotten and will never be used again).
            unsafe { sys::mdb_txn_abort(raw.as_ptr()) };
            if self.write {
                self.env.writer_active.set(false);
            }
        }
    }
}

/// A snapshot reader; drop ends the transaction.
pub struct ReadTransaction(Transaction);

/// A writer; drop rolls back any uncommitted changes.
pub struct WriteTransaction(Transaction);

macro_rules! transaction_reads {
    ($type:ident) => {
        impl $type {
            /// Return an owned value, or None for a missing key.
            pub fn get(&mut self, db: &Database, key: &[u8]) -> Result<Option<Vec<u8>>> {
                self.0.get(db, key)
            }

            pub fn cursor(&mut self, db: &Database) -> Result<Cursor<'_>> {
                self.0.cursor(db)
            }

            /// Abort and release this transaction immediately.
            pub fn abort(self) {}
        }
    };
}

transaction_reads!(ReadTransaction);
transaction_reads!(WriteTransaction);

impl WriteTransaction {
    /// Insert or replace a value; duplicate databases retain other distinct values.
    pub fn put(&mut self, db: &Database, key: &[u8], value: &[u8]) -> Result<()> {
        let dbi = self.0.database(db)?;
        let mut key = input(key);
        let mut value = input(value);
        // SAFETY: exclusive live writer, matching DBI, and readable inputs. No
        // reserve/multiple flags are exposed, so native code copies these bytes.
        check(unsafe { sys::mdb_put(self.0.raw(), dbi, &mut key, &mut value, 0) })
    }

    /// Delete a key and all of its values, returning false if the key is absent.
    pub fn delete(&mut self, db: &Database, key: &[u8]) -> Result<bool> {
        let dbi = self.0.database(db)?;
        let mut key = input(key);
        // SAFETY: exclusive live writer and matching DBI. A null data pointer
        // requests deletion of the key and all duplicates.
        let code = unsafe { sys::mdb_del(self.0.raw(), dbi, &mut key, ptr::null_mut()) };
        if code == sys::MDB_NOTFOUND {
            return Ok(false);
        }
        check(code)?;
        Ok(true)
    }

    /// Commit and consume this transaction, including when the native commit fails.
    pub fn commit(self) -> Result<()> {
        self.0.commit()
    }
}

/// Borrowed bytes valid until the next mutable use or drop of their cursor.
#[derive(Debug)]
pub struct Entry<'cursor> {
    pub key: &'cursor [u8],
    pub value: &'cursor [u8],
}

pub struct Cursor<'txn> {
    ptr: NonNull<sys::MDB_cursor>,
    _txn: PhantomData<&'txn mut Transaction>,
    // MDB_SET_RANGE may leave the key pointing at its input for an exact match.
    seek_key: Vec<u8>,
}

impl Cursor<'_> {
    pub fn first(&mut self) -> Result<Option<Entry<'_>>> {
        self.read(sys::MDB_FIRST)
    }

    /// Advance, or start at the first entry when the cursor is unpositioned.
    pub fn next_entry(&mut self) -> Result<Option<Entry<'_>>> {
        self.read(sys::MDB_NEXT)
    }

    pub fn seek(&mut self, key: &[u8]) -> Result<Option<Entry<'_>>> {
        self.seek_key.clear();
        self.seek_key.extend_from_slice(key);
        self.read(sys::MDB_SET_RANGE)
    }

    /// Count values for a key in a duplicate database, checking both native calls.
    /// A missing key has zero values; native failures remain errors.
    pub fn value_count(&mut self, key: &[u8]) -> Result<usize> {
        let mut key = input(key);
        let mut value = input(&[]);
        // SAFETY: exclusive live cursor with initialized input/output holders.
        let code =
            unsafe { sys::mdb_cursor_get(self.ptr.as_ptr(), &mut key, &mut value, sys::MDB_SET) };
        if code == sys::MDB_NOTFOUND {
            return Ok(0);
        }
        check(code)?;
        let mut count = 0;
        // SAFETY: cursor_get succeeded and positioned the cursor; count is writable.
        check(unsafe { sys::mdb_cursor_count(self.ptr.as_ptr(), &mut count) })?;
        Ok(count)
    }

    fn read(&mut self, op: sys::MDB_cursor_op) -> Result<Option<Entry<'_>>> {
        let mut key = input(&self.seek_key);
        let mut value = input(&[]);
        // SAFETY: exclusive live cursor. These operations read entries without
        // modifying the caller's input buffer; holders remain live for the call.
        let code = unsafe { sys::mdb_cursor_get(self.ptr.as_ptr(), &mut key, &mut value, op) };
        if code == sys::MDB_NOTFOUND {
            return Ok(None);
        }
        check(code)?;
        // SAFETY: success initialized both values. Their borrowed lifetime is
        // restricted to this mutable cursor borrow, including compressed scratch.
        Ok(Some(unsafe {
            Entry {
                key: bytes(key),
                value: bytes(value),
            }
        }))
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        // SAFETY: the transaction borrow keeps it live until this cursor is closed.
        unsafe { sys::mdb_cursor_close(self.ptr.as_ptr()) };
    }
}

fn input(bytes: &[u8]) -> sys::MDB_val {
    sys::MDB_val {
        mv_size: bytes.len(),
        mv_data: bytes.as_ptr().cast_mut().cast(),
    }
}

unsafe fn bytes<'a>(value: sys::MDB_val) -> &'a [u8] {
    if value.mv_size == 0 {
        return &[];
    }
    // SAFETY: caller guarantees this native value is readable for the chosen lifetime.
    unsafe { slice::from_raw_parts(value.mv_data.cast(), value.mv_size) }
}
