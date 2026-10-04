use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
pub async fn read<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Value>> {
    let mut line = Vec::new();
    // Bounded header: read_until alone can grow without limit.
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            ensure!(line.is_empty(), "truncated header");
            return Ok(None);
        }
        let n = buffer
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(buffer.len());
        ensure!(line.len() + n <= 65536, "header too large");
        line.extend_from_slice(&buffer[..n]);
        reader.consume(n);
        if line.last() == Some(&b'\n') {
            break;
        }
    }
    let mut event: Value = serde_json::from_slice(&line)?;
    ensure!(event["type"].is_string(), "missing event type");
    for (key, limit) in [("data_length", 1048576u64), ("payload_length", 1048576u64)] {
        let length = match event.get(key) {
            None => 0,
            Some(v) => v
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("invalid length"))?,
        };
        ensure!(length <= limit, "event too large");
        let mut bytes = vec![0; length as usize];
        reader.read_exact(&mut bytes).await?;
        if key == "data_length" && length > 0 {
            let data: Value = serde_json::from_slice(&bytes)?;
            ensure!(data.is_object(), "data must be an object");
            if event.get("data").is_none() {
                event["data"] = json!({});
            }
            let target = event["data"]
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("data must be an object"))?;
            target.extend(data.as_object().unwrap().clone());
        }
    }
    Ok(Some(event))
}
pub async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: &str,
    data: Value,
    payload: &[u8],
) -> Result<()> {
    let bytes = serde_json::to_vec(&data)?;
    let mut header = json!({"type":kind,"data_length":bytes.len()});
    if !payload.is_empty() {
        header["payload_length"] = json!(payload.len());
    }
    let mut line = serde_json::to_vec(&header)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.write_all(&bytes).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn external_data_merges_with_inline_utf8() {
        let body = r#"{"text":"Grüße"}"#;
        let message = format!(
            "{{\"type\":\"synthesize\",\"data\":{{\"voice\":{{\"name\":\"F2\"}}}},\"data_length\":{}}}\n{}",
            body.len(),
            body
        );
        let mut reader = tokio::io::BufReader::new(message.as_bytes());
        let event = read(&mut reader).await.unwrap().unwrap();
        assert_eq!(event["data"]["text"], "Grüße");
        assert_eq!(event["data"]["voice"]["name"], "F2");
    }
    #[tokio::test]
    async fn oversized_event_is_rejected() {
        let mut reader = tokio::io::BufReader::new(
            b"{\"type\":\"synthesize\",\"data_length\":99999999}\n".as_slice(),
        );
        assert!(read(&mut reader).await.is_err());
    }
}
