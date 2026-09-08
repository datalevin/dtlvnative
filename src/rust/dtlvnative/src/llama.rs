//! CPU embeddings, text generation, and single-image vision through DTLV's C API.
//! Handles are confined to one thread. Model execution takes an exclusive borrow.

use crate::sys::llama as ffi;
use std::{
    ffi::{CString, c_char},
    fmt,
    path::Path,
    ptr::{self, NonNull},
    rc::Rc,
};

#[derive(Debug)]
pub enum Error {
    InvalidInput(&'static str),
    Native(i32),
    Runtime(String),
    InvalidUtf8(std::string::FromUtf8Error),
    Allocation,
    InvalidNativeResult,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
            // DTLV returns C errno values, including on Windows where Rust's
            // io::Error instead interprets raw codes as Win32 GetLastError.
            Self::Native(code) => write!(f, "llama native error (errno {code})"),
            Self::Runtime(message) => f.write_str(message),
            Self::InvalidUtf8(error) => error.fmt(f),
            Self::Allocation => f.write_str("Could not allocate model output"),
            Self::InvalidNativeResult => f.write_str("Invalid result from the model runtime"),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Default)]
pub struct ModelOptions {
    /// Zero selects the native model default.
    pub context_size: u32,
    pub batch_size: u32,
    pub threads: u32,
}
impl ModelOptions {
    fn native(self) -> Result<(i32, i32, i32)> {
        Ok((
            integer(self.context_size as usize)?,
            integer(self.batch_size as usize)?,
            integer(self.threads as usize)?,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EmbedderOptions {
    pub model: ModelOptions,
    pub normalize: bool,
}
impl Default for EmbedderOptions {
    fn default() -> Self {
        Self {
            model: ModelOptions::default(),
            normalize: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VisionOptions {
    pub model: ModelOptions,
    /// Zero preserves the projector's metadata defaults.
    pub image_min_tokens: u32,
    pub image_max_tokens: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct GenerationOptions {
    /// Zero uses DTLV's default: 128 tokens for text, 256 for vision.
    pub max_tokens: u32,
    /// Output bytes, including room for the native trailing NUL. Insufficient
    /// capacity returns the native error; generation is not silently rerun.
    pub max_output_bytes: usize,
}
impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            max_tokens: 0,
            max_output_bytes: 64 * 1024,
        }
    }
}

fn integer(value: usize) -> Result<i32> {
    i32::try_from(value).map_err(|_| Error::InvalidInput("Value exceeds the native i32 limit"))
}
fn text(value: &str) -> Result<CString> {
    integer(value.len())?;
    CString::new(value).map_err(|_| Error::InvalidInput("Text contains an interior NUL"))
}
fn path(value: &Path) -> Result<CString> {
    let value = value
        .to_str()
        .ok_or(Error::InvalidInput("Model paths must be UTF-8"))?;
    if value.is_empty() {
        return Err(Error::InvalidInput("Model path is empty"));
    }
    text(value)
}
fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Native(code))
    }
}
fn count(code: i32) -> Result<usize> {
    if code < 0 {
        Err(Error::Native(
            code.checked_neg().ok_or(Error::InvalidNativeResult)?,
        ))
    } else {
        Ok(code as usize)
    }
}
fn zeroes<T: Default + Clone>(len: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation)?;
    values.resize(len, T::default());
    Ok(values)
}
fn api() -> Result<&'static ffi::LlamaApi> {
    ffi::api().map_err(|error| Error::Runtime(error.into()))
}

/// Token IDs produced by this embedder's vocabulary. Keeping construction private
/// prevents invalid token IDs from reaching native vocabulary lookups.
#[derive(Debug, Clone)]
pub struct Tokens {
    ids: Vec<i32>,
    model: Rc<()>,
}
impl Tokens {
    pub fn as_slice(&self) -> &[i32] {
        &self.ids
    }
}

