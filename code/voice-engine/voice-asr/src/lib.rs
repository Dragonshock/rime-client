//! Qwen3-ASR 的音频塔、文本塔与流式状态。
//!
//! 图的数值求值留在张量后端。本箱的测试用假后端驱动 `feed` 与 `finish`，不启动 GPU。
//! 键盘组合不在这里。

mod config;
mod encoder;
mod error;
mod prefix;
mod session;
mod state;

pub use config::{
    audio_tokens, conv_out_len, AsrConfig, Conv2dSpec, EncoderConfig, TextTowerConfig,
};
pub use error::EngineError;
pub use prefix::{audio_placeholder_ids, build_prefix, render_prefix, PrefixPart};
pub use session::{DecodeText, DecodeView, Session, StepOutput, TranscriptEvent, TranscriptSink};
pub use state::{EncoderPrefix, KvCache, KvLayer, MelPending, StreamState};

/// 16 kHz 下 2.0 秒的样本数。
pub const STEP_SAMPLES: usize = 32_000;

#[cfg(test)]
mod tests {
    use super::*;
    use voice_tensor::{FakeBackend, RecordedCall};

    struct Fake {
        partial: &'static str,
        final_text: &'static str,
        calls: Vec<bool>,
        empty_written: bool,
        saw_hotword: bool,
        last_active_frames: usize,
        last_committed: usize,
    }

    impl DecodeText for Fake {
        fn decode_text(&mut self, view: &DecodeView<'_>) -> StepOutput {
            self.calls.push(view.is_final);
            self.last_active_frames = view.active_frames;
            self.last_committed = view.committed_tokens;
            self.empty_written |= view.prefix.iter().any(|part| part.written().is_empty());
            self.saw_hotword |= view
                .prefix
                .iter()
                .any(|part| matches!(part, PrefixPart::System(text) if text == "专名"));
            let text = if view.is_final {
                self.final_text
            } else {
                self.partial
            };
            StepOutput {
                text: text.to_string(),
                tokens: vec![11, 12, 13, 14, 15, 16, 17],
            }
        }
    }

    fn session(
        hotwords: Option<&str>,
        fake: Fake,
    ) -> Session<FakeBackend, Fake, Vec<TranscriptEvent>> {
        Session::new(hotwords, FakeBackend::new(), fake, Vec::new()).expect("假后端应能立起形状")
    }

    #[test]
    fn feed_32000_then_finish() {
        let fake = Fake {
            partial: "部分",
            final_text: "定稿",
            calls: Vec::new(),
            empty_written: false,
            saw_hotword: false,
            last_active_frames: 0,
            last_committed: 0,
        };
        let mut session = session(None, fake);
        let pcm = vec![0i16; STEP_SAMPLES];
        session.feed(&pcm).expect("编码步");
        assert!(session.tensor().calls().iter().any(|call| {
            matches!(
                call,
                RecordedCall::PcmToF32 {
                    samples: STEP_SAMPLES
                }
            )
        }));
        let conv = session.tensor().conv_shapes();
        assert_eq!(conv.len(), 6);
        assert_eq!(conv[0].in_ch, 1);
        assert_eq!(conv[0].out_ch, 480);
        assert_eq!(conv[0].kernel, 3);
        assert_eq!(conv[0].stride, 2);
        assert_eq!(conv[0].pad, 1);
        assert_eq!(conv[0].input.dims(), &[1, 128, 100, 1]);
        assert_eq!(conv[2].output.dims(), &[1, 16, 13, 480]);
        let attention = session.tensor().attention_shapes();
        let encoder_attn = attention
            .iter()
            .filter(|shape| shape.query.dims()[1] == 14)
            .count();
        let decoder_attn = attention
            .iter()
            .filter(|shape| shape.query.dims()[1] == 16)
            .count();
        assert_eq!(encoder_attn, 18);
        assert_eq!(decoder_attn, 28);
        assert_eq!(
            attention
                .iter()
                .find(|shape| shape.query.dims()[1] == 14)
                .unwrap()
                .query
                .dims(),
            &[1, 14, 26, 64]
        );
        assert_eq!(
            session.sink(),
            &vec![TranscriptEvent {
                text: "部分".to_string(),
                is_final: false,
            }]
        );
        assert!(session.state().pcm_tail.is_empty());
        assert_eq!(session.state().mel_pending.frames, 200);
        assert_eq!(session.state().mel_pending.bins, 128);
        assert_eq!(session.state().encoder_prefix.token_count, 0);
        assert_eq!(session.state().encoder_prefix.width, 1024);
        assert_eq!(session.state().unfixed, vec![13, 14, 15, 16, 17]);
        assert_eq!(session.state().stable_tokens(), &[11, 12]);
        assert_eq!(session.state().kv.layers.len(), 28);
        assert!(session.state().kv.layers.iter().all(|layer| layer.len == 2));
        assert!(!session.decoder().empty_written);

        session.finish().expect("定稿步");
        assert_eq!(session.sink().len(), 2);
        assert_eq!(
            session.sink()[1],
            TranscriptEvent {
                text: "定稿".to_string(),
                is_final: true,
            }
        );
        assert_eq!(session.decoder().calls, vec![false, true]);
    }

