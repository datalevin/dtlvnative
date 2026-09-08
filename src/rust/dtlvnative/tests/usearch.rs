use dtlvnative::usearch::{Index, IndexOptions, Metric, Scalar};

#[test]
fn vector_add_search_get_and_remove() {
    let mut index = Index::new(IndexOptions {
        dimensions: 3,
        metric: Metric::L2Squared,
        quantization: Scalar::F32,
        ..Default::default()
    })
    .unwrap();
    index.reserve(8).unwrap();
    index.add(42, &[1_f32, 2., 3.]).unwrap();
    index.add(7, &[4_f32, 5., 6.]).unwrap();
    assert_eq!(index.search(&[1_f32, 2., 3.], 2).unwrap().keys, [42, 7]);
    assert_eq!(index.get::<f32>(42, 1).unwrap(), vec![vec![1., 2., 3.]]);
    assert_eq!(index.remove(42).unwrap(), 1);
    assert!(!index.contains(42).unwrap());
}

#[test]
fn dimensions_metrics_and_exact_matrix_search() {
    use dtlvnative::usearch::{Binary, distance, exact_search};
    assert!(Index::new(IndexOptions::default()).is_err());
    assert!(
        Index::new(IndexOptions {
            dimensions: 1,
            metric: Metric::Haversine,
            ..Default::default()
        })
        .is_err()
    );
    assert!(distance(&[1_f32], &[1_f32, 2.], 2, Metric::L2Squared).is_err());
    assert_eq!(
        distance(&[1_f32, 2.], &[4_f32, 6.], 2, Metric::L2Squared).unwrap(),
        25.
    );
    assert_eq!(
        distance(&[Binary(0b0011)], &[Binary(0b0101)], 8, Metric::Hamming).unwrap(),
        2.
    );
    let rows = exact_search(
        &[1_f64, 2., 4., 6., 8., 10.],
        &[4_f64, 6., 1., 2.],
        2,
        Metric::L2Squared,
        9,
        2,
    )
    .unwrap();
    assert_eq!(rows[0].keys, [1, 0, 2]);
    assert_eq!(rows[0].distances, [0., 25., 32.]);
    assert_eq!(rows[1].keys[0], 0);
    assert!(exact_search(&[1_f32, 2., 3.], &[1_f32, 2.], 2, Metric::Cosine, 1, 1).is_err());
}

#[test]
fn duplicate_keys_filters_and_panics_preserve_the_index() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let mut index = Index::new(IndexOptions {
        dimensions: 2,
        metric: Metric::L2Squared,
        multi: true,
        ..Default::default()
    })
    .unwrap();
    index.reserve(8).unwrap();
    index.set_threads_add(2).unwrap();
    index.set_threads_search(3).unwrap();
    index.set_expansion_add(32).unwrap();
    index.set_expansion_search(32).unwrap();
    assert_eq!(index.expansion_add().unwrap(), 32);
    assert_eq!(index.expansion_search().unwrap(), 32);
    assert_eq!(index.options().expansion_add, 32);
    assert_eq!(index.options().expansion_search, 32);
    index.add(1, &[1_f32, 2.]).unwrap();
    index.add(1, &[2_f32, 3.]).unwrap();
    index.add(2, &[3_f32, 4.]).unwrap();
    assert_eq!(index.count(1).unwrap(), 2);
    assert_eq!(index.get::<f64>(1, 10).unwrap().len(), 2);
    assert_eq!(
        index
            .filtered_search(&[1_f32, 2.], 5, |key| key == 2)
            .unwrap()
            .keys,
        [2]
    );
    let panic = catch_unwind(AssertUnwindSafe(|| {
        index.filtered_search(&[1_f32, 2.], 1, |_| panic!("filter panic"))
    }));
    assert!(panic.is_err());
    assert_eq!(index.search(&[1_f32, 2.], 1).unwrap().keys, [1]);
    assert_eq!(index.rename(1, 9).unwrap(), 2);
    assert_eq!(index.remove(9).unwrap(), 2);
    index.clear().unwrap();
    assert!(index.is_empty().unwrap());
}

