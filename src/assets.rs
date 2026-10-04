use crate::config::Config;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, path::Path};
use supertonic3_tts::{
    Style,
    helper::{StyleComponent, VoiceStyleData},
};
use tokio::io::AsyncWriteExt;
pub const REVISION: &str = "3cadd1ee6394adea1bd021217a0e650ede09a323";
pub const PRESETS: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"];
async fn download(client: &reqwest::Client, remote: &str, dest: &Path) -> Result<()> {
    if tokio::fs::try_exists(dest).await? {
        ensure!(
            tokio::fs::metadata(dest).await?.is_file(),
            "not a file: {}",
            dest.display()
        );
        tracing::info!(path=%dest.display(),"asset exists; no download");
        return Ok(());
    }
    tokio::fs::create_dir_all(dest.parent().context("asset parent")?).await?;
    let temp = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|x| x.to_str()).unwrap_or("")
    ));
    let url = format!("https://huggingface.co/Supertone/supertonic-3/resolve/{REVISION}/{remote}");
    for attempt in 1..=3 {
        let result: Result<()> = async {
            tracing::info!(remote, attempt, "downloading missing asset");
            let mut response = client.get(&url).send().await?.error_for_status()?;
            let expected = response.content_length();
            let mut file = tokio::fs::File::create(&temp).await?;
            let mut bytes = 0u64;
            while let Some(chunk) = response.chunk().await? {
                bytes += chunk.len() as u64;
                file.write_all(&chunk).await?;
            }
            ensure!(
                bytes > 0 && expected.is_none_or(|n| n == bytes),
                "incomplete download"
            );
            file.sync_all().await?;
            drop(file);
            if dest.extension().is_some_and(|x| x == "json") {
                let _: serde_json::Value = serde_json::from_slice(&tokio::fs::read(&temp).await?)?;
            }
            tokio::fs::rename(&temp, dest).await?;
            tracing::info!(remote, bytes, "asset installed");
            Ok(())
        }
        .await;
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!(%error,remote,attempt,"download failed");
                let _ = tokio::fs::remove_file(&temp).await;
                if attempt == 3 {
                    return Err(error).with_context(|| format!("download {remote}"));
                }
                tokio::time::sleep(std::time::Duration::from_secs(attempt * 2)).await;
            }
        }
    }
    unreachable!()
}
pub async fn prepare(c: &Config) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    for name in [
        "duration_predictor.onnx",
        "text_encoder.onnx",
        "tts.json",
        "unicode_indexer.json",
        "vector_estimator.onnx",
        "vocoder.onnx",
    ] {
        download(
            &client,
            &format!("onnx/{name}"),
            &c.models.join("onnx").join(name),
        )
        .await?;
    }
    download(&client, "LICENSE", &c.models.join("LICENSE")).await?;
    tokio::fs::create_dir_all(&c.voices).await?;
    for id in PRESETS {
        download(
            &client,
            &format!("voice_styles/{id}.json"),
            &c.voices.join(format!("{id}.json")),
        )
        .await?;
    }
    Ok(())
}
fn validate_component(c: &StyleComponent, expected: &[usize]) -> Result<()> {
    ensure!(c.dtype == "float32", "voice tensor must be float32");
    ensure!(
        c.dims == expected,
        "unexpected tensor dimensions {:?}; expected {:?}",
        c.dims,
        expected
    );
    ensure!(c.data.len() == expected[0], "incorrect batch dimension");
    for batch in &c.data {
        ensure!(batch.len() == expected[1], "incorrect row count");
        for row in batch {
            ensure!(row.len() == expected[2], "incorrect column count");
            ensure!(row.iter().all(|v| v.is_finite()), "non-finite tensor");
        }
    }
    Ok(())
}
pub fn validate(bytes: &[u8]) -> Result<Style> {
    let data: VoiceStyleData = serde_json::from_slice(bytes)?;
    validate_component(&data.style_ttl, &[1, 50, 256])?;
    validate_component(&data.style_dp, &[1, 8, 16])?;
    Style::from_voice_style_data(&data)
}
pub fn scan(dir: &Path) -> Result<BTreeMap<String, Style>> {
    let mut voices = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        let result: Result<(String, Style)> = (|| {
            ensure!(entry.file_type()?.is_file(), "voice must be a regular file");
            ensure!(
                entry.metadata()?.len() <= 8 * 1024 * 1024,
                "voice file too large"
            );
            let id = path
                .file_stem()
                .and_then(|x| x.to_str())
                .context("invalid filename")?
                .to_owned();
            ensure!(!id.is_empty(), "empty voice name");
            Ok((id, validate(&std::fs::read(&path)?)?))
        })();
        match result {
            Ok((id, style)) => {
                tracing::info!(%id,"registered voice");
                voices.insert(id, style);
            }
            Err(error) => tracing::warn!(path=%path.display(),%error,"ignoring invalid voice"),
        }
    }
    ensure!(!voices.is_empty(), "no valid voices");
    Ok(voices)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatible_and_mismatched_shapes() {
        let mut voice = serde_json::json!({
            "style_ttl":{"type":"float32","dims":[1,50,256],"data":vec![vec![vec![0.0f32;256];50];1]},
            "style_dp":{"type":"float32","dims":[1,8,16],"data":vec![vec![vec![0.0f32;16];8];1]}
        });
        assert!(validate(&serde_json::to_vec(&voice).unwrap()).is_ok());
        voice["style_dp"]["dims"] = serde_json::json!([1, 8, 17]);
        assert!(validate(&serde_json::to_vec(&voice).unwrap()).is_err());
        voice["style_dp"]["dims"] = serde_json::json!([1, 8, 16]);
        voice["style_ttl"]["data"][0][0] = serde_json::json!([0.0]);
        assert!(validate(&serde_json::to_vec(&voice).unwrap()).is_err());
    }
    #[test]
    fn malformed_voices_are_rejected_without_panicking() {
        for text in [
            r#"{}"#,
            r#"{"style_ttl":{"dims":[],"data":[]},"style_dp":{"dims":[],"data":[]}}"#,
            r#"{"style_ttl":{"dims":[1,50,256],"data":[]},"style_dp":{"dims":[1,8,16],"data":[]}}"#,
        ] {
            assert!(validate(text.as_bytes()).is_err());
        }
    }
}