    #[test]
    fn finish_without_samples_callbacks_empty_final() {
        let fake = Fake {
            partial: "不应出现",
            final_text: "不应出现",
            calls: Vec::new(),
            empty_written: false,
            saw_hotword: false,
            last_active_frames: 0,
            last_committed: 0,
        };
        let mut session = session(Some(""), fake);
        assert!(session
            .prefix()
            .iter()
            .all(|part| !part.written().is_empty()));
        assert!(!session
            .prefix()
            .iter()
            .any(|part| matches!(part, PrefixPart::System(_))));
        session.finish().expect("空定稿");
        assert_eq!(
            session.sink(),
            &vec![TranscriptEvent {
                text: String::new(),
                is_final: true,
            }]
        );
        assert!(session.decoder().calls.is_empty());
        assert!(session.tensor().calls().iter().all(|call| !matches!(
            call,
            RecordedCall::PcmToF32 { .. } | RecordedCall::Conv2d { .. }
        )));
        assert_eq!(session.state().published, "");
    }

    #[test]
    fn short_of_two_seconds_does_not_callback_until_the_last_sample() {
        let fake = Fake {
            partial: "甲",
            final_text: "乙",
            calls: Vec::new(),
            empty_written: false,
            saw_hotword: false,
            last_active_frames: 0,
            last_committed: 0,
        };
        let mut session = session(Some("专名"), fake);
        session
            .feed(&vec![1i16; STEP_SAMPLES - 1])
            .expect("不足两秒");
        assert!(session.sink().is_empty());
        assert_eq!(session.state().pcm_tail.len(), 159);
        session.feed(&[1i16]).expect("凑满两秒");
        assert_eq!(session.sink().len(), 1);
        assert!(!session.sink()[0].is_final);
        assert!(session.decoder().saw_hotword);
        assert!(!session.decoder().empty_written);
    }

    #[test]
    fn an_empty_partial_step_does_not_retract_the_published_text() {
        // 说话中的一步可以没有新文本（例如松开之后跳过的那一步）。空串不下发，
        // 已经写出去的预编辑留着，由定稿决定清不清。
        let fake = Fake {
            partial: "",
            final_text: "乙",
            calls: Vec::new(),
            empty_written: false,
            saw_hotword: false,
            last_active_frames: 0,
            last_committed: 0,
        };
        let mut session = session(None, fake);
        session.feed(&vec![1i16; STEP_SAMPLES]).expect("两秒");
        assert!(session.sink().is_empty(), "空的部分结果不下发");
        session.finish().expect("定稿");
        assert_eq!(session.sink().len(), 1);
        assert!(session.sink()[0].is_final);
        assert_eq!(session.sink()[0].text, "乙");
    }

    #[test]
    fn eight_seconds_commits_one_encoder_window() {
        let fake = Fake {
            partial: "窗",
            final_text: "窗完",
            calls: Vec::new(),
            empty_written: false,
            saw_hotword: false,
            last_active_frames: 0,
            last_committed: 0,
        };
        let mut session = session(None, fake);
        session.feed(&vec![0i16; STEP_SAMPLES * 4]).expect("八秒");
        assert!(!session.sink().is_empty());
        assert!(session.sink().iter().all(|event| !event.is_final));
        assert_eq!(session.state().encoder_prefix.token_count, 104);
        assert_eq!(session.state().encoder_prefix.width, 1024);
        assert_eq!(session.state().mel_pending.frames, 0);
        assert_eq!(session.decoder().last_committed, 104);
        assert_eq!(session.decoder().last_active_frames, 0);
    }
}