#[test]
fn serialization_views_and_binary_metric_tags_round_trip() {
    use dtlvnative::usearch::{Binary, Error, metadata, metadata_buffer};
    for metric in [
        Metric::Jaccard,
        Metric::Tanimoto,
        Metric::Hamming,
        Metric::Sorensen,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("index.usearch");
        let mut index = Index::new(IndexOptions {
            dimensions: 16,
            metric,
            quantization: Scalar::Binary,
            ..Default::default()
        })
        .unwrap();
        index.reserve(4).unwrap();
        let vector = [Binary(0x03), Binary(0x01)];
        index.add(42, &vector).unwrap();
        let bytes = index.to_bytes().unwrap();
        index.save(&file).unwrap();
        // SAFETY: These bytes and the unchanged file were just produced by this
        // same runtime. Their owners outlive the corresponding views.
        unsafe {
            assert_eq!(metadata_buffer(&bytes).unwrap().metric, metric);
            assert_eq!(metadata(&file).unwrap().metric, metric);
            let mut view = Index::view_buffer(&bytes).unwrap();
            assert_eq!(view.search(&vector, 1).unwrap().keys, [42]);
            let mut mapped = Index::view_file(&file).unwrap();
            assert_eq!(mapped.search(&vector, 1).unwrap().keys, [42]);
            assert!(matches!(mapped.add(7, &vector), Err(Error::ReadOnly)));
            assert!(matches!(mapped.clear(), Err(Error::ReadOnly)));
            let mut copy = Index::new(IndexOptions {
                dimensions: 1,
                ..Default::default()
            })
            .unwrap();
            copy.load_buffer(&bytes).unwrap();
            assert_eq!(copy.dimensions(), 16);
            assert_eq!(copy.get::<Binary>(42, 1).unwrap(), vec![vector.to_vec()]);
            copy.load(&file).unwrap();
            assert_eq!(copy.search(&vector, 1).unwrap().distances, [0.]);
        }
    }
}

#[test]
fn native_scalar_conversions_and_query_validation() {
    use dtlvnative::usearch::{BFloat16, F16};
    for scalar in [
        Scalar::F32,
        Scalar::F64,
        Scalar::F16,
        Scalar::BFloat16,
        Scalar::I8,
        Scalar::U8,
        Scalar::E5M2,
        Scalar::E4M3,
        Scalar::E3M2,
        Scalar::E2M3,
    ] {
        let mut index = Index::new(IndexOptions {
            dimensions: 2,
            metric: Metric::Cosine,
            quantization: scalar,
            ..Default::default()
        })
        .unwrap();
        index.reserve(4).unwrap();
        index.add(1, &[1_f32, 2.]).unwrap();
        assert_eq!(index.search(&[1_f64, 2.], 1).unwrap().keys, [1]);
        assert_eq!(index.get::<F16>(1, 1).unwrap()[0].len(), 2);
        assert_eq!(index.get::<BFloat16>(1, 1).unwrap()[0].len(), 2);
        assert!(index.add(2, &[1_f32]).is_err());
        assert!(index.search(&[1_f32], 1).is_err());
        assert!(index.set_threads_search(0).is_err());
        assert!(!index.hardware_acceleration().unwrap().is_empty());
    }
}

#[test]
fn floating_metrics_and_runtime_information() {
    use dtlvnative::usearch::{distance, version};
    assert!(!version().unwrap().is_empty());
    for metric in [
        Metric::Cosine,
        Metric::InnerProduct,
        Metric::L2Squared,
        Metric::Haversine,
        Metric::Divergence,
        Metric::Pearson,
    ] {
        let first = [0.25_f32, 0.75];
        let second = [0.75_f32, 0.25];
        assert!(distance(&first, &second, 2, metric).unwrap().is_finite());
        let mut index = Index::new(IndexOptions {
            dimensions: 2,
            metric,
            ..Default::default()
        })
        .unwrap();
        index.reserve(4).unwrap();
        index.add(1, &first).unwrap();
        assert_eq!(index.search(&first, 1).unwrap().keys, [1]);
        assert!(index.capacity().unwrap() >= 4);
        assert!(index.connectivity().unwrap() > 0);
        assert!(index.memory_usage().unwrap() > 0);
    }
}
