use dtlvnative::llama::{Embedder, EmbedderOptions, GenerationOptions, Generator, ModelOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let kind = args
        .next()
        .expect("Usage: models embed|generate MODEL_PATH");
    let path = args.next().expect("Missing GGUF model path");
    if kind == "embed" {
        let mut embedder = Embedder::new(path, EmbedderOptions::default())?;
        let vector = embedder.embed("Datalevin uses native vector search.")?;
        println!(
            "{} dimensions: {:?}",
            vector.len(),
            &vector[..vector.len().min(8)]
        );
    } else {
        let mut generator = Generator::new(
            path,
            ModelOptions {
                context_size: 512,
                batch_size: 128,
                threads: 2,
            },
        )?;
        println!(
            "{}",
            generator.generate(
                "Write one sentence about databases.",
                GenerationOptions::default()
            )?
        );
    }
    Ok(())
}