pub struct Embedder {
    raw: NonNull<ffi::dtlv_llama_embedder>,
    api: &'static ffi::LlamaApi,
    identity: Rc<()>,
    dimensions: usize,
    context_size: usize,
}
impl Embedder {
    pub fn new(model: impl AsRef<Path>, options: EmbedderOptions) -> Result<Self> {
        let model = path(model.as_ref())?;
        let (ctx, batch, threads) = options.model.native()?;
        let api = api()?;
        let mut raw = ptr::null_mut();
        // SAFETY: Input strings outlive this call; raw points to an initialized
        // out parameter. Successful creation transfers exclusive ownership.
        status(unsafe {
            api.dtlv_llama_embedder_create(
                &mut raw,
                model.as_ptr(),
                ctx,
                batch,
                threads,
                i32::from(options.normalize),
            )
        })?;
        let raw = NonNull::new(raw).ok_or(Error::InvalidNativeResult)?;
        let mut result = Self {
            raw,
            api,
            identity: Rc::new(()),
            dimensions: 0,
            context_size: 0,
        };
        // SAFETY: The successfully created handle is live and exclusively owned.
        let (dimensions, context) = unsafe {
            (
                api.dtlv_llama_embedder_n_embd(raw.as_ptr()),
                api.dtlv_llama_embedder_n_ctx(raw.as_ptr()),
            )
        };
        if dimensions <= 0 || context <= 0 {
            return Err(Error::InvalidNativeResult);
        }
        result.dimensions = dimensions as usize;
        result.context_size = context as usize;
        Ok(result)
    }
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
    pub fn context_size(&self) -> usize {
        self.context_size
    }
    pub fn token_count(&self, input: &str) -> Result<usize> {
        let input = text(input)?;
        // SAFETY: The handle and terminated input are valid for this read-only call.
        count(unsafe {
            self.api
                .dtlv_llama_token_count(self.raw.as_ptr(), input.as_ptr())
        })
    }
    pub fn tokenize(&self, input: &str) -> Result<Tokens> {
        let capacity = self.token_count(input)?;
        let input = text(input)?;
        let mut ids = zeroes::<i32>(capacity.max(1))?;
        // SAFETY: Capacity is obtained from the same unchanged vocabulary and
        // input. The output allocation contains capacity initialized i32 values.
        let written = unsafe {
            self.api.dtlv_llama_tokenize(
                self.raw.as_ptr(),
                input.as_ptr(),
                ids.as_mut_ptr(),
                integer(ids.len())?,
            )
        };
        if written < 0 || written as usize > ids.len() {
            return Err(Error::InvalidNativeResult);
        }
        ids.truncate(written as usize);
        Ok(Tokens {
            ids,
            model: Rc::clone(&self.identity),
        })
    }
    pub fn detokenize(&self, tokens: &Tokens) -> Result<String> {
        if !Rc::ptr_eq(&tokens.model, &self.identity) {
            return Err(Error::InvalidInput(
                "Tokens belong to a different model handle",
            ));
        }
        if tokens.ids.is_empty() {
            return Ok(String::new());
        }
        let n_tokens = integer(tokens.ids.len())?;
        let mut bytes = zeroes::<u8>(
            tokens
                .ids
                .len()
                .checked_mul(8)
                .ok_or(Error::Allocation)?
                .max(1),
        )?;
        loop {
            // SAFETY: Token IDs were generated by this exact live vocabulary,
            // and all nonempty input/output buffers have checked native lengths.
            let rc = unsafe {
                self.api.dtlv_llama_detokenize(
                    self.raw.as_ptr(),
                    tokens.ids.as_ptr(),
                    n_tokens,
                    bytes.as_mut_ptr().cast(),
                    integer(bytes.len())?,
                )
            };
            if rc >= 0 {
                if rc as usize > bytes.len() {
                    return Err(Error::InvalidNativeResult);
                }
                bytes.truncate(rc as usize);
                return String::from_utf8(bytes).map_err(Error::InvalidUtf8);
            }
            // This operation returns -required_size after input prevalidation.
            let needed = rc.checked_neg().ok_or(Error::InvalidNativeResult)? as usize;
            if needed <= bytes.len() {
                return Err(Error::InvalidNativeResult);
            }
            bytes = zeroes(needed)?;
        }
    }
    pub fn embed(&mut self, input: &str) -> Result<Vec<f32>> {
        let input = text(input)?;
        let mut output = zeroes::<f32>(self.dimensions)?;
        // SAFETY: Exclusive context access, live input, and a buffer sized to
        // the model's reported embedding width meet the native contract.
        status(unsafe {
            self.api.dtlv_llama_embed(
                self.raw.as_ptr(),
                input.as_ptr(),
                output.as_mut_ptr(),
                output.len(),
            )
        })?;
        Ok(output)
    }
    pub fn embed_batch(&mut self, inputs: &[&str]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let n = integer(inputs.len())?;
        let texts: Vec<_> = inputs
            .iter()
            .map(|value| text(value))
            .collect::<Result<_>>()?;
        let pointers: Vec<_> = texts.iter().map(|value| value.as_ptr()).collect();
        let mut output = zeroes::<f32>(
            inputs
                .len()
                .checked_mul(self.dimensions)
                .ok_or(Error::Allocation)?,
        )?;
        // SAFETY: All CStrings remain live, pointers has n entries, and output
        // holds n * dimensions floats. &mut self excludes simultaneous execution.
        status(unsafe {
            self.api.dtlv_llama_embed_batch(
                self.raw.as_ptr(),
                pointers.as_ptr().cast_mut(),
                n,
                output.as_mut_ptr(),
                output.len(),
            )
        })?;
        Ok(output
            .chunks_exact(self.dimensions)
            .map(<[f32]>::to_vec)
            .collect())
    }
}
impl Drop for Embedder {
    fn drop(&mut self) {
        // SAFETY: This object exclusively owns the handle and destroys it once.
        unsafe { self.api.dtlv_llama_embedder_destroy(self.raw.as_ptr()) };
    }
}

