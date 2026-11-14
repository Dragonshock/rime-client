use voice_tensor::{
    broadcast_shape, conv2d_spatial_out, conv_stem_out_len, f16_clip_limit, position_half_dim,
    AttentionShape, AxisSlice, Conv2d, DType, FakeBackend, FakeTensor, KvCache, LayerNorm, Linear,
    PadWidth, RecordedCall, RmsNorm, Storage, TensorMeta, TensorOps, AUDIO_END, AUDIO_PAD,
    AUDIO_START, CHUNK_FRAMES, D_MODEL, ENCODER_HEADS, ENCODER_HEAD_DIM, ENCODER_LAYERS,
    ENCODER_LINEAR_BIAS, GROUP_SIZE, HIDDEN, KV_HEADS, MEL_BINS, QUERY_HEADS, QWEN3_ASR_0_6B,
    STEM_FREQ, STEM_LINEAR_IN, TEXT_HEAD_DIM, TEXT_LAYERS, TEXT_LINEAR_BIAS, TOKENS_PER_CHUNK,
    VOCAB_SIZE,
};

#[test]
fn profile_matches_qwen3_asr_0_6b() {
    let profile = QWEN3_ASR_0_6B;
    assert_eq!(profile.encoder_layers, 18);
    assert_eq!(ENCODER_LAYERS, 18);
    assert_eq!(profile.d_model, 896);
    assert_eq!(D_MODEL, 896);
    assert_eq!(profile.text_layers, 28);
    assert_eq!(TEXT_LAYERS, 28);
    assert_eq!(profile.hidden, 1024);
    assert_eq!(HIDDEN, 1024);
    assert_eq!(profile.query_heads, 16);
    assert_eq!(QUERY_HEADS, 16);
    assert_eq!(profile.kv_heads, 8);
    assert_eq!(KV_HEADS, 8);
    assert_eq!(profile.vocab, 151_936);
    assert_eq!(VOCAB_SIZE, 151_936);
    assert_eq!(profile.audio_pad, 151_676);
    assert_eq!(AUDIO_PAD, 151_676);

    assert_eq!(profile.audio_start, 151_669);
    assert_eq!(AUDIO_START, 151_669);
    assert_eq!(profile.audio_end, 151_670);
    assert_eq!(AUDIO_END, 151_670);
    assert!(AUDIO_START < VOCAB_SIZE as u32);
    assert!(AUDIO_PAD < VOCAB_SIZE as u32);
    assert!(AUDIO_END < VOCAB_SIZE as u32);

    assert_eq!(profile.mel_bins, 128);
    assert_eq!(MEL_BINS, 128);
    assert_eq!(profile.encoder_heads, 14);
    assert_eq!(ENCODER_HEADS, 14);
    assert_eq!(profile.encoder_head_dim, 64);
    assert_eq!(ENCODER_HEAD_DIM, 64);
    assert_eq!(ENCODER_HEADS * ENCODER_HEAD_DIM, D_MODEL);
    assert_eq!(profile.encoder_ffn, 3584);
    assert_eq!(profile.encoder_ffn, D_MODEL * 4);
    assert_eq!(profile.projection_out, 1024);
    assert_eq!(profile.projection_out, HIDDEN);
    assert_eq!(profile.n_window, 50);
    assert_eq!(profile.chunk_frames, 100);
    assert_eq!(CHUNK_FRAMES, 100);
    assert_eq!(profile.n_window_infer, 800);
    assert_eq!(profile.downsample_hidden, 480);
    assert!(profile.encoder_linear_bias);
    assert_eq!(ENCODER_LINEAR_BIAS, profile.encoder_linear_bias);
    assert!(!profile.text_linear_bias);
    assert_eq!(TEXT_LINEAR_BIAS, profile.text_linear_bias);
    assert_eq!(
        profile.conv_stem.map(|layer| (layer.in_ch, layer.out_ch)),
        [(1, 480), (480, 480), (480, 480),]
    );
    assert!(profile
        .conv_stem
        .iter()
        .all(|layer| layer.kernel == 3 && layer.stride == 2 && layer.pad == 1));
    assert_eq!(profile.stem_freq, 16);
    assert_eq!(STEM_FREQ, 16);
    assert_eq!(
        conv_stem_out_len(conv_stem_out_len(conv_stem_out_len(128))),
        16
    );
    assert_eq!(profile.stem_linear_in, 7680);
    assert_eq!(STEM_LINEAR_IN, 7680);
    assert_eq!(STEM_LINEAR_IN % GROUP_SIZE, 0);
    assert_eq!(profile.tokens_per_chunk, 13);
    assert_eq!(TOKENS_PER_CHUNK, 13);
    assert_eq!(
        conv_stem_out_len(conv_stem_out_len(conv_stem_out_len(100))),
        13
    );
    assert_eq!(profile.max_position, 1500);
    assert_eq!(position_half_dim(D_MODEL), 448);
    assert_eq!(position_half_dim(D_MODEL) - 1, 447);
    assert_eq!(profile.window_split_at, 20);
    assert_eq!(profile.f16_clip_margin, 1000);
    assert_eq!(f16_clip_limit(), 64504.0);
    assert_eq!(profile.intermediate, 3072);
    assert_eq!(profile.intermediate, HIDDEN * 3);
    // 文本塔头维是 128。查询投影宽度是 16×128，不是隐层 1024。
    assert_eq!(profile.text_head_dim, 128);
    assert_eq!(TEXT_HEAD_DIM, 128);
    assert_eq!(QUERY_HEADS * TEXT_HEAD_DIM, 2048);
    assert_eq!(KV_HEADS * TEXT_HEAD_DIM, HIDDEN);
    assert_ne!(QUERY_HEADS * TEXT_HEAD_DIM, HIDDEN);
    assert_eq!(profile.rope_theta, 1_000_000.0);
    assert_eq!(profile.rope_sections, 3);
    assert_eq!(profile.rms_eps, 1e-6);
    assert!(profile.tie_embeddings);
    assert_eq!(profile.group_size, 64);
    assert_eq!(GROUP_SIZE, 64);
    assert_eq!(QUERY_HEADS % KV_HEADS, 0);
}

