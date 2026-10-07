use anyhow::{Context, Result, ensure};
use std::{env, path::PathBuf};
#[derive(Clone)]
pub struct Config {
    pub voice_profiles: crate::options::VoiceProfiles,
    pub german_normalization: bool,
    pub streaming: bool,
    pub streaming_prefill_ms: usize,
    pub ort_pool: String,
    pub ort_inter: usize,
    pub ort_spin: bool,
    pub ort_parallel: bool,
    pub ort_configured: bool,
    pub chunk_bytes: usize,
    pub optimized_datapath: bool,
    pub buffered_tcp: bool,
    pub language: String,
    pub voice: String,
    pub speed: f32,
    pub steps: usize,
    pub threads: usize,
    pub concurrent: usize,
    pub wyoming: u16,
    pub http: u16,
    pub models: PathBuf,
    pub voices: PathBuf,
}
fn get(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.into())
}
fn parse<T: std::str::FromStr>(key: &str, default: &str) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    get(key, default)
        .parse()
        .with_context(|| format!("invalid {key}"))
}
impl Config {
    pub fn load() -> Result<Self> {
        let mut c = Self {
            voice_profiles: crate::options::VoiceProfiles::new(),
            german_normalization: parse("TTS_GERMAN_NORMALIZATION", "false")?,
            streaming_prefill_ms: prefill(
                env::var("TTS_STREAM_PREFILL_MS").ok().as_deref(),
                env::var("TTS_STREAMING_PREFILL_MS").ok().as_deref(),
            )?,
            streaming: parse("TTS_STREAMING", "false")?,
            ort_pool: get("ORT_THREAD_POOL", "global"),
            ort_inter: parse("ORT_INTER_THREADS", "1")?,
            ort_spin: parse("ORT_SPINNING", "false")?,
            ort_parallel: match get("ORT_EXECUTION_MODE", "sequential").as_str() {
                "sequential" => false,
                "parallel" => true,
                _ => anyhow::bail!("ORT_EXECUTION_MODE must be sequential or parallel"),
            },
            ort_configured: parse("ORT_CONFIGURED", "false")?,
            chunk_bytes: parse("WYOMING_CHUNK_BYTES", "16384")?,
            optimized_datapath: parse("TTS_OPTIMIZED_DATAPATH", "true")?,
            buffered_tcp: parse("WYOMING_BUFFERED_TCP", "true")?,
            language: crate::normalize::language(&get("TTS_LANGUAGE", "de")).to_owned(),
            voice: get("TTS_VOICE", "F1"),
            speed: parse("TTS_SPEED", "1.0")?,
            steps: parse("TTS_STEPS", "5")?,
            threads: parse("TTS_THREADS", "4")?,
            concurrent: parse("TTS_CONCURRENT_REQUESTS", "1")?,
            wyoming: parse("WYOMING_PORT", "10200")?,
            http: parse("HTTP_PORT", "8881")?,
            models: get("MODEL_DIR", "/app/models").into(),
            voices: get("VOICE_DIR", "/app/voices").into(),
        };
        ensure!(
            supertonic3_tts::is_valid_lang(&c.language),
            "unsupported language"
        );
        c.synthesis_options()
            .validate()
            .context("invalid TTS_SPEED/TTS_STEPS")?;
        c.voice_profiles = crate::options::parse_profiles(
            &get("TTS_VOICE_PROFILES", "{}"),
            c.synthesis_options(),
        )?;
        ensure!((1..=64).contains(&c.threads), "threads must be 1..64");
        ensure!(
            (1..=16).contains(&c.concurrent),
            "concurrent requests must be 1..16"
        );
        ensure!(
            c.wyoming != 0 && c.http != 0 && c.wyoming != c.http,
            "invalid ports"
        );
        ensure!(
            c.streaming_prefill_ms <= 5000,
            "streaming prefill must be 0..5000 ms"
        );
        ensure!(
            ["global", "session"].contains(&c.ort_pool.as_str()),
            "ORT_THREAD_POOL must be global or session"
        );
        ensure!(
            (1..=16).contains(&c.ort_inter),
            "inter threads must be 1..16"
        );
        ensure!(
            (512..=262144).contains(&c.chunk_bytes) && c.chunk_bytes % 2 == 0,
            "chunk bytes must be even and 512..262144"
        );
        ensure!(
            c.ort_configured || (c.ort_pool == "global" && !c.ort_parallel),
            "legacy engine supports only global sequential sessions"
        );
        Ok(c)
    }
    pub fn synthesis_options(&self) -> crate::options::SynthesisOptions {
        crate::options::SynthesisOptions {
            speed: self.speed,
            steps: self.steps,
        }
    }
}

fn prefill(short: Option<&str>, legacy: Option<&str>) -> Result<usize> {
    short
        .or(legacy)
        .unwrap_or("3500")
        .parse()
        .context("invalid TTS_STREAM_PREFILL_MS / TTS_STREAMING_PREFILL_MS")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefill_alias_precedence() {
        assert_eq!(prefill(Some("2000"), Some("3500")).unwrap(), 2000);
        assert_eq!(prefill(None, Some("1500")).unwrap(), 1500);
        assert_eq!(prefill(None, None).unwrap(), 3500);
        assert!(prefill(Some("bad"), Some("3500")).is_err());
    }
}
