//! USearch C API adapter. Native handles are owned, thread confined, and searched
//! through exclusive borrows. Serialized inputs must come from a trusted writer.

use crate::sys::usearch as ffi;
use std::{
    any::Any,
    ffi::{CStr, CString, c_void},
    fmt,
    marker::PhantomData,
    ops::Deref,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::Path,
    ptr::{self, NonNull},
    rc::Rc,
    sync::OnceLock,
};

#[derive(Debug)]
pub enum Error {
    Native(String),
    InvalidInput(&'static str),
    Allocation,
    ReadOnly,
    InvalidNativeResult,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(message) => f.write_str(message),
            Self::InvalidInput(message) => f.write_str(message),
            Self::Allocation => f.write_str("Could not allocate vector data"),
            Self::ReadOnly => f.write_str("An index view cannot be modified"),
            Self::InvalidNativeResult => f.write_str("Invalid result from USearch"),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Cosine,
    InnerProduct,
    L2Squared,
    Haversine,
    Divergence,
    Pearson,
    Jaccard,
    Hamming,
    Tanimoto,
    Sorensen,
}
impl Metric {
    fn native(self) -> ffi::usearch_metric_kind_t {
        match self {
            Self::Cosine => ffi::usearch_metric_cos_k,
            Self::InnerProduct => ffi::usearch_metric_ip_k,
            Self::L2Squared => ffi::usearch_metric_l2sq_k,
            Self::Haversine => ffi::usearch_metric_haversine_k,
            Self::Divergence => ffi::usearch_metric_divergence_k,
            Self::Pearson => ffi::usearch_metric_pearson_k,
            Self::Jaccard => ffi::usearch_metric_jaccard_k,
            Self::Hamming => ffi::usearch_metric_hamming_k,
            Self::Tanimoto => ffi::usearch_metric_tanimoto_k,
            Self::Sorensen => ffi::usearch_metric_sorensen_k,
        }
    }
    fn from_native(value: ffi::usearch_metric_kind_t) -> Result<Self> {
        [
            Self::Cosine,
            Self::InnerProduct,
            Self::L2Squared,
            Self::Haversine,
            Self::Divergence,
            Self::Pearson,
            Self::Jaccard,
            Self::Hamming,
            Self::Tanimoto,
            Self::Sorensen,
        ]
        .into_iter()
        .find(|metric| metric.native() == value)
        .ok_or(Error::InvalidNativeResult)
    }
    fn binary(self) -> bool {
        matches!(
            self,
            Self::Jaccard | Self::Hamming | Self::Tanimoto | Self::Sorensen
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    F64,
    F32,
    F16,
    BFloat16,
    I8,
    U8,
    Binary,
    E5M2,
    E4M3,
    E3M2,
    E2M3,
}
impl Scalar {
    fn native(self) -> ffi::usearch_scalar_kind_t {
        match self {
            Self::F64 => ffi::usearch_scalar_f64_k,
            Self::F32 => ffi::usearch_scalar_f32_k,
            Self::F16 => ffi::usearch_scalar_f16_k,
            Self::BFloat16 => ffi::usearch_scalar_bf16_k,
            Self::I8 => ffi::usearch_scalar_i8_k,
            Self::U8 => ffi::usearch_scalar_u8_k,
            Self::Binary => ffi::usearch_scalar_b1_k,
            Self::E5M2 => ffi::usearch_scalar_e5m2_k,
            Self::E4M3 => ffi::usearch_scalar_e4m3_k,
            Self::E3M2 => ffi::usearch_scalar_e3m2_k,
            Self::E2M3 => ffi::usearch_scalar_e2m3_k,
        }
    }
    fn from_native(value: ffi::usearch_scalar_kind_t) -> Result<Self> {
        [
            Self::F64,
            Self::F32,
            Self::F16,
            Self::BFloat16,
            Self::I8,
            Self::U8,
            Self::Binary,
            Self::E5M2,
            Self::E4M3,
            Self::E3M2,
            Self::E2M3,
        ]
        .into_iter()
        .find(|scalar| scalar.native() == value)
        .ok_or(Error::InvalidNativeResult)
    }
    fn width(self, dimensions: usize) -> usize {
        if self == Self::Binary {
            dimensions.div_ceil(8)
        } else {
            dimensions
        }
    }
}

/// IEEE half-precision bits, with the native scalar's alignment.
#[repr(transparent)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct F16(pub u16);
/// BFloat16 bits, with the native scalar's alignment.
#[repr(transparent)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BFloat16(pub u16);
/// Eight packed binary dimensions per byte, most significant bit first. Unused
/// low bits of the last byte must be zero; dimensions are expressed in bits.
#[repr(transparent)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Binary(pub u8);
mod sealed {
    pub trait Sealed {}
}
pub trait VectorElement: sealed::Sealed + Copy + Default {
    const SCALAR: Scalar;
}
macro_rules! element {
    ($type:ty, $scalar:ident) => {
        impl sealed::Sealed for $type {}
        impl VectorElement for $type {
            const SCALAR: Scalar = Scalar::$scalar;
        }
    };
}
element!(f64, F64);
element!(f32, F32);
element!(F16, F16);
element!(BFloat16, BFloat16);
element!(i8, I8);
element!(u8, U8);
element!(Binary, Binary);

#[derive(Clone, Copy, Debug)]
pub struct IndexOptions {
    pub dimensions: usize,
    pub metric: Metric,
    pub quantization: Scalar,
    /// Zero selects the native default.
    pub connectivity: usize,
    pub expansion_add: usize,
    pub expansion_search: usize,
    pub multi: bool,
}
impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            dimensions: 0,
            metric: Metric::Cosine,
            quantization: Scalar::F32,
            connectivity: 0,
            expansion_add: 0,
            expansion_search: 0,
            multi: false,
        }
    }
}
impl IndexOptions {
    fn validate(self) -> Result<()> {
        if self.dimensions == 0 || self.dimensions > isize::MAX as usize / 8 {
            return Err(Error::InvalidInput("Invalid vector dimensions"));
        }
        if self.metric == Metric::Haversine
            && (self.dimensions != 2 || !matches!(self.quantization, Scalar::F32 | Scalar::F64))
        {
            return Err(Error::InvalidInput(
                "Haversine requires two f32/f64 dimensions",
            ));
        }
        if self.metric.binary() != (self.quantization == Scalar::Binary) {
            return Err(Error::InvalidInput(
                "Binary metrics require binary quantization",
            ));
        }
        if self.metric == Metric::Divergence && matches!(self.quantization, Scalar::I8 | Scalar::U8)
        {
            return Err(Error::InvalidInput(
                "Divergence requires floating point scalars",
            ));
        }
        if self.connectivity > u16::MAX as usize {
            return Err(Error::InvalidInput("Connectivity exceeds the native limit"));
        }
        Ok(())
    }
    fn native(self) -> ffi::usearch_init_options_t {
        ffi::usearch_init_options_t {
            metric_kind: self.metric.native(),
            metric: None,
            quantization: self.quantization.native(),
            dimensions: self.dimensions,
            connectivity: self.connectivity,
            expansion_add: self.expansion_add,
            expansion_search: self.expansion_search,
            multi: self.multi,
        }
    }
    fn from_native(value: ffi::usearch_init_options_t) -> Result<Self> {
        let options = Self {
            dimensions: value.dimensions,
            metric: Metric::from_native(value.metric_kind)?,
            quantization: Scalar::from_native(value.quantization)?,
            connectivity: value.connectivity,
            expansion_add: value.expansion_add,
            expansion_search: value.expansion_search,
            multi: value.multi,
        };
        options.validate()?;
        Ok(options)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Matches {
    pub keys: Vec<u64>,
    pub distances: Vec<f32>,
}
impl Matches {
    fn allocate(count: usize) -> Result<Self> {
        Ok(Self {
            keys: zeroes(count)?,
            distances: zeroes(count)?,
        })
    }
    fn truncate(&mut self, count: usize) -> Result<()> {
        if count > self.keys.len() {
            return Err(Error::InvalidNativeResult);
        }
        self.keys.truncate(count);
        self.distances.truncate(count);
        Ok(())
    }
}
fn zeroes<T: Default + Clone>(len: usize) -> Result<Vec<T>> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation)?;
    result.resize(len, T::default());
    Ok(result)
}
fn api() -> Result<&'static ffi::UsearchApi> {
    ffi::api().map_err(|error| Error::Native(error.into()))
}
fn checked<T>(call: impl FnOnce(*mut ffi::usearch_error_t) -> T) -> Result<T> {
    let mut error = ptr::null();
    let value = call(&mut error);
    if error.is_null() {
        Ok(value)
    } else {
        // SAFETY: The native API returns a valid NUL-terminated error string
        // with static/native-owned storage. Copy it before the next native call.
        Err(Error::Native(
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned(),
        ))
    }
}
fn path(value: &Path) -> Result<CString> {
    let value = value
        .to_str()
        .ok_or(Error::InvalidInput("Index paths must be UTF-8"))?;
    if value.is_empty() {
        return Err(Error::InvalidInput("Index path is empty"));
    }
    CString::new(value).map_err(|_| Error::InvalidInput("Index path contains a NUL"))
}

