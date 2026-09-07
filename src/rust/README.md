# Rust native interfaces

The workspace currently provides the storage foundation: `dtlvnative-sys`
generates all 90 default DLMDB/DTLV storage function bindings, and `dtlvnative`
provides a small ownership-based API. USearch and llama Rust features will be
added in later phases of [rust.md](../../rust.md).

## Build and use

Requirements: a current stable Rust toolchain with edition 2024 support, a C
compiler, CMake for the compatibility fixtures, and libclang for bindgen.
Initialize the DLMDB submodule:

```sh
git submodule update --init src/lmdb
cargo run --manifest-path src/rust/Cargo.toml -p dtlvnative --example storage
script/test-rust
```

Run these commands from the repository root. `script/test-rust` supports Unix
hosts and Windows x86-64 through Git Bash. It checks formatting, Clippy, runtime
and compile-fail tests, the build
with default features disabled, the storage example, and C/Rust persistence in
both directions. CMake builds the storage libraries and a separate C executable
with `DTLV_BUILD_RUST_FIXTURE=ON`, using the native compiler and platform linkage.
The C fixture exercises DTLV key iteration as well as plain and duplicate
databases with counted/prefix flags.

If bindgen cannot find libclang, set `LIBCLANG_PATH` to its library directory.
For example, with Homebrew LLVM: `export LIBCLANG_PATH="$(brew --prefix llvm)/lib"`.
On Windows, use Visual Studio 2022's x64 developer environment with Git Bash,
and set `LIBCLANG_PATH` to the LLVM directory containing `libclang.dll`.
CI initializes that environment before running Rust checks. Its Windows runner
provides [LLVM and Visual Studio 2022](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md).
Storage builds need neither OpenMP nor the USearch/llama submodules. To build
the separate CMake storage target:

```sh
cmake -S src -B target/rust-cmake \
  -DDTLV_USE_USEARCH=OFF -DDTLV_USE_LLAMA=OFF \
  -DCMAKE_INSTALL_PREFIX="$PWD/target/rust-cmake/lib"
cmake --build target/rust-cmake
```

Datalevin can use a local dependency:

```toml
[dependencies]
dtlvnative = { path = "../dtlvnative/src/rust/dtlvnative" }
```

Only `dlmdb` is currently implemented; it is enabled by default. Both crates
are unpublished and require the surrounding native source tree. See the
[storage example](dtlvnative/examples/storage.rs) for a complete transaction.

## Ownership contract

`Environment::open` is unsafe because callers must coordinate access to the
memory-mapped files, including other language bindings and external processes.
Its safety documentation states the full contract. Opening a second handle to
the same environment in this process is forbidden until every derived resource
has been dropped. The safe methods enforce ownership after that boundary:

- Database handles and transactions retain the environment. Database handles
  are published only after their opening transaction commits and are closed
  with the environment. A transaction rejects a database from another environment.
- Commit consumes a writer, even on failure. Dropping an unfinished transaction
  aborts it. A second simultaneous writer reports `WriterActive`.
- Cursors exclusively borrow their transaction. An `Entry` borrows its cursor;
  advancing requires releasing that entry. Transaction `get` returns owned bytes.
- All handles are confined to their creating thread. Readers use `MDB_NOTLS`.
  No `Send`/`Sync` promise is made for the managed native resources.
- Duplicate value counts check `mdb_cursor_get` and `mdb_cursor_count` separately.
  Missing keys return zero; error codes are preserved as `Error::Native`.

The API covers named databases, transactions, get/put/delete, forward cursors,
seeking, duplicate counts, and database flags for counted/prefix layouts. Range
counts, rank access, custom comparators, DTLV iterator wrappers, transaction
reset/renew, and environment administration remain Phase 2 work. Raw calls are
available through `dtlvnative::sys`; callers must uphold the native contracts.

## Build ownership and provenance

Cargo compiles `mdb.c`, `midl.c`, and storage-only `dtlv.c` into its `OUT_DIR`.
It never links the aggregate JVM library or patches a shared submodule. The
existing Make build combines `dtlv.c` and `dtlv_llama.c`; `dtlv.h` retains the
aggregate public interface. CMake can select USearch and llama independently.

`dtlvnative::sys::BUILD_INFO` records the target, DLMDB revision, source-content
fingerprints, compiler arguments, and runtime linkage. Fingerprints use FNV-1a
for change identification, not cryptographic verification. The Phase 0 JSON/TSV
remain a historical audit; their old header locations and source hashes describe
the code before the split. DLMDB needs no repository patch. Patches/configuration
for optional USearch and llama Cargo builds remain part of their implementation.

The build uses `cc` target/compiler discovery and passes its Unix compiler
arguments, or MSVC include/define arguments, to bindgen. Compiler/SDK settings
come from the usual `cc` environment variables; bindgen also supports
`LIBCLANG_PATH` and `BINDGEN_EXTRA_CLANG_ARGS`. Any manually supplied ABI settings
must agree between C and bindgen. Custom ABI configurations and cross compilation
have not been validated. Do not combine another LMDB implementation or the JVM
aggregate library with this archive in one process.

## Validation recorded on 2026-09-06

Validated locally on macOS ARM64, Rust/Cargo 1.98.1:

- `script/test-rust`: seven runtime regressions, two compile-fail cases, Clippy,
  formatting, feature-disabled compilation, example, and bidirectional C fixtures.
- The storage-only CMake configure/build shown above.
- `./script/build-macos`, including USearch `test_cpp` and `test_c`.
- Fresh JavaCPP JNI generation and Java `Test`: all storage/vector cases and
  llama embeddings passed. Summarization/OCR were skipped without their models.
- All 150 default public native symbols remain exported; the 90 storage functions
  appear in the generated bindings. The Rust storage executable contains one
  DLMDB engine and no USearch/llama engine or OpenMP dependency.

The full Rust script needed an unsandboxed run for macOS semaphore locks after
the sandbox returned `EPERM`. Tests passed with normal host permissions.
The [Rust checks in build.yml](../../.github/workflows/build.yml) run in the existing
build job on every matrix platform: macOS ARM64, Linux x86-64, Linux ARM64,
and Windows x86-64. These four platforms are the required validation scope from
Phase 1 onward. Remote CI has not run yet; Linux and Windows execution remain
pending. FreeBSD and cross compilation remain outside this initial matrix.
Packaged platform binaries were not regenerated. Detailed local commands are
in [phase-1.md](phase-1.md).