fn generate(
    options: GenerationOptions,
    call: impl FnOnce(i32, *mut c_char, usize) -> i32,
) -> Result<String> {
    let tokens = integer(options.max_tokens as usize)?;
    integer(options.max_output_bytes)?;
    if options.max_output_bytes == 0 {
        return Err(Error::InvalidInput("Output capacity must be positive"));
    }
    let mut output = zeroes::<u8>(options.max_output_bytes)?;
    let written = count(call(tokens, output.as_mut_ptr().cast(), output.len()))?;
    if written >= output.len() {
        return Err(Error::InvalidNativeResult);
    }
    output.truncate(written);
    String::from_utf8(output).map_err(Error::InvalidUtf8)
}

pub struct Generator {
    raw: NonNull<ffi::dtlv_llama_generator>,
    api: &'static ffi::LlamaApi,
    context_size: usize,
    _thread: Rc<()>,
}
impl Generator {
    pub fn new(model: impl AsRef<Path>, options: ModelOptions) -> Result<Self> {
        let model = path(model.as_ref())?;
        let (ctx, batch, threads) = options.native()?;
        let api = api()?;
        let mut raw = ptr::null_mut();
        // SAFETY: Live input string, valid output pointer, and checked options.
        status(unsafe {
            api.dtlv_llama_generator_create(&mut raw, model.as_ptr(), ctx, batch, threads)
        })?;
        let raw = NonNull::new(raw).ok_or(Error::InvalidNativeResult)?;
        let mut result = Self {
            raw,
            api,
            context_size: 0,
            _thread: Rc::new(()),
        };
        // SAFETY: The handle was just successfully created and remains owned.
        let context = unsafe { api.dtlv_llama_generator_n_ctx(raw.as_ptr()) };
        if context <= 0 {
            return Err(Error::InvalidNativeResult);
        }
        result.context_size = context as usize;
        Ok(result)
    }
    pub fn context_size(&self) -> usize {
        self.context_size
    }
    pub fn token_count(&self, input: &str) -> Result<usize> {
        let input = text(input)?;
        // SAFETY: A live handle and input string are supplied to a read-only call.
        count(unsafe {
            self.api
                .dtlv_llama_generator_token_count(self.raw.as_ptr(), input.as_ptr())
        })
    }
    pub fn generate(&mut self, prompt: &str, options: GenerationOptions) -> Result<String> {
        let prompt = text(prompt)?;
        generate(options, |tokens, output, len| {
            // SAFETY: Exclusive handle access, live prompt, and output allocation
            // owned and sized by generate() remain valid throughout the call.
            unsafe {
                self.api.dtlv_llama_generate(
                    self.raw.as_ptr(),
                    prompt.as_ptr(),
                    tokens,
                    output,
                    len,
                )
            }
        })
    }
    pub fn summarize(&mut self, input: &str, options: GenerationOptions) -> Result<String> {
        let input = text(input)?;
        generate(options, |tokens, output, len| {
            // SAFETY: Exclusive handle access and valid input/output buffers.
            unsafe {
                self.api.dtlv_llama_summarize(
                    self.raw.as_ptr(),
                    input.as_ptr(),
                    tokens,
                    output,
                    len,
                )
            }
        })
    }
}
impl Drop for Generator {
    fn drop(&mut self) {
        // SAFETY: This object exclusively owns the live handle and frees it once.
        unsafe { self.api.dtlv_llama_generator_destroy(self.raw.as_ptr()) };
    }
}