pub fn version() -> Result<&'static str> {
    static VERSION: OnceLock<String> = OnceLock::new();
    let api = api()?;
    Ok(VERSION.get_or_init(|| {
        // SAFETY: The result is a static string. OnceLock serializes the native
        // version function, which formats into a shared static buffer.
        unsafe { CStr::from_ptr(api.usearch_version()) }
            .to_string_lossy()
            .into_owned()
    }))
}

pub struct Index {
    raw: NonNull<c_void>,
    api: &'static ffi::UsearchApi,
    options: IndexOptions,
    read_only: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Index {
    pub fn new(options: IndexOptions) -> Result<Self> {
        options.validate()?;
        let api = api()?;
        let mut native = options.native();
        // SAFETY: All configuration values are checked; the options and error
        // output remain live. A non-null result transfers handle ownership.
        let raw = checked(|error| unsafe { api.usearch_init(&mut native, error) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::InvalidNativeResult)?,
            api,
            options,
            read_only: false,
            _thread: PhantomData,
        })
    }
    pub fn options(&self) -> IndexOptions {
        self.options
    }
    fn writable(&self) -> Result<()> {
        if self.read_only {
            Err(Error::ReadOnly)
        } else {
            Ok(())
        }
    }
    fn vector<T: VectorElement>(&self, vector: &[T]) -> Result<()> {
        if vector.len() != T::SCALAR.width(self.options.dimensions) {
            return Err(Error::InvalidInput("Vector dimension mismatch"));
        }
        Ok(())
    }
    pub fn len(&self) -> Result<usize> {
        // SAFETY: The index is live and the checked helper owns the error output.
        checked(|error| unsafe { self.api.usearch_size(self.raw.as_ptr(), error) })
    }
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }
    pub fn capacity(&self) -> Result<usize> {
        // SAFETY: The index is live and this is a read-only query.
        checked(|error| unsafe { self.api.usearch_capacity(self.raw.as_ptr(), error) })
    }
    pub fn dimensions(&self) -> usize {
        self.options.dimensions
    }
    pub fn connectivity(&self) -> Result<usize> {
        // SAFETY: The index is live and this is a read-only query.
        checked(|error| unsafe { self.api.usearch_connectivity(self.raw.as_ptr(), error) })
    }
    pub fn memory_usage(&self) -> Result<usize> {
        // SAFETY: The index is live and this is a read-only query.
        checked(|error| unsafe { self.api.usearch_memory_usage(self.raw.as_ptr(), error) })
    }
    pub fn hardware_acceleration(&self) -> Result<String> {
        // SAFETY: The index is live; native returns a static description string.
        let value = checked(|error| unsafe {
            self.api
                .usearch_hardware_acceleration(self.raw.as_ptr(), error)
        })?;
        if value.is_null() {
            return Err(Error::InvalidNativeResult);
        }
        // SAFETY: The returned non-null pointer is a terminated native string.
        Ok(unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned())
    }
    pub fn reserve(&mut self, capacity: usize) -> Result<()> {
        self.writable()?;
        // SAFETY: Exclusive access to a live, mutable index and error output.
        checked(|error| unsafe { self.api.usearch_reserve(self.raw.as_ptr(), capacity, error) })
    }
    pub fn expansion_add(&self) -> Result<usize> {
        // SAFETY: The index is live and this is a read-only query.
        checked(|error| unsafe { self.api.usearch_expansion_add(self.raw.as_ptr(), error) })
    }
    pub fn expansion_search(&self) -> Result<usize> {
        // SAFETY: The index is live and this is a read-only query.
        checked(|error| unsafe { self.api.usearch_expansion_search(self.raw.as_ptr(), error) })
    }
    pub fn set_expansion_add(&mut self, value: usize) -> Result<()> {
        self.writable()?;
        if value == 0 {
            return Err(Error::InvalidInput("Expansion must be positive"));
        }
        // SAFETY: Exclusive access and a positive expansion value.
        checked(|error| unsafe {
            self.api
                .usearch_change_expansion_add(self.raw.as_ptr(), value, error)
        })?;
        self.options.expansion_add = value;
        Ok(())
    }
    pub fn set_expansion_search(&mut self, value: usize) -> Result<()> {
        self.writable()?;
        if value == 0 {
            return Err(Error::InvalidInput("Expansion must be positive"));
        }
        // SAFETY: Exclusive access and a positive expansion value.
        checked(|error| unsafe {
            self.api
                .usearch_change_expansion_search(self.raw.as_ptr(), value, error)
        })?;
        self.options.expansion_search = value;
        Ok(())
    }
    pub fn set_threads_add(&mut self, value: usize) -> Result<()> {
        self.writable()?;
        if value == 0 {
            return Err(Error::InvalidInput("Thread count must be positive"));
        }
        // SAFETY: Exclusive access; the native setter preserves search limits.
        checked(|error| unsafe {
            self.api
                .usearch_change_threads_add(self.raw.as_ptr(), value, error)
        })
    }
    pub fn set_threads_search(&mut self, value: usize) -> Result<()> {
        self.writable()?;
        if value == 0 {
            return Err(Error::InvalidInput("Thread count must be positive"));
        }
        // SAFETY: Exclusive access; the native setter preserves add limits.
        checked(|error| unsafe {
            self.api
                .usearch_change_threads_search(self.raw.as_ptr(), value, error)
        })
    }
    pub fn set_metric(&mut self, metric: Metric) -> Result<()> {
        self.writable()?;
        let options = IndexOptions {
            metric,
            ..self.options
        };
        options.validate()?;
        // SAFETY: Exclusive access and a validated dimension/scalar/metric tuple.
        checked(|error| unsafe {
            self.api
                .usearch_change_metric_kind(self.raw.as_ptr(), metric.native(), error)
        })?;
        self.options = options;
        Ok(())
    }
    pub fn add<T: VectorElement>(&mut self, key: u64, vector: &[T]) -> Result<()> {
        self.writable()?;
        self.vector(vector)?;
        // SAFETY: Live index and a correctly typed/aligned vector of its full
        // dimensionality. The C API copies vector data during this call.
        checked(|error| unsafe {
            self.api.usearch_add(
                self.raw.as_ptr(),
                key,
                vector.as_ptr().cast(),
                T::SCALAR.native(),
                error,
            )
        })
    }
    pub fn contains(&self, key: u64) -> Result<bool> {
        // SAFETY: Read-only query on a live index.
        checked(|error| unsafe { self.api.usearch_contains(self.raw.as_ptr(), key, error) })
    }
    pub fn count(&self, key: u64) -> Result<usize> {
        // SAFETY: Read-only query on a live index.
        checked(|error| unsafe { self.api.usearch_count(self.raw.as_ptr(), key, error) })
    }
    pub fn get<T: VectorElement>(&self, key: u64, limit: usize) -> Result<Vec<Vec<T>>> {
        let limit = limit.min(self.count(key)?);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let width = T::SCALAR.width(self.options.dimensions);
        let mut values = zeroes::<T>(limit.checked_mul(width).ok_or(Error::Allocation)?)?;
        // SAFETY: Output is aligned for T and contains limit complete vectors.
        let found = checked(|error| unsafe {
            self.api.usearch_get(
                self.raw.as_ptr(),
                key,
                limit,
                values.as_mut_ptr().cast(),
                T::SCALAR.native(),
                error,
            )
        })?;
        if found > limit {
            return Err(Error::InvalidNativeResult);
        }
        values.truncate(found * width);
        Ok(values.chunks_exact(width).map(<[T]>::to_vec).collect())
    }
    pub fn search<T: VectorElement>(&mut self, query: &[T], limit: usize) -> Result<Matches> {
        self.vector(query)?;
        let limit = limit.min(self.len()?);
        let mut matches = Matches::allocate(limit)?;
        if limit == 0 {
            return Ok(matches);
        }
        // SAFETY: Query dimensions/type and both result capacities are checked.
        // Exclusive access also protects the index's native search scratch space.
        let count = checked(|error| unsafe {
            self.api.usearch_search(
                self.raw.as_ptr(),
                query.as_ptr().cast(),
                T::SCALAR.native(),
                limit,
                matches.keys.as_mut_ptr(),
                matches.distances.as_mut_ptr(),
                error,
            )
        })?;
        matches.truncate(count)?;
        Ok(matches)
    }
    pub fn filtered_search<T: VectorElement, F: FnMut(u64) -> bool>(
        &mut self,
        query: &[T],
        limit: usize,
        mut filter: F,
    ) -> Result<Matches> {
        self.vector(query)?;
        let limit = limit.min(self.len()?);
        let mut matches = Matches::allocate(limit)?;
        if limit == 0 {
            return Ok(matches);
        }
        let mut state = Filter {
            callback: &mut filter,
            panic: None,
        };
        // SAFETY: USearch invokes this callback synchronously on this thread.
        // State and all input/output allocations outlive the call. The callback
        // catches panics, so none can unwind through the C++ boundary.
        let result = checked(|error| unsafe {
            self.api.usearch_filtered_search(
                self.raw.as_ptr(),
                query.as_ptr().cast(),
                T::SCALAR.native(),
                limit,
                Some(filter_callback::<F>),
                (&mut state as *mut Filter<'_, F>).cast(),
                matches.keys.as_mut_ptr(),
                matches.distances.as_mut_ptr(),
                error,
            )
        });
        if let Some(panic) = state.panic {
            resume_unwind(panic);
        }
        matches.truncate(result?)?;
        Ok(matches)
    }
    pub fn remove(&mut self, key: u64) -> Result<usize> {
        self.writable()?;
        // SAFETY: Exclusive access to a live mutable index.
        checked(|error| unsafe { self.api.usearch_remove(self.raw.as_ptr(), key, error) })
    }
    pub fn rename(&mut self, from: u64, to: u64) -> Result<usize> {
        self.writable()?;
        // SAFETY: Exclusive access to a live mutable index.
        checked(|error| unsafe { self.api.usearch_rename(self.raw.as_ptr(), from, to, error) })
    }
    pub fn clear(&mut self) -> Result<()> {
        self.writable()?;
        // SAFETY: Exclusive access to a live mutable index.
        checked(|error| unsafe { self.api.usearch_clear(self.raw.as_ptr(), error) })
    }
    pub fn save(&self, destination: impl AsRef<Path>) -> Result<()> {
        let destination = path(destination.as_ref())?;
        // SAFETY: Live index and a terminated destination path.
        checked(|error| unsafe {
            self.api
                .usearch_save(self.raw.as_ptr(), destination.as_ptr(), error)
        })
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        // SAFETY: Read-only length query on a live index.
        let len = checked(|error| unsafe {
            self.api.usearch_serialized_length(self.raw.as_ptr(), error)
        })?;
        if len == 0 {
            return Err(Error::InvalidNativeResult);
        }
        let mut output = zeroes::<u8>(len)?;
        // SAFETY: The buffer holds the exact reported serialized length.
        checked(|error| unsafe {
            self.api.usearch_save_buffer(
                self.raw.as_ptr(),
                output.as_mut_ptr().cast(),
                output.len(),
                error,
            )
        })?;
        Ok(output)
    }
    /// Load a trusted USearch file, replacing this index only on success.
    ///
    /// # Safety
    /// The file must be a valid index from a compatible, trusted USearch writer
    /// and must not change during loading. The native parser is not hardened
    /// against malicious or structurally invalid graph data.
    pub unsafe fn load(&mut self, source: impl AsRef<Path>) -> Result<()> {
        // SAFETY: The caller guarantees a stable, trusted serialized index.
        let options = unsafe { metadata(&source)? };
        let source = path(source.as_ref())?;
        let replacement = Self::new(options)?;
        // SAFETY: A fresh handle and the caller-validated file are supplied.
        checked(|error| unsafe {
            replacement
                .api
                .usearch_load(replacement.raw.as_ptr(), source.as_ptr(), error)
        })?;
        *self = replacement;
        Ok(())
    }
    /// Load a trusted serialized buffer into owned native memory.
    ///
    /// # Safety
    /// Bytes must encode a valid index produced by a compatible trusted writer.
    pub unsafe fn load_buffer(&mut self, bytes: &[u8]) -> Result<()> {
        // SAFETY: The caller guarantees the serialized input's validity.
        let options = unsafe { metadata_buffer(bytes)? };
        let replacement = Self::new(options)?;
        // SAFETY: The input is valid and remains live for the copying load.
        checked(|error| unsafe {
            replacement.api.usearch_load_buffer(
                replacement.raw.as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len(),
                error,
            )
        })?;
        *self = replacement;
        Ok(())
    }
    /// View a trusted serialized buffer without copying it.
    ///
    /// # Safety
    /// Bytes must encode a valid index from a compatible trusted writer. The
    /// returned view borrows the buffer and prevents mutation or early release.
    pub unsafe fn view_buffer(bytes: &[u8]) -> Result<IndexView<'_>> {
        if !bytes.as_ptr().addr().is_multiple_of(align_of::<u64>()) {
            return Err(Error::InvalidInput(
                "Index view buffer is not aligned to 8 bytes",
            ));
        }
        // SAFETY: The caller guarantees the serialized input's validity.
        let options = unsafe { metadata_buffer(bytes)? };
        let mut index = Self::new(options)?;
        // SAFETY: Input validity/alignment are established, and the returned
        // lifetime retains the immutable buffer for all native accesses.
        checked(|error| unsafe {
            index.api.usearch_view_buffer(
                index.raw.as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len(),
                error,
            )
        })?;
        index.read_only = true;
        Ok(IndexView {
            index,
            _buffer: PhantomData,
        })
    }
    /// Map a trusted serialized file for read-only search.
    ///
    /// # Safety
    /// The file must be valid and remain unchanged (including no truncation)
    /// until the returned index and all accesses to it have finished.
    pub unsafe fn view_file(source: impl AsRef<Path>) -> Result<Self> {
        // SAFETY: The caller guarantees validity and external coordination.
        let options = unsafe { metadata(&source)? };
        let source = path(source.as_ref())?;
        let mut index = Self::new(options)?;
        // SAFETY: The live index owns the mapping of the caller-validated file.
        checked(|error| unsafe {
            index
                .api
                .usearch_view(index.raw.as_ptr(), source.as_ptr(), error)
        })?;
        index.read_only = true;
        Ok(index)
    }
}
impl Drop for Index {
    fn drop(&mut self) {
        let mut error = ptr::null();
        // SAFETY: The handle is exclusively owned and is freed exactly once.
        unsafe { self.api.usearch_free(self.raw.as_ptr(), &mut error) };
    }
}

