use dtlvnative::usearch::{Index, IndexOptions, Metric, Scalar};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    assert_eq!(args.len(), 3, "Usage: vector_fixture write|read PATH");
    let mut index = Index::new(IndexOptions {
        dimensions: 3,
        metric: Metric::L2Squared,
        quantization: Scalar::F32,
        ..Default::default()
    })?;
    if args[1] == "write" {
        index.reserve(4)?;
        index.add(42, &[1_f32, 2., 3.])?;
        index.save(&args[2])?;
    } else {
        // SAFETY: The fixture file was produced by the matching C API in CI.
        unsafe {
            index.load(&args[2])?;
        }
        let matches = index.search(&[1_f32, 2., 3.], 1)?;
        assert_eq!(matches.keys, [42]);
        assert_eq!(matches.distances, [0.]);
        let bytes = std::fs::read(&args[2])?;
        // SAFETY: The same trusted C fixture, now through the buffer API.
        unsafe { index.load_buffer(&bytes)? };
        assert_eq!(index.search(&[1_f32, 2., 3.], 1)?.keys, [42]);
        // The C reader checks this Rust buffer output separately.
        let mut buffer_path = args[2].clone();
        buffer_path.push(".rust-buffer");
        std::fs::write(buffer_path, index.to_bytes()?)?;
    }
    Ok(())
}