pub struct VisionGenerator {
    raw: NonNull<ffi::dtlv_llama_vision_generator>,
    api: &'static ffi::LlamaApi,
    context_size: usize,
    _thread: Rc<()>,
}
impl VisionGenerator {
    pub fn new(
        model: impl AsRef<Path>,
        projector: impl AsRef<Path>,
        options: VisionOptions,
    ) -> Result<Self> {
        let model = path(model.as_ref())?;
        let projector = path(projector.as_ref())?;
        let (ctx, batch, threads) = options.model.native()?;
        let minimum = integer(options.image_min_tokens as usize)?;
        let maximum = integer(options.image_max_tokens as usize)?;
        if maximum > 0 && minimum > maximum {
            return Err(Error::InvalidInput("Minimum image tokens exceed maximum"));
        }
        let api = api()?;
        let mut raw = ptr::null_mut();
        // SAFETY: Both path strings and the output pointer remain live; all
        // integer parameters fit the native API and creation transfers ownership.
        status(unsafe {
            api.dtlv_llama_vision_generator_create(
                &mut raw,
                model.as_ptr(),
                projector.as_ptr(),
                ctx,
                batch,
                threads,
                minimum,
                maximum,
            )
        })?;
        let raw = NonNull::new(raw).ok_or(Error::InvalidNativeResult)?;
        let mut result = Self {
            raw,
            api,
            context_size: 0,
            _thread: Rc::new(()),
        };
        // SAFETY: The newly created handle is live and owned by result.
        let context = unsafe { api.dtlv_llama_vision_generator_n_ctx(raw.as_ptr()) };
        if context <= 0 {
            return Err(Error::InvalidNativeResult);
        }
        result.context_size = context as usize;
        Ok(result)
    }
    pub fn context_size(&self) -> usize {
        self.context_size
    }
    pub fn generate(
        &mut self,
        prompt: &str,
        image: impl AsRef<Path>,
        options: GenerationOptions,
    ) -> Result<String> {
        if prompt.matches("<__media__>").count() > 1 {
            return Err(Error::InvalidInput("Only one media marker is supported"));
        }
        let prompt = text(prompt)?;
        let image = path(image.as_ref())?;
        generate(options, |tokens, output, len| {
            // SAFETY: Exclusive handle access, terminated input strings, and
            // valid output storage satisfy the single-image native API.
            unsafe {
                self.api.dtlv_llama_vision_generate(
                    self.raw.as_ptr(),
                    prompt.as_ptr(),
                    image.as_ptr(),
                    tokens,
                    output,
                    len,
                )
            }
        })
    }
    pub fn ocr(&mut self, image: impl AsRef<Path>, options: GenerationOptions) -> Result<String> {
        let image = path(image.as_ref())?;
        generate(options, |tokens, output, len| {
            // SAFETY: Exclusive handle access and valid input/output buffers.
            unsafe {
                self.api
                    .dtlv_llama_ocr(self.raw.as_ptr(), image.as_ptr(), tokens, output, len)
            }
        })
    }
}
impl Drop for VisionGenerator {
    fn drop(&mut self) {
        // SAFETY: This object exclusively owns and destroys the handle once.
        unsafe {
            self.api
                .dtlv_llama_vision_generator_destroy(self.raw.as_ptr())
        };
    }
}
