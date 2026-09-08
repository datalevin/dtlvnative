# Rust native interfaces

`dtlvnative` provides DLMDB storage, USearch vector indexes, and CPU model
operations through the same native C interfaces used by Datalevin's JVM core.
`dtlvnative-sys` exposes the raw bindings. The remaining storage extensions are
tracked separately in [rust.md](../../rust.md).

## Features and use

```toml
[dependencies]
dtlvnative = { version = "0.1.0", features = ["usearch", "llama"] }
```

The first crates.io publication is pending the release workflow.

| Feature | Interface | Native component |
|---|---|---|
| `dlmdb` (default) | Environments, named databases, transactions, cursors | Static DLMDB/DTLV storage library |
| `usearch` | Indexing, filtered search, metrics, exact search, serialization | Shared USearch library |
| `llama` | Embeddings, tokenization, generation, summarization, vision/OCR | Shared DTLV llama library |
| `full` | All three | All three |

Each feature works independently with `default-features = false`. Disabling all
features avoids native downloads and linkage. Examples: [storage](dtlvnative/examples/storage.rs),
[vector persistence](dtlvnative/examples/vector_fixture.rs), and
[embedding/text generation](dtlvnative/examples/models.rs).

Both public crates ship Rust source, generated bindings, and a manifest with
release URLs and SHA-256 pins. CI builds the native code separately. Consumers
need Rust, its platform linker, and `curl` for the initial HTTPS download; they
do not compile C/C++, run CMake/bindgen, or need the native checkout or libclang.
The private `native-builder` crate is never published.

## Native artifacts and deployment

Archives are named `dtlvnative-VERSION-TARGET-COMPONENT.tar.gz`, where component
is `dlmdb`, `usearch`, or `llama`. Only enabled components are downloaded.

| Rust target | Build baseline | Optional runtime dependencies |
|---|---|---|
| `aarch64-apple-darwin` | macOS 14, LLVM | System libc++; bundled `libomp.dylib` |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04, GCC 12 | System glibc/libstdc++; bundled `libgomp.so.1` |
| `aarch64-unknown-linux-gnu` | Ubuntu 24.04, GCC 12 | System glibc/libstdc++; bundled `libgomp.so.1` |
| `x86_64-pc-windows-msvc` | Windows 2022, VS 2022 | Dynamic MSVC runtime; bundled `vcomp140.dll` |

Storage needs pthread on Unix or Advapi32 on Windows, and no OpenMP. Other
targets, Windows `crt-static`, and cross-compilation validation are outside the
initial release matrix. Windows deployments need the Visual C++ 2022 runtime.
Linux deployments must provide libstdc++ compatible with the build baseline.

USearch and llama are loaded lazily from Cargo's extracted native directory.
For application deployment, copy their shared libraries and bundled OpenMP
library together to a deployment directory and set **`DTLVNATIVE_RUNTIME_DIR`**
to that directory before first use. The directory is also available through
`dtlvnative::sys::runtime::directory()`. Alternatively, extract the matching
release archives there. Preserve the accompanying license notices. The override
must contain the same trusted, version-matched libraries selected at build time.
Libraries and their API tables remain loaded for the process lifetime.

For offline builds, put the matching **archives** in `DTLVNATIVE_NATIVE_DIR`.
Their checksums are still enforced. Set `DTLVNATIVE_OFFLINE=1` to prohibit native
downloads; Cargo's `--offline` alone does not stop build-script networking.
Downloads are cached in Cargo's output directory. Runtime deployment uses
extracted libraries, whereas the build override uses archives.

## API contracts

- Storage: `Environment::open` is unsafe because callers must coordinate all
  access to memory-mapped files. Transactions and database handles retain the
  environment; commit consumes a writer and drop aborts unfinished writers.
  Cursor entries borrow the cursor exclusively. See the API safety documentation.
- Vectors: `Index` owns its native handle. Dimensions, scalar/metric combinations,
  result capacities, and thread limits are checked. Search requires exclusive
  access. A filtered-search panic is caught at the callback boundary and resumed
  only after returning from native code. `IndexView` borrows its backing bytes
  and does not expose a mutable owned index. File/buffer loading and views are
  unsafe: serialized graph data must come from a compatible trusted writer, and
  mapped files must remain unchanged. Binary dimensions are bits, packed most
  significant bit first; unused low bits must be zero.
- Models: `Embedder`, `Generator`, and `VisionGenerator` free their native handles
  on drop. Context operations require exclusive access. Token IDs are tied to
  the originating embedder. Strings reject interior NULs; output UTF-8 is checked.
  Embedding truncation, normalization, batch budgets, greedy generation, default
  prompts, and token limits follow the existing DTLV C functions. Insufficient
  generation output capacity returns an error without silently rerunning it.

Managed handles are confined to their creating thread. Raw functions remain
available through `sys`; callers must uphold the C lifetime and threading rules.
Do not link another LMDB or the aggregate JVM native library into this interface.

The initial storage wrapper covers get/put/delete, forward cursors, seeking,
duplicate counts, and counted/prefix database flags. Range counts, rank access,
custom comparators, iterator wrappers, reset/renew, and administration remain
Phase 2 work.

