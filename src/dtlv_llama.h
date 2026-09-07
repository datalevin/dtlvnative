/** @file dtlv.h
 *	Native supporting functions for Datalevin: a simple, fast and
 *  versatile Datalog database.
 *
 *  Datalevin works with LMDB, a Btree based key value store. This library
 *  provides an iterator interface to LMDB.
 *
 *	@author	Huahai Yang
 *
 *	@copyright Copyright 2020-2025. Huahai Yang. All rights reserved.
 *
 *  This code is released under Eclipse Public License 2.0.
 */

#ifndef DTLV_LLAMA_H
#define DTLV_LLAMA_H

#include "dtlv_common.h"

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

  /**
   * Opaque llama.cpp embedding handle.
   */
  typedef struct dtlv_llama_embedder dtlv_llama_embedder;

  /**
   * Create a CPU-only llama.cpp embedder backed by a GGUF model.
   *
   * @param embedder The address where the embedder will be stored.
   * @param model_path Path to a GGUF embedding model.
   * @param n_ctx Context size. Use 0 to default to the model training context.
   * @param n_batch Max tokens accepted per embedding request. Use 0 to mirror
   *                the context size.
   * @param n_threads CPU thread count. Use 0 to keep llama.cpp defaults.
   * @param normalize Non-zero to L2 normalize returned embeddings.
   * @return MDB_SUCCESS or an error code.
   */
  int dtlv_llama_embedder_create(dtlv_llama_embedder **embedder,
                                 const char *model_path,
                                 int n_ctx,
                                 int n_batch,
                                 int n_threads,
                                 int normalize);

  /**
   * Return the embedding width for this embedder.
   *
   * @param embedder The embedder handle.
   * @return The number of floats in each embedding, or -1 on invalid input.
   */
  int dtlv_llama_embedder_n_embd(dtlv_llama_embedder *embedder);

  /**
   * Return the number of tokens for a UTF-8 string.
   *
   * @param embedder The embedder handle.
   * @param text The UTF-8 text to tokenize.
   * @return The token count, or a negative errno on error.
   */
  int dtlv_llama_token_count(dtlv_llama_embedder *embedder,
                              const char *text);

  /**
   * Return the context size (max tokens) for this embedder.
   *
   * @param embedder The embedder handle.
   * @return The context size, or -1 on invalid input.
   */
  int dtlv_llama_embedder_n_ctx(dtlv_llama_embedder *embedder);

  /**
   * Tokenize a UTF-8 string into a caller-owned token buffer.
   *
   * @param embedder The embedder handle.
   * @param text The UTF-8 text to tokenize.
   * @param tokens Caller-owned token buffer.
   * @param n_tokens_max Capacity of the token buffer.
   * @return Number of tokens written, or a negative errno on error.
   *         -EMSGSIZE if the buffer is too small (absolute value is
   *         the required size).
   */
  int dtlv_llama_tokenize(dtlv_llama_embedder *embedder,
                           const char *text,
                           int *tokens,
                           int n_tokens_max);

  /**
   * Convert tokens back to a UTF-8 string.
   *
   * @param embedder The embedder handle.
   * @param tokens Array of token ids.
   * @param n_tokens Number of tokens.
   * @param text Caller-owned output buffer.
   * @param text_len_max Capacity of the text buffer in bytes.
   * @return Number of bytes written, or a negative errno on error.
   *         -EMSGSIZE if the buffer is too small (absolute value is
   *         the required size).
   */
  int dtlv_llama_detokenize(dtlv_llama_embedder *embedder,
                             const int *tokens,
                             int n_tokens,
                             char *text,
                             int text_len_max);

  /**
   * Compute an embedding for a single UTF-8 string.
   *
   * Text that exceeds the context/batch token limit is automatically
   * truncated (only the leading tokens are kept).
   *
   * @param embedder The embedder handle.
   * @param text The UTF-8 text to embed.
   * @param output Caller-owned float buffer.
   * @param output_len Number of floats available in output.
   * @return MDB_SUCCESS or an error code.
   */
  int dtlv_llama_embed(dtlv_llama_embedder *embedder,
                       const char *text,
                       float *output,
                       size_t output_len);

  /**
   * Compute embeddings for multiple UTF-8 strings in a single batch.
   *
   * Individual texts that exceed the context/batch token limit are
   * automatically truncated (only the leading tokens are kept).
   * EMSGSIZE is returned only when the combined token count of all
   * (possibly truncated) texts still exceeds the batch/context capacity.
   *
   * @param embedder The embedder handle.
   * @param texts Array of UTF-8 strings to embed.
   * @param n_texts Number of strings.
   * @param output Caller-owned float buffer of size n_texts * n_embd.
   * @param output_len Total number of floats available in output.
   * @return MDB_SUCCESS or an error code.
   */
  int dtlv_llama_embed_batch(dtlv_llama_embedder *embedder,
                              const char **texts,
                              int n_texts,
                              float *output,
                              size_t output_len);

  /**
   * Destroy a llama.cpp embedder.
   *
   * @param embedder The embedder handle.
   */
  void dtlv_llama_embedder_destroy(dtlv_llama_embedder *embedder);

  /**
   * Opaque llama.cpp text-generation handle.
   */
  typedef struct dtlv_llama_generator dtlv_llama_generator;

  /**
   * Create a CPU-only llama.cpp generator backed by a decoder-only GGUF model.
   *
   * This API is intended for prompt-based text generation such as document
   * summarization. Encoder-only embedding models are not supported here.
   *
   * @param generator The address where the generator will be stored.
   * @param model_path Path to a GGUF decoder-only text model.
   * @param n_ctx Context size. Use 0 to default to the model training context.
   * @param n_batch Max prompt tokens processed per decode call. Use 0 to mirror
   *                the context size.
   * @param n_threads CPU thread count. Use 0 to keep llama.cpp defaults.
   * @return MDB_SUCCESS or an error code.
   */
  int dtlv_llama_generator_create(dtlv_llama_generator **generator,
                                  const char *model_path,
                                  int n_ctx,
                                  int n_batch,
                                  int n_threads);

  /**
   * Return the context size (max tokens) for this generator.
   *
   * @param generator The generator handle.
   * @return The context size, or -1 on invalid input.
   */
  int dtlv_llama_generator_n_ctx(dtlv_llama_generator *generator);

  /**
   * Return the number of tokens for a UTF-8 string.
   *
   * @param generator The generator handle.
   * @param text The UTF-8 text to tokenize.
   * @return The token count, or a negative errno on error.
   */
  int dtlv_llama_generator_token_count(dtlv_llama_generator *generator,
                                       const char *text);

  /**
   * Generate text for a raw prompt.
   *
   * Prompt text that exceeds the context size is automatically truncated
   * (only the leading tokens are kept). When n_predict <= 0, a default budget
   * of 128 generated tokens is used.
   *
   * @param generator The generator handle.
   * @param prompt The UTF-8 prompt text.
   * @param n_predict Maximum number of tokens to generate.
   * @param output Caller-owned output buffer.
   * @param output_len Capacity of the output buffer in bytes.
   * @return Number of bytes written, or a negative errno on error.
   *         -EMSGSIZE if the output buffer is too small.
   */
  int dtlv_llama_generate(dtlv_llama_generator *generator,
                          const char *prompt,
                          int n_predict,
                          char *output,
                          size_t output_len);

  /**
   * Generate a summary for a UTF-8 document.
   *
   * This helper formats a concise summarization prompt, using the model's
   * built-in chat template when one is present.
   *
   * Text that exceeds the context size is automatically truncated
   * (only the leading tokens are kept). When n_predict <= 0, a default budget
   * of 128 generated tokens is used.
   *
   * @param generator The generator handle.
   * @param text The UTF-8 source document.
   * @param n_predict Maximum number of tokens to generate.
   * @param output Caller-owned output buffer.
   * @param output_len Capacity of the output buffer in bytes.
   * @return Number of bytes written, or a negative errno on error.
   *         -EMSGSIZE if the output buffer is too small.
   */
  int dtlv_llama_summarize(dtlv_llama_generator *generator,
                           const char *text,
                           int n_predict,
                           char *output,
                           size_t output_len);

  /**
   * Destroy a llama.cpp generator.
   *
   * @param generator The generator handle.
   */
  void dtlv_llama_generator_destroy(dtlv_llama_generator *generator);

  /**
   * Opaque llama.cpp vision-generation handle.
   */
  typedef struct dtlv_llama_vision_generator dtlv_llama_vision_generator;

  /**
   * Create a CPU-only llama.cpp vision generator backed by a multimodal GGUF
   * text model and matching mmproj GGUF.
   *
   * This API is intended for single-image prompt completion such as OCR and
   * document parsing with models like PaddleOCR-VL. The prompt may optionally
   * contain one media marker (`<__media__>`). When omitted, the marker is
   * automatically prepended before tokenization.
   *
   * @param generator The address where the generator will be stored.
   * @param model_path Path to the multimodal text GGUF model.
   * @param mmproj_path Path to the matching multimodal projector GGUF.
   * @param n_ctx Context size. Use 0 to default to the model training context.
   * @param n_batch Max prompt tokens processed per decode call. Use 0 to mirror
   *                the context size.
   * @param n_threads CPU thread count. Use 0 to keep dtlv defaults.
   * @param image_min_tokens Minimum image tokens for dynamic-resolution models.
   *                         Use 0 to keep model metadata defaults.
   * @param image_max_tokens Maximum image tokens for dynamic-resolution models.
   *                         Use 0 to keep model metadata defaults.
   * @return MDB_SUCCESS or an error code.
   */
  int dtlv_llama_vision_generator_create(dtlv_llama_vision_generator **generator,
                                         const char *model_path,
                                         const char *mmproj_path,
                                         int n_ctx,
                                         int n_batch,
                                         int n_threads,
                                         int image_min_tokens,
                                         int image_max_tokens);

  /**
   * Return the context size (max positions) for this vision generator.
   *
   * @param generator The generator handle.
   * @return The context size, or -1 on invalid input.
   */
  int dtlv_llama_vision_generator_n_ctx(dtlv_llama_vision_generator *generator);

  /**
   * Generate text for a single image plus prompt.
   *
   * The prompt may contain at most one `<__media__>` marker. If none is
   * present, the marker is automatically prepended. When n_predict <= 0, a
   * default budget of 256 generated tokens is used.
   *
   * @param generator The generator handle.
   * @param prompt The UTF-8 prompt text.
   * @param image_path Path to an image file understood by stb_image.
   * @param n_predict Maximum number of tokens to generate.
   * @param output Caller-owned output buffer.
   * @param output_len Capacity of the output buffer in bytes.
   * @return Number of bytes written, or a negative errno on error.
   *         -EMSGSIZE if the prompt/image pair exceeds the context or the
   *         output buffer is too small.
   */
  int dtlv_llama_vision_generate(dtlv_llama_vision_generator *generator,
                                 const char *prompt,
                                 const char *image_path,
                                 int n_predict,
                                 char *output,
                                 size_t output_len);

  /**
   * Convenience helper for OCR-style extraction using the built-in `OCR:`
   * prompt expected by PaddleOCR-VL.
   *
   * @param generator The generator handle.
   * @param image_path Path to an image file.
   * @param n_predict Maximum number of tokens to generate.
   * @param output Caller-owned output buffer.
   * @param output_len Capacity of the output buffer in bytes.
   * @return Number of bytes written, or a negative errno on error.
   */
  int dtlv_llama_ocr(dtlv_llama_vision_generator *generator,
                     const char *image_path,
                     int n_predict,
                     char *output,
                     size_t output_len);

  /**
   * Destroy a llama.cpp vision generator.
   *
   * @param generator The generator handle.
   */
  void dtlv_llama_vision_generator_destroy(dtlv_llama_vision_generator *generator);

#ifdef __cplusplus
}
#endif

#endif