/// A buffer view cannot outlive its source bytes.
///
/// ```compile_fail
/// # use dtlvnative::usearch::Index;
/// let mut view = {
///     let bytes = vec![0_u8; 1024];
///     unsafe { Index::view_buffer(&bytes).unwrap() }
/// };
/// view.search(&[1_f32, 2.], 1).unwrap();
/// ```
/// The owned index cannot be extracted from the view to escape its borrow.
///
/// ```compile_fail
/// # use dtlvnative::usearch::{Index, IndexOptions};
/// # fn escape(bytes: &[u8], replacement: Index) -> Index {
/// let mut view = unsafe { Index::view_buffer(bytes).unwrap() };
/// std::mem::replace(&mut *view, replacement)
/// # }
/// ```
pub struct IndexView<'a> {
    index: Index,
    _buffer: PhantomData<&'a [u8]>,
}
impl Deref for IndexView<'_> {
    type Target = Index;
    fn deref(&self) -> &Index {
        &self.index
    }
}
impl IndexView<'_> {
    pub fn search<T: VectorElement>(&mut self, query: &[T], limit: usize) -> Result<Matches> {
        self.index.search(query, limit)
    }
    pub fn filtered_search<T: VectorElement, F: FnMut(u64) -> bool>(
        &mut self,
        query: &[T],
        limit: usize,
        filter: F,
    ) -> Result<Matches> {
        self.index.filtered_search(query, limit, filter)
    }
}

