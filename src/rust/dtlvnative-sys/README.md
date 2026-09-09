# dtlvnative-sys

Raw bindings to Datalevin's DLMDB fork, USearch C API, and `dtlv_llama_*` model
interface. Prefer `dtlvnative` for the managed Rust API. Stock LMDB is incompatible.

Features: `dlmdb` (default), `usearch`, `llama`, and `full`. Each optional feature
works with default features disabled; enabling none avoids native downloads.
USearch and llama function tables are available through `usearch::api()` and
`llama::api()`. Loading resolves every required symbol once per process.

Published crates contain generated bindings and a manifest for macOS ARM64,
Linux x86-64/ARM64, and Windows x86-64 (MSVC). The build script downloads each
enabled component from a pinned GitHub release and verifies its SHA-256. Rust,
its normal platform linker, and `curl` with HTTPS are required. Native sources,
bindgen, libclang, and CMake are unnecessary for consumers.

Offline builds use `dtlvnative-VERSION-TARGET-COMPONENT.tar.gz` archives in
`DTLVNATIVE_NATIVE_DIR`, with component `dlmdb`, `usearch`, or `llama`. Checksums
are always verified. Set `DTLVNATIVE_OFFLINE=1` to forbid downloads; Cargo's
`--offline` flag alone does not control build-script networking.

Storage links statically with pthread on Unix or Advapi32 on Windows. Optional
runtimes load shared libraries with bundled OpenMP. Copy their extracted libraries
and notices together when deploying and set `DTLVNATIVE_RUNTIME_DIR` to their
new directory before first use. `runtime::directory()` reports the current path.
The override must contain the same trusted libraries selected at build time.
Linux uses system glibc/libstdc++ (Ubuntu 22.04 x86-64, Ubuntu 24.04 ARM64); macOS
uses system libc++ (macOS 14). Windows needs the dynamic Visual C++ 2022 runtime.
Other targets and Windows `crt-static` are unsupported.

`BUILD_INFO` records native revisions, compiler configuration, artifact versions,
and SHA-256 values. All raw calls remain unsafe and retain the C ownership,
buffer, and threading contracts. Do not link another LMDB or the aggregate JVM
library alongside these components.

See the [Rust guide](https://github.com/datalevin/dtlvnative/blob/master/src/rust/README.md)
for installation, supported platforms, offline builds, and deployment.
