#[cfg(feature = "usearch")]
#[test]
fn usearch_runtime_resolves_every_required_symbol() {
    dtlvnative_sys::usearch::api().expect("Load the packaged USearch runtime");
}

#[cfg(feature = "llama")]
#[test]
fn llama_runtime_resolves_every_required_symbol() {
    dtlvnative_sys::llama::api().expect("Load the packaged llama runtime");
}
