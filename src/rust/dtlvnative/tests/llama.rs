use dtlvnative::llama::{
    Embedder, EmbedderOptions, Generator, ModelOptions, VisionGenerator, VisionOptions,
};

#[test]
fn invalid_model_paths_return_errors() {
    assert!(Embedder::new("/missing/model.gguf", EmbedderOptions::default()).is_err());
    assert!(Generator::new("/missing/model.gguf", ModelOptions::default()).is_err());
    assert!(
        VisionGenerator::new(
            "/missing/model.gguf",
            "/missing/mmproj.gguf",
            VisionOptions::default()
        )
        .is_err()
    );
}

#[test]
fn model_options_and_strings_are_checked_before_native_calls() {
    assert!(Embedder::new("bad\0path", EmbedderOptions::default()).is_err());
    assert!(Generator::new("", ModelOptions::default()).is_err());
    assert!(
        Generator::new(
            "model.gguf",
            ModelOptions {
                context_size: u32::MAX,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        VisionGenerator::new(
            "model.gguf",
            "projector.gguf",
            VisionOptions {
                image_min_tokens: 512,
                image_max_tokens: 128,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires DTLV_EMBEDDING_MODEL_PATH; run script/test-rust-models"]
fn model_embeddings_tokens_and_batch() {
    let path =
        std::env::var_os("DTLV_EMBEDDING_MODEL_PATH").expect("Set DTLV_EMBEDDING_MODEL_PATH");
    let options = EmbedderOptions {
        model: ModelOptions {
            context_size: 128,
            batch_size: 128,
            threads: 2,
        },
        normalize: true,
    };
    let mut embedder = Embedder::new(&path, options).unwrap();
    assert!(embedder.dimensions() > 0);
    assert_eq!(embedder.context_size(), 128);
    let text = "query: datalevin database";
    let tokens = embedder.tokenize(text).unwrap();
    assert_eq!(tokens.as_slice().len(), embedder.token_count(text).unwrap());
    assert!(!embedder.detokenize(&tokens).unwrap().is_empty());
    assert!(embedder.tokenize("bad\0text").is_err());
    let first = embedder.embed(text).unwrap();
    assert!(first.iter().all(|value| value.is_finite()));
    let norm: f32 = first.iter().map(|value| value * value).sum();
    assert!((norm - 1.).abs() < 0.001);
    let batch = embedder
        .embed_batch(&[text, "query: vector search"])
        .unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].len(), embedder.dimensions());
    let max_difference = first
        .iter()
        .zip(&batch[0])
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    let similarity: f32 = first.iter().zip(&batch[0]).map(|(a, b)| a * b).sum();
    // Quantized kernels can round differently for different batch shapes.
    assert!(
        max_difference < 0.005 && similarity > 0.9995,
        "Single/batch max difference {max_difference}; cosine {similarity}"
    );
    let second = embedder.embed("query: vector search").unwrap();
    let second_similarity: f32 = second.iter().zip(&batch[1]).map(|(a, b)| a * b).sum();
    assert!(
        second_similarity > 0.9995,
        "Second sequence cosine {second_similarity}"
    );
    assert!(embedder.embed_batch(&[]).unwrap().is_empty());
    let long = "word ".repeat(256);
    assert!(embedder.embed(&long).is_ok());
    assert!(embedder.embed_batch(&[&long, &long]).is_err());
    let other = Embedder::new(&path, options).unwrap();
    assert!(other.detokenize(&tokens).is_err());
}

#[test]
#[ignore = "requires DTLV_TEXT_MODEL_PATH; run script/test-rust-models"]
fn model_generation_summary_and_capacity_errors() {
    use dtlvnative::llama::GenerationOptions;
    let path = std::env::var_os("DTLV_TEXT_MODEL_PATH").expect("Set DTLV_TEXT_MODEL_PATH");
    let mut generator = Generator::new(
        path,
        ModelOptions {
            context_size: 512,
            batch_size: 128,
            threads: 2,
        },
    )
    .unwrap();
    let options = GenerationOptions {
        max_tokens: 24,
        ..Default::default()
    };
    let prompt = "The capital of France is";
    assert!(generator.token_count(prompt).unwrap() > 0);
    let first = generator.generate(prompt, options).unwrap();
    assert!(!first.is_empty());
    assert_eq!(first, generator.generate(prompt, options).unwrap());
    assert!(
        !generator
            .summarize(
                "Datalevin is a database. It supports Datalog queries and vector search.",
                options
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        generator
            .generate(
                prompt,
                GenerationOptions {
                    max_output_bytes: 1,
                    ..options
                }
            )
            .is_err()
    );
    assert!(
        generator
            .generate(
                prompt,
                GenerationOptions {
                    max_output_bytes: 0,
                    ..options
                }
            )
            .is_err()
    );
    assert!(generator.generate("bad\0prompt", options).is_err());
}