## Development and validation

Maintainers need C/C++ compilers, CMake, Python 3, and libclang. On macOS install
Homebrew `llvm` and `libomp`; use `LIBCLANG_PATH`, `LLVM_PREFIX`, or `LIBOMP_PREFIX`
if detection needs an override. Linux CI uses GCC/G++ 12. Windows uses the VS
2022 x64 developer environment and Git Bash; the script selects MSVC's linker
explicitly and binding generation avoids verbatim Windows header paths.

```sh
git submodule update --init --recursive
script/test-rust
script/test-rust-models --download
```

`test-rust` builds all three artifacts in `target/rust-native`, runs USearch's
C/C++ tests, formatting, Clippy, artifact/release regressions, Rust runtime and
compile-fail tests, every feature combination, examples, and bidirectional C/Rust
storage and vector persistence fixtures. It then tests isolated source crates,
builds both `.crate` packages with native compiler/libclang paths disabled, and
loads the libraries from a separate deployment directory in a fresh process.

`test-rust-models` runs embedding, tokenization, batch/single comparison,
normalization, truncation, generation, summarization, and output-capacity checks.
The embedding and text fixtures have pinned revisions and SHA-256 values in
[model-fixtures.json](model-fixtures.json). Without `--download`, models must
already be cached. `DTLV_EMBEDDING_MODEL_PATH` and `DTLV_TEXT_MODEL_PATH` select
custom local fixtures. Model tests are ignored in ordinary `cargo test` runs.
Vision/OCR model validation is intentionally excluded; wrappers and constructor
argument/error paths are included in regular checks.

For subsequent local Cargo commands:

```sh
export DTLVNATIVE_ARTIFACT_MANIFEST="$PWD/target/rust-native/native-artifacts.json"
export DTLVNATIVE_NATIVE_DIR="$PWD/target/rust-native"
cargo test --manifest-path src/rust/Cargo.toml -p dtlvnative --all-features
```

`DTLVNATIVE_ARTIFACT_MANIFEST` replaces the published manifest and uses its adjacent
`bindings/` directory. It is for local/custom native builds. `DTLVNATIVE_NATIVE_DIR`
alone retains the published checksum pins. Finish artifact generation before
starting consumers against that manifest.

## Release process

Everything runs in the existing [build.yml](../../.github/workflows/build.yml).
All four matrix runners build native artifacts and run Rust, native dependency,
and pinned embedding/text model checks. JVM steps retain their existing controls.

1. Set the Rust version in `Cargo.toml` and the exact sys dependency in
   `dtlvnative/Cargo.toml`, refresh Cargo.lock, and update `CHANGELOG.md`.
   Rust versions are independent of Java package versions.
2. Configure GitHub's `CARGO_REGISTRY_TOKEN` secret with publishing rights to both
   crates, then create the normal GitHub release from the validated commit.
3. Each runner uploads three native archives, generated bindings, checksums, and
   its manifest. Archives include build provenance and dependency license notices.
4. After the entire matrix passes, `script/prepare-rust-release` requires all
   twelve target/component pairs, validates tags, versions, checksums, matching
   source provenance, and compatible bundled OpenMP bytes. It stages a clean
   source workspace with no native checkout or private builder.
5. `script/publish-rust` verifies packages with every feature, uploads immutable
   release assets and `SHA256SUMS`, tests public downloads, then publishes
   `dtlvnative-sys` followed by `dtlvnative`.

To review the same release locally from downloaded workflow artifacts:

```sh
python3 script/prepare-rust-release target/rust-release-inputs/rust-native-* \
  --release-tag RELEASE_TAG --output target/rust-release
script/publish-rust target/rust-release RELEASE_TAG
```

Staging requires a fresh output directory. The publish script defaults to package
verification; `--publish` performs uploads and crates.io publication. `--local`
permits partial staging for tests but never publication. Pushes and manual test
runs do not publish. Never replace a published native asset with different bytes;
change the crate version and record the new checksums in the next release.

## Build provenance and compatibility

Storage is compiled once from `mdb.c`, `midl.c`, and `dtlv.c`. Optional runtimes
use a private snapshot of tracked submodule files, with repository patches
applied idempotently there. Shared CMake flags disable NumKong explicitly to
match the existing JVM backend, disable host-native CPU tuning, and select the
platform OpenMP runtime. Model builds use CPU only, including mtmd for vision.
The C wrapper enables multiple embedding sequences within a shared token budget.

`sys::BUILD_INFO` records artifact version/SHA-256, native revisions, source
fingerprints, patches, and compiler configuration. Release assembly compares
source provenance across platforms while allowing different compilers/SDKs.
No shared submodule is patched by the Rust builder.

Required validation platforms are macOS ARM64, Linux x86-64, Linux ARM64, and
Windows x86-64. Local results are recorded in [phase-3-4.md](phase-3-4.md); CI
configuration alone does not establish a platform pass. The first public native
asset download and crates.io publication remain pending a release run.
