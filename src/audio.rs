pub const FORMAT: &str = r#"{"rate":44100,"width":2,"channels":1}"#;
pub fn pcm_into(audio: &[f32], pcm: &mut Vec<u8>) {
    pcm.clear();
    pcm.reserve(audio.len() * 2);
    pcm.extend(
        audio
            .iter()
            .copied()
            .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()),
    );
}
pub fn chunk_prefix(bytes: usize) -> Vec<u8> {
    format!(
        "{{\"type\":\"audio-chunk\",\"data_length\":{},\"payload_length\":{bytes}}}\n{FORMAT}",
        FORMAT.len()
    )
    .into_bytes()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_matches_original_byte_for_byte() {
        let mut samples = vec![-2., -1., -0.5, -0.0001, 0., 0.0001, 0.5, 1., 2.];
        samples.extend((0..100000).map(|i| (i as f32 / 99999.0) * 2.0 - 1.0));
        let original: Vec<u8> = samples
            .iter()
            .copied()
            .flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
            .collect();
        for chunk_samples in [1, 1024, 8192] {
            let mut pcm = Vec::with_capacity(chunk_samples * 2);
            let mut streamed = Vec::new();
            for chunk in samples.chunks(chunk_samples) {
                pcm_into(chunk, &mut pcm);
                streamed.extend_from_slice(&pcm);
            }
            assert_eq!(streamed, original);
        }
    }
    #[test]
    fn prepared_chunk_has_correct_framing() {
        let prefix = chunk_prefix(4096);
        let split = prefix.iter().position(|b| *b == b'\n').unwrap();
        let header: serde_json::Value = serde_json::from_slice(&prefix[..split]).unwrap();
        assert_eq!(header["payload_length"], 4096);
        assert_eq!(
            header["data_length"].as_u64().unwrap() as usize,
            prefix.len() - split - 1
        );
        let data: serde_json::Value = serde_json::from_slice(&prefix[split + 1..]).unwrap();
        assert_eq!(
            data,
            serde_json::json!({"rate":44100,"channels":1,"width":2})
        );
    }
}
