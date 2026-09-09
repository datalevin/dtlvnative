# Rust native interfaces

[`dtlvnative`](https://crates.io/crates/dtlvnative) provides DLMDB storage,
USearch vector indexes, and CPU model operations for Rust applications.
[`dtlvnative-sys`](https://crates.io/crates/dtlvnative-sys) exposes the raw native
bindings and is included automatically by `dtlvnative`.

## Installation

Add the following to your `Cargo.toml` to enable storage, vectors, and models:

```toml
[dependencies]
dtlvnative = { version = "1.1.1", features = ["usearch", "llama"] }
```

For storage alone, use `dtlvnative = "1.1.1"`.

The crates use prebuilt native libraries downloaded from
[GitHub releases](https://github.com/datalevin/dtlvnative/releases/tag/1.1.1).
Only the libraries for your enabled features are downloaded, and their SHA-256
checksums are verified. You need Rust, its platform linker, and `curl` for the
initial HTTPS download. Native C/C++ compilation, CMake, and libclang are
unnecessary when using the published crates.

## Features

| Feature | Interface | Native component |
|---|---|---|
| `dlmdb` (default) | Environments, named databases, transactions, cursors | Static DLMDB/DTLV storage library |
| `usearch` | Indexing, filtered search, metrics, exact search, serialization | Shared USearch library |
| `llama` | Embeddings, tokenization, generation, summarization, vision/OCR | Shared DTLV llama library |
| `full` | All three | All three |

Each feature works independently with `default-features = false`. For example,
to use vector search without storage:

```toml
[dependencies]
dtlvnative = { version = "1.1.1", default-features = false, features = ["usearch"] }
```

Disabling all features avoids native downloads and linkage.

## Supported platforms

| Rust target | Native binary build | Optional runtime dependencies |
|---|---|---|
| `aarch64-apple-darwin` | macOS 14, LLVM | System libc++; bundled `libomp.dylib` |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04, GCC 12 | System glibc/libstdc++; bundled `libgomp.so.1` |
| `aarch64-unknown-linux-gnu` | Ubuntu 24.04, GCC 12 | System glibc/libstdc++; bundled `libgomp.so.1` |
| `x86_64-pc-windows-msvc` | Windows 2022, VS 2022 | Dynamic MSVC runtime; bundled `vcomp140.dll` |

Windows applications need the Visual C++ 2022 runtime. Linux applications need
glibc and libstdc++ compatible with the listed builds. Other targets and Windows
`crt-static` are unsupported.

## Deployment

DLMDB links statically with pthread on Unix or Advapi32 on Windows. Storage
alone does not require a separate native library directory or OpenMP runtime.

USearch and llama load their shared libraries from Cargo's extracted native
directory. To deploy an application, copy the enabled components' shared
libraries and their bundled OpenMP library together into a deployment directory.
Set **`DTLVNATIVE_RUNTIME_DIR`** to that directory before first use. You can find
the original directory through `dtlvnative::sys::runtime::directory()` or extract
the matching native release archives yourself.

Use the same library versions selected when building the application, and
preserve their accompanying license notices. Loaded libraries remain in memory
for the process lifetime.

## Offline builds

Download the archives for your crate version, target, and enabled features from
the matching GitHub release. Their names follow
`dtlvnative-VERSION-TARGET-COMPONENT.tar.gz`, where `COMPONENT` is `dlmdb`,
`usearch`, or `llama`.

Put the archives in a directory and set:

```sh
export DTLVNATIVE_NATIVE_DIR=/path/to/native-archives
export DTLVNATIVE_OFFLINE=1
cargo build --offline
```

Cargo dependencies must also be cached for an offline build. Native archive
checksums are always verified. Cargo's `--offline` flag alone does not prevent
native downloads; `DTLVNATIVE_OFFLINE=1` controls those downloads.

`DTLVNATIVE_NATIVE_DIR` contains archives used during compilation.
`DTLVNATIVE_RUNTIME_DIR` contains extracted shared libraries used at runtime.

## API contracts

- Storage: `Environment::open` is unsafe because callers must coordinate all
  access to memory-mapped files. Transactions and database handles retain the
  environment; commit consumes a writer and drop aborts unfinished writers.
  Cursor entries borrow the cursor exclusively. The managed API supports
  get/put/delete, forward cursors, seeking, duplicate counts, and counted/prefix
  database flags.
- Vectors: `Index` owns its native handle. Dimensions, scalar/metric combinations,
  result capacities, and thread limits are checked. Search requires exclusive
  access. A filtered-search panic is caught at the callback boundary and resumed
  after returning from native code. `IndexView` borrows its backing bytes.
  File/buffer loading and views are unsafe: serialized indexes must come from a
  compatible trusted writer, and mapped files must remain unchanged. Binary
  dimensions are bits, packed most significant bit first; unused low bits must
  be zero.
- Models: `Embedder`, `Generator`, and `VisionGenerator` free their native handles
  on drop. Context operations require exclusive access. Token IDs are tied to
  the originating embedder. Strings reject interior NULs; output UTF-8 is checked.
  Insufficient generation output capacity returns an error without rerunning
  generation. Supply a GGUF model suited to the operation; vision/OCR also
  requires a matching projector GGUF. Model execution uses the CPU backend.

Managed handles are confined to their creating thread. Raw functions remain
available through `sys`; callers must uphold the C lifetime and threading rules.
Do not link another LMDB or the aggregate JVM native library into this interface.

## Examples

- [Storage](dtlvnative/examples/storage.rs): open an environment, write and read
  values, and iterate with a cursor.
- [Vector persistence](dtlvnative/examples/vector_fixture.rs): create an index,
  add vectors, search, and save or load an index.
- [Embeddings and text generation](dtlvnative/examples/models.rs): load a GGUF
  model and embed text or generate a response.
