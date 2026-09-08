use std::{env, path::PathBuf};

/// Directory containing optional runtime libraries and their bundled OpenMP
/// dependency. Deploy these together and set DTLVNATIVE_RUNTIME_DIR when moving
/// an application away from its Cargo build directory.
pub fn directory() -> PathBuf {
    env::var_os("DTLVNATIVE_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("DTLVNATIVE_RUNTIME_DIR")))
}

pub(crate) fn load(name: &str) -> Result<libloading::Library, String> {
    let path = std::path::absolute(directory().join(name)).map_err(|error| error.to_string())?;
    // SAFETY: These are the versioned runtime artifacts selected by build.rs.
    // The deployment override must contain the same trusted native libraries.
    // Handles are retained by the API's OnceLock for the life of the process.
    unsafe {
        #[cfg(windows)]
        let result = libloading::os::windows::Library::load_with_flags(
            &path,
            libloading::os::windows::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                | libloading::os::windows::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .map(libloading::Library::from);
        #[cfg(not(windows))]
        let result = libloading::Library::new(&path);
        result.map_err(|error| format!("Could not load {}: {error}. Deploy the runtime and its bundled OpenMP library together and set DTLVNATIVE_RUNTIME_DIR.", path.display()))
    }
}