#[test]
fn package_does_not_depend_on_mlx() {
    let package = include_str!("../Cargo.toml");
    assert!(!package.to_ascii_lowercase().contains("mlx"));
}

#[test]
fn fake_records_linear_conv_and_attention_shapes() {
    let mut backend = FakeBackend::new();
    let mut hidden = FakeTensor::dense(&[1, CHUNK_FRAMES, MEL_BINS, 1], DType::F32);
    for stem in QWEN3_ASR_0_6B.conv_stem {
        let weight = FakeTensor::dense(&[stem.out_ch, 3, 3, stem.in_ch], DType::F32);
        let bias = FakeTensor::dense(&[stem.out_ch], DType::F32);
        let layer = Conv2d::new(weight, Some(bias)).unwrap();
        hidden = backend.conv2d(&hidden, &layer).unwrap();
        hidden = backend.gelu(&hidden).unwrap();
    }
    assert_eq!(
        hidden.dims(),
        &[
            1,
            TOKENS_PER_CHUNK,
            STEM_FREQ,
            QWEN3_ASR_0_6B.downsample_hidden
        ]
    );

    let flat = backend
        .reshape(
            &hidden,
            &[1, TOKENS_PER_CHUNK as isize, STEM_LINEAR_IN as isize],
        )
        .unwrap();
    let proj_w = FakeTensor::grouped(&[D_MODEL, STEM_LINEAR_IN], DType::F32, GROUP_SIZE).unwrap();
    let proj_b = FakeTensor::dense(&[D_MODEL], DType::F32);
    let proj = Linear::new(proj_w, Some(proj_b)).unwrap();
    let projected = backend.linear(&flat, &proj).unwrap();
    assert_eq!(projected.dims(), &[1, TOKENS_PER_CHUNK, D_MODEL]);

    let heads = ENCODER_HEADS;
    let head_dim = ENCODER_HEAD_DIM;
    let query = FakeTensor::dense(&[1, heads, TOKENS_PER_CHUNK, head_dim], DType::F32);
    let key = FakeTensor::dense(&[1, heads, TOKENS_PER_CHUNK, head_dim], DType::F32);
    let value = query.clone();
    let attended = backend.attention(&query, &key, &value, None).unwrap();
    assert_eq!(attended.dims(), query.dims());

    let convs = backend.conv_shapes();
    assert_eq!(convs.len(), 3);
    assert_eq!(convs[0].input.dims(), &[1, 100, 128, 1]);
    assert_eq!(convs[0].output.dims(), &[1, 50, 64, 480]);
    assert_eq!(
        (
            convs[0].in_ch,
            convs[0].out_ch,
            convs[0].kernel,
            convs[0].stride,
            convs[0].pad
        ),
        (1, 480, 3, 2, 1)
    );
    assert_eq!(convs[1].output.dims(), &[1, 25, 32, 480]);
    assert_eq!(convs[2].output.dims(), &[1, 13, 16, 480]);

    let linears = backend.linear_shapes();
    assert_eq!(linears.len(), 1);
    assert_eq!(linears[0].input.dims(), &[1, 13, 7680]);
    assert_eq!(linears[0].weight.dims(), &[896, 7680]);
    assert!(linears[0].bias);
    assert!(linears[0].grouped);
    assert_eq!(linears[0].output.dims(), &[1, 13, 896]);

    let attentions = backend.attention_shapes();
    assert_eq!(
        attentions,
        vec![AttentionShape {
            query: query.shape_view(),
            key: key.shape_view(),
            value: query.shape_view(),
            mask: None,
            output: query.shape_view(),
        }]
    );
}