/// Read metadata from a trusted serialized index.
///
/// # Safety
/// The file must be a valid compatible index and remain unchanged during this call.
pub unsafe fn metadata(source: impl AsRef<Path>) -> Result<IndexOptions> {
    let source = path(source.as_ref())?;
    let api = api()?;
    let mut options = IndexOptions::default().native();
    // SAFETY: The caller guarantees file validity; output is initialized storage.
    checked(|error| unsafe { api.usearch_metadata(source.as_ptr(), &mut options, error) })?;
    IndexOptions::from_native(options)
}
/// Read metadata from a trusted serialized buffer.
///
/// # Safety
/// Bytes must encode a valid compatible index.
pub unsafe fn metadata_buffer(bytes: &[u8]) -> Result<IndexOptions> {
    if bytes.is_empty() {
        return Err(Error::InvalidInput("Serialized index is empty"));
    }
    let api = api()?;
    let mut options = IndexOptions::default().native();
    // SAFETY: The input validity is the caller's contract; output is initialized.
    checked(|error| unsafe {
        api.usearch_metadata_buffer(bytes.as_ptr().cast(), bytes.len(), &mut options, error)
    })?;
    IndexOptions::from_native(options)
}

struct Filter<'a, F> {
    callback: &'a mut F,
    panic: Option<Box<dyn Any + Send>>,
}
unsafe extern "C" fn filter_callback<F: FnMut(u64) -> bool>(key: u64, state: *mut c_void) -> i32 {
    // SAFETY: filtered_search supplies a unique Filter<F> that outlives this call.
    let state = unsafe { &mut *state.cast::<Filter<'_, F>>() };
    if state.panic.is_some() {
        return 0;
    }
    match catch_unwind(AssertUnwindSafe(|| (state.callback)(key))) {
        Ok(accept) => i32::from(accept),
        Err(panic) => {
            state.panic = Some(panic);
            0
        }
    }
}

/// Distance between two equally typed vectors. Binary dimensions are in bits.
pub fn distance<T: VectorElement>(
    first: &[T],
    second: &[T],
    dimensions: usize,
    metric: Metric,
) -> Result<f32> {
    IndexOptions {
        dimensions,
        metric,
        quantization: T::SCALAR,
        ..Default::default()
    }
    .validate()?;
    if first.len() != T::SCALAR.width(dimensions) || first.len() != second.len() {
        return Err(Error::InvalidInput("Vector dimension mismatch"));
    }
    let api = api()?;
    // SAFETY: Metric/scalar compatibility and both buffer dimensions/alignment
    // are checked before invoking the C API's unchecked metric dispatch.
    checked(|error| unsafe {
        api.usearch_distance(
            first.as_ptr().cast(),
            second.as_ptr().cast(),
            T::SCALAR.native(),
            dimensions,
            metric.native(),
            error,
        )
    })
}

/// Exact search over contiguous row-major matrices. Returned keys are dataset
/// row offsets. Binary rows contain ceil(dimensions / 8) packed bytes.
pub fn exact_search<T: VectorElement>(
    dataset: &[T],
    queries: &[T],
    dimensions: usize,
    metric: Metric,
    limit: usize,
    threads: usize,
) -> Result<Vec<Matches>> {
    IndexOptions {
        dimensions,
        metric,
        quantization: T::SCALAR,
        ..Default::default()
    }
    .validate()?;
    let width = T::SCALAR.width(dimensions);
    if !dataset.len().is_multiple_of(width) || !queries.len().is_multiple_of(width) {
        return Err(Error::InvalidInput("Matrix does not contain whole vectors"));
    }
    let rows = dataset.len() / width;
    let query_rows = queries.len() / width;
    let limit = limit.min(rows);
    if limit == 0 || query_rows == 0 {
        return (0..query_rows).map(|_| Matches::allocate(0)).collect();
    }
    let mut matches = Matches::allocate(query_rows.checked_mul(limit).ok_or(Error::Allocation)?)?;
    let stride = width.checked_mul(size_of::<T>()).ok_or(Error::Allocation)?;
    let key_stride = limit
        .checked_mul(size_of::<u64>())
        .ok_or(Error::Allocation)?;
    let distance_stride = limit
        .checked_mul(size_of::<f32>())
        .ok_or(Error::Allocation)?;
    let api = api()?;
    // SAFETY: All matrix/result sizes and byte strides are checked, scalar
    // alignment is supplied by T, and count never exceeds the dataset size.
    checked(|error| unsafe {
        api.usearch_exact_search(
            dataset.as_ptr().cast(),
            rows,
            stride,
            queries.as_ptr().cast(),
            query_rows,
            stride,
            T::SCALAR.native(),
            dimensions,
            metric.native(),
            limit,
            threads,
            matches.keys.as_mut_ptr(),
            key_stride,
            matches.distances.as_mut_ptr(),
            distance_stride,
            error,
        )
    })?;
    Ok(matches
        .keys
        .chunks_exact(limit)
        .zip(matches.distances.chunks_exact(limit))
        .map(|(keys, distances)| Matches {
            keys: keys.to_vec(),
            distances: distances.to_vec(),
        })
        .collect())
}
