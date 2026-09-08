# dtlvnative

Rust interfaces to Datalevin's native dependencies:

- `dlmdb` (default): environments, named databases, transactions, and cursors for
  Datalevin's DLMDB fork, including counted/prefix database options.
- `usearch`: vector indexing, filtered search, metrics, exact search, file/buffer
  serialization, and borrowed views.
- `llama`: CPU embeddings, tokenization, generation, summarization, vision/OCR.
- `full`: all three interfaces.

```toml
[dependencies]
dtlvnative = { version = "0.1.0", features = ["usearch", "llama"] }
```

Each feature also works independently with `default-features = false`.
`dtlvnative-sys` downloads versioned, SHA-256-verified native binaries for macOS
ARM64, Linux x86-64/ARM64, and Windows x86-64 (MSVC). Consumers need Rust, its
platform linker, and `curl` for the first download. Native C/C++ compilation and
libclang are unnecessary. Offline builds use matching release archives in
`DTLVNATIVE_NATIVE_DIR`; `DTLVNATIVE_OFFLINE=1` forbids native downloads.

Storage links statically. USearch and llama use shared libraries and bundled
OpenMP. When deploying an executable, copy those libraries together and set
`DTLVNATIVE_RUNTIME_DIR` before first use. The original extraction directory is
available through `sys::runtime::directory()`. Windows also needs the Visual C++
2022 runtime; Linux uses system glibc/libstdc++ from the release build baseline.

Handles own and free their resources, remain on their creating thread, and
require exclusive access for mutable native operations. `Environment::open`
requires external coordination of memory-mapped files. Vector loading/viewing
requires trusted, valid serialized indexes; borrowed views retain their backing
memory. Model strings and capacities are checked, and token IDs retain their
originating vocabulary identity. See API safety documentation for each boundary.

See `examples/storage.rs`, `examples/vector_fixture.rs`, and `examples/models.rs`.
Raw APIs are available through `sys`. Vision/OCR model validation is excluded
from the initial validation scope; argument/error paths are checked.