trait ShapeView {
    fn shape_view(&self) -> voice_tensor::Shape;
}

impl ShapeView for FakeTensor {
    fn shape_view(&self) -> voice_tensor::Shape {
        voice_tensor::Shape::new(self.dims().to_vec())
    }
}

#[test]
fn text_attention_accepts_grouped_query_and_kv_cache() {
    let mut backend = FakeBackend::new();
    let weight = FakeTensor::grouped(&[2048, HIDDEN], DType::F16, GROUP_SIZE).unwrap();
    assert!(matches!(
        weight.storage(),
        Storage::Grouped {
            group_size: 64,
            bits: 4,
        }
    ));
    let layer = Linear::new(weight, None).unwrap();
    assert!(!layer.has_bias());
    let input = FakeTensor::dense(&[1, 1, HIDDEN], DType::F16);
    let projected = backend.linear(&input, &layer).unwrap();
    assert_eq!(projected.dims(), &[1, 1, QUERY_HEADS * TEXT_HEAD_DIM]);
    assert_eq!(projected.dtype(), DType::F16);

    let query = FakeTensor::dense(&[1, QUERY_HEADS, 1, TEXT_HEAD_DIM], DType::F16);
    let key = FakeTensor::dense(&[1, KV_HEADS, 4, TEXT_HEAD_DIM], DType::F16);
    let value = key.clone();
    let mask = FakeTensor::dense(&[1, 1, 1, 4], DType::F16);
    let step = backend
        .attention(&query, &key, &value, Some(&mask))
        .unwrap();
    assert_eq!(step.dims(), &[1, QUERY_HEADS, 1, TEXT_HEAD_DIM]);

    let mut cache = KvCache::new();
    backend.kv_write(&mut cache, &key, &value).unwrap();
    let next_key = FakeTensor::dense(&[1, KV_HEADS, 1, TEXT_HEAD_DIM], DType::F16);
    let next_value = next_key.clone();
    backend
        .kv_write(&mut cache, &next_key, &next_value)
        .unwrap();
    let (cached_key, cached_value) = backend.kv_read(&cache).unwrap();
    assert_eq!(cached_key.dims(), &[1, KV_HEADS, 5, TEXT_HEAD_DIM]);
    assert_eq!(cached_value.dims(), cached_key.dims());
    assert!(!cache.is_empty());

    let attention = backend.attention_shapes();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].mask.as_ref().unwrap().dims(), &[1, 1, 1, 4]);
    assert!(backend
        .calls()
        .iter()
        .any(|call| matches!(call, RecordedCall::KvWrite { cached_time: 5, .. })));
}

