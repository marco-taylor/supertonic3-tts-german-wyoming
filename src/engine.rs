// Session tuning only. Text processing, denoising, vocoder, chunking and silence
// remain the published supertonic3-tts 1.3.0 implementation.
use crate::config::Config;
use anyhow::{Result, ensure};
use ort::session::Session;
use supertonic3_tts::{
    DEFAULT_SILENCE_DURATION, Device, Style, TtsEngine,
    helper::{TextToSpeech, UnicodeProcessor, load_cfgs},
};

pub enum Engine {
    Legacy(TtsEngine),
    Configured(tokio::sync::Mutex<TextToSpeech>),
}
impl Engine {
    pub async fn load(c: &Config) -> Result<Self> {
        if !c.ort_configured {
            return Ok(Self::Legacy(
                TtsEngine::on_device(c.models.join("onnx"), c.models.clone(), false, Device::Cpu)
                    .await?,
            ));
        }
        supertonic3_tts::helper::set_verbose(false);
        let dir = c.models.join("onnx");
        let cfg = load_cfgs(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut sessions = Vec::new();
        for name in [
            "duration_predictor.onnx",
            "text_encoder.onnx",
            "vector_estimator.onnx",
            "vocoder.onnx",
        ] {
            let mut builder = Session::builder()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .with_parallel_execution(c.ort_parallel)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .with_config_entry(
                    "session.intra_op.allow_spinning",
                    if c.ort_spin { "1" } else { "0" },
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .with_config_entry(
                    "session.inter_op.allow_spinning",
                    if c.ort_spin { "1" } else { "0" },
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if c.ort_pool == "session" {
                builder = builder
                    .with_independent_thread_pool()
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .with_intra_threads(c.threads)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .with_inter_threads(c.ort_inter)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            sessions.push(
                builder
                    .commit_from_file(dir.join(name))
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            );
        }
        let vocoder = sessions.pop().unwrap();
        let estimator = sessions.pop().unwrap();
        let encoder = sessions.pop().unwrap();
        let duration = sessions.pop().unwrap();
        let engine = TextToSpeech::new(
            cfg,
            UnicodeProcessor::new(dir.join("unicode_indexer.json"))
                .map_err(|e| anyhow::anyhow!("{e}"))?,
            duration,
            encoder,
            estimator,
            vocoder,
        );
        ensure!(engine.sample_rate == 44100, "unexpected model sample rate");
        Ok(Self::Configured(tokio::sync::Mutex::new(engine)))
    }
    pub async fn synthesize(
        &self,
        text: &str,
        style: &Style,
        speed: f32,
        lang: &str,
        steps: usize,
    ) -> Result<Vec<f32>> {
        match self {
            Self::Legacy(engine) => {
                engine
                    .synthesize_with_style(text, style, speed, 1.0, Some(lang), Some(steps))
                    .await
            }
            Self::Configured(engine) => {
                // Exactly the same helper.call invocation as TtsEngine::synthesize_with_style
                // with gain=1.0. No model or audio-quality option is altered here.
                let (wav, _) = engine
                    .lock()
                    .await
                    .call(text, lang, style, steps, speed, DEFAULT_SILENCE_DURATION)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(wav)
            }
        }
    }
}