#[test]
fn shape_ops_follow_the_trait() {
    let mut backend = FakeBackend::new();
    let pcm = backend.pcm_to_f32(&[0, 16, -16]).unwrap();
    assert_eq!(pcm.dims(), &[3]);
    assert_eq!(pcm.dtype(), DType::F32);

    let zeros = backend.zeros(&[2, 3], DType::F16).unwrap();
    let range = backend.arange(0, 10, 3, DType::I32).unwrap();
    assert_eq!(range.dims(), &[4]);
    let backward = backend.arange(10, 1, -3, DType::I32).unwrap();
    assert_eq!(backward.dims(), &[3]);
    let normal = backend.normal(&[4], 0.0, 1.0, DType::F32).unwrap();
    assert_eq!(normal.dims(), &[4]);

    let reshaped = backend.reshape(&zeros, &[3, -1]).unwrap();
    assert_eq!(reshaped.dims(), &[3, 2]);
    let turned = backend.transpose(&reshaped, &[1, 0]).unwrap();
    assert_eq!(turned.dims(), &[2, 3]);
    let sliced = backend
        .slice(&turned, &[AxisSlice::full(2), AxisSlice::new(1, 3)])
        .unwrap();
    assert_eq!(sliced.dims(), &[2, 2]);
    let padded = backend
        .pad(&sliced, &[PadWidth::none(), PadWidth::new(1, 1)])
        .unwrap();
    assert_eq!(padded.dims(), &[2, 4]);

    let left = FakeTensor::dense(&[2, 3], DType::F32);
    let right = FakeTensor::dense(&[4, 3], DType::F32);
    let joined = backend.concatenate(&[&left, &right], 0).unwrap();
    assert_eq!(joined.dims(), &[6, 3]);

    let cond = FakeTensor::dense(&[1, 1, 4], DType::Bool);
    let when_true = FakeTensor::dense(&[2, 3, 4], DType::F32);
    let when_false = FakeTensor::dense(&[4], DType::F32);
    let chosen = backend.where_op(&cond, &when_true, &when_false).unwrap();
    assert_eq!(chosen.dims(), &[2, 3, 4]);

    let total = backend.cumsum(&joined, 0).unwrap();
    assert_eq!(total.dims(), joined.dims());
    let reduced = backend.sum(&joined, 1, false).unwrap();
    assert_eq!(reduced.dims(), &[6]);
    let kept = backend.max(&joined, 1, true).unwrap();
    assert_eq!(kept.dims(), &[6, 1]);

    let table = FakeTensor::dense(&[VOCAB_SIZE, HIDDEN], DType::F16);
    let index = FakeTensor::dense(&[1, 8], DType::I32);
    let embedded = backend.gather(&table, &index, 0).unwrap();
    assert_eq!(embedded.dims(), &[1, 8, HIDDEN]);

    let position = FakeTensor::dense(&[TOKENS_PER_CHUNK, D_MODEL], DType::F16);
    let hidden = FakeTensor::dense(&[1, TOKENS_PER_CHUNK, D_MODEL], DType::F16);
    let added = backend.add(&hidden, &position).unwrap();
    assert_eq!(added.dims(), &[1, TOKENS_PER_CHUNK, D_MODEL]);
    let wave = backend.sin(&position).unwrap();
    assert_eq!(wave.dims(), position.dims());
    let scaled = backend
        .mul(&wave, &FakeTensor::dense(&[1], DType::F16))
        .unwrap();
    assert_eq!(scaled.dims(), position.dims());

    let norm_w = FakeTensor::dense(&[D_MODEL], DType::F16);
    let norm_b = FakeTensor::dense(&[D_MODEL], DType::F16);
    let layer_norm = LayerNorm::new(norm_w, Some(norm_b), 1e-5).unwrap();
    let normalized = backend.layer_norm(&added, &layer_norm).unwrap();
    assert_eq!(normalized.dims(), added.dims());
    let rms = RmsNorm::new(
        FakeTensor::dense(&[HIDDEN], DType::F16),
        QWEN3_ASR_0_6B.rms_eps,
    )
    .unwrap();
    let text = FakeTensor::dense(&[1, 4, HIDDEN], DType::F16);
    assert_eq!(backend.rms_norm(&text, &rms).unwrap().dims(), text.dims());
    let clipped = backend
        .clip(&text, -f16_clip_limit(), f16_clip_limit())
        .unwrap();
    assert_eq!(clipped.dims(), text.dims());
    assert_eq!(backend.silu(&text).unwrap().dims(), text.dims());
    assert_eq!(backend.exp(&text).unwrap().dtype(), DType::F16);
    assert_eq!(backend.rsqrt(&text).unwrap().dims(), &[1, 4, HIDDEN]);
    assert_eq!(backend.cos(&text).unwrap().dims(), text.dims());
    assert_eq!(backend.min(&text, 0, false).unwrap().dims(), &[4, HIDDEN]);

    assert_eq!(
        broadcast_shape(&[1, 13, 896], &[13, 896]).unwrap().dims(),
        &[1, 13, 896]
    );
    assert_eq!(conv2d_spatial_out(7, 3, 2, 1).unwrap(), 4);
    backend.clear();
    assert!(backend.calls().is_empty());
}

#[test]
fn rejected_calls_are_not_recorded() {
    let mut backend = FakeBackend::new();
    let bad_kernel = FakeTensor::dense(&[4, 1, 1, 1], DType::F32);
    assert!(Conv2d::new(bad_kernel, None).is_err());

    let input = FakeTensor::dense(&[1, 4], DType::F16);
    let weight = FakeTensor::dense(&[4, 8], DType::F32);
    let layer = Linear::new(weight, None).unwrap();
    let error = backend.linear(&input, &layer).unwrap_err();
    assert!(error.to_string().contains("linear"));
    assert!(backend.calls().is_empty());

    assert!(FakeTensor::grouped(&[4, 30], DType::F32, GROUP_SIZE).is_err());
    assert!(backend.arange(0, 4, 0, DType::I32).is_err());
    assert!(backend.calls().is_empty());

    let query = FakeTensor::dense(&[1, 16, 2, 128], DType::F32);
    let key = FakeTensor::dense(&[1, 6, 2, 128], DType::F32);
    assert!(backend.attention(&query, &key, &key, None).is_err());
    assert!(backend.kv_read(&KvCache::new()).is_err());
    assert!(backend.calls().is_empty());

    let short = FakeTensor::dense(&[1, 0, 5, 1], DType::F32);
    let conv_w = FakeTensor::dense(&[1, 3, 3, 1], DType::F32);
    let conv = Conv2d::new(conv_w, None).unwrap();
    assert!(backend.conv2d(&short, &conv).is_err());
    assert!(backend.calls().is_empty());
}
