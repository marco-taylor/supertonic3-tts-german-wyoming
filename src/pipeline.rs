use crate::{App, EngineLease, audio, normalize, segment, wire};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc, time::Instant};
use tokio::{
    io::{AsyncBufRead, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

struct Segment {
    text: String,
    prepend_silence: bool,
}
fn enqueue(queue: &mut VecDeque<Segment>, texts: Vec<String>, language: &str, phrase: bool) {
    let texts = if phrase {
        texts
            .into_iter()
            .flat_map(|text| {
                segment::bounded(&text, supertonic3_tts::helper::max_chunk_length(language))
            })
            .collect()
    } else {
        texts
    };
    for text in texts {
        for (index, part) in supertonic3_tts::helper::chunk_text(
            &text,
            Some(supertonic3_tts::helper::max_chunk_length(language)),
        )
        .into_iter()
        .enumerate()
        {
            queue.push_back(Segment {
                text: part,
                prepend_silence: index > 0,
            });
        }
    }
}
pub async fn respond<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    app: Arc<App>,
    data: &Value,
    input_stream: bool,
    stop: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let received = Instant::now();
    let (voice, options) = crate::options::resolve(
        data,
        &app.config.voice,
        app.config.synthesis_options(),
        &app.config.voice_profiles,
    )?;
    let style = app.voices.get(voice).context("unknown voice")?.clone();
    let language = normalize::language(
        data["voice"]["language"]
            .as_str()
            .unwrap_or(&app.config.language),
    )
    .to_owned();
    ensure!(
        supertonic3_tts::is_valid_lang(&language),
        "unsupported language"
    );
    ensure!(
        data["text_format"].as_str().is_none_or(|v| v == "text"),
        "only plain text supported"
    );
    let mut pending = VecDeque::new();
    let mut seen = String::new();
    let mut segmenter = segment::Segmenter::new();
    let mut normalizer = normalize::Normalizer::new(app.config.german_normalization, &language);
    let mut input_done = !input_stream;
    if !input_stream {
        let text = data["text"].as_str().context("missing text")?;
        ensure!(
            !text.trim().is_empty() && text.len() <= 16384,
            "text is empty or too long"
        );
        let text = normalize::apply(text, app.config.german_normalization, &language);
        if app.config.streaming {
            enqueue(
                &mut pending,
                segment::sentences(&text),
                &language,
                app.config.streaming,
            );
        } else {
            enqueue(
                &mut pending,
                vec![text.into_owned()],
                &language,
                app.config.streaming,
            );
        }
    }
    let permit = app.permits.clone().acquire_owned().await?;
    let index = app.free.lock().unwrap().pop().context("no free engine")?;
    let lease = EngineLease {
        app: app.clone(),
        index,
        _permit: permit,
    };
    let (text_tx, mut text_rx) = mpsc::channel::<Segment>(1);
    // At most one completed floating-point segment waits for the socket writer.
    let (audio_tx, mut audio_rx) = mpsc::channel::<Result<Vec<f32>>>(1);
    let runtime = tokio::runtime::Handle::current();
    let worker_app = app.clone();
    let worker_language = language.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let mut count = 0usize;
        let mut synthesis_seconds = 0.0;
        while let Some(segment) = text_rx.blocking_recv() {
            if segment.prepend_silence {
                let silence =
                    vec![0.0; (supertonic3_tts::DEFAULT_SILENCE_DURATION * 44100.0) as usize];
                if audio_tx.blocking_send(Ok(silence)).is_err() {
                    break;
                }
            }
            let text = segment.text;
            let begin = Instant::now();
            let result = runtime.block_on(worker_app.engines[lease.index].synthesize(
                &text,
                &style,
                options.speed,
                &worker_language,
                options.steps,
            ));
            let segment_seconds = begin.elapsed().as_secs_f64();
            synthesis_seconds += segment_seconds;
            match result {
                Ok(audio) => {
                    if audio.is_empty() || audio.iter().any(|v| !v.is_finite()) {
                        let _ =
                            audio_tx.blocking_send(Err(anyhow::anyhow!("invalid engine audio")));
                        break;
                    }
                    tracing::info!(
                        segment = count,
                        samples = audio.len(),
                        segment_seconds,
                        "segment synthesized"
                    );
                    // Ownership transfer, no whole-segment clone and no whole-WAV PCM buffer.
                    if audio_tx.blocking_send(Ok(audio)).is_err() {
                        break;
                    }
                    count += 1;
                }
                Err(error) => {
                    let _ = audio_tx.blocking_send(Err(error));
                    break;
                }
            }
        }
        tracing::info!(
            synthesis_seconds,
            segments = count,
            steps = options.steps,
            speed = options.speed,
            "pipeline synthesis complete"
        );
        drop(lease);
    });
    let mut text_tx = Some(text_tx);
    let mut prefetched = VecDeque::new();
    let mut prefill_samples = 0usize;
    let target_prefill = if app.config.streaming {
        app.config.streaming_prefill_ms * 44100 / 1000
    } else {
        0
    };
    let mut started = false;
    let mut pcm = Vec::with_capacity(app.config.chunk_bytes);
    let full_prefix = audio::chunk_prefix(app.config.chunk_bytes);
    let mut audio_seconds = 0.0;
    let mut audio_start_seconds = 0.0;
    let mut ttfa_seconds = None;
    // Keep a persistent reader future: cancelling wire::read after a partial
    // TCP frame would discard bytes already consumed into its local buffer.
    let (input_tx, mut input_rx) = mpsc::channel::<Result<Option<Value>>>(1);
    let read_input = async move {
        loop {
            let result =
                match tokio::time::timeout(std::time::Duration::from_secs(300), wire::read(reader))
                    .await
                {
                    Ok(result) => result,
                    Err(error) => Err(error.into()),
                };
            let last = match &result {
                Ok(Some(event)) => event["type"].as_str() == Some("synthesize-stop"),
                _ => true,
            };
            if input_tx.send(result).await.is_err() || last {
                break;
            }
        }
    };
    tokio::pin!(read_input);
    let mut input_reader_done = false;
    loop {
        if input_done && pending.is_empty() {
            text_tx.take();
        }
        tokio::select! {
            block=audio_rx.recv()=>{
                if let Some(block)=block {
                    let block=block?;
                    prefill_samples+=block.len();
                    prefetched.push_back(block);
                }
            },
            result=async {text_tx.as_ref().unwrap().reserve().await},if text_tx.is_some() && !pending.is_empty()=>{
                result?.send(pending.pop_front().unwrap());
            },
            _=stop.changed(),if input_stream && !input_done=>anyhow::bail!("server shutting down"),
            _=&mut read_input,if input_stream && !input_done && !input_reader_done=>{input_reader_done=true;},
            event=input_rx.recv(),if input_stream && !input_done=>{
                let event=event.context("input reader ended")??.context("stream disconnected before synthesize-stop")?;
                match event["type"].as_str().unwrap_or("") {
                    "synthesize-chunk"=>{
                        let text=event["data"]["text"].as_str().context("missing streamed text")?;
                        ensure!(seen.len()+text.len()<=16384,"streamed text too long");
                        seen.push_str(text);
                        if app.config.streaming {
                            segmenter.push(&normalizer.push(text,false));enqueue(&mut pending,segmenter.drain(false),&language,app.config.streaming);
                        }
                    },
                    "synthesize"=>{
                        // Home Assistant sends the accumulated full message for compatibility.
                        // It must never trigger a second synthesis of already streamed text.
                        let text=event["data"]["text"].as_str().context("missing fallback text")?;
                        ensure!(text.len()<=16384,"streamed text too long");
                        if seen.is_empty() {
                            seen.push_str(text);
                            if app.config.streaming {segmenter.push(&normalizer.push(text,false));enqueue(&mut pending,segmenter.drain(false),&language,app.config.streaming);}
                        } else {ensure!(text==seen,"compatibility text differs from streamed text");}
                    },
                    "synthesize-stop"=>{
                        ensure!(!seen.trim().is_empty(),"empty streamed text");
                        if app.config.streaming {segmenter.push(&normalizer.push("",true));enqueue(&mut pending,segmenter.drain(true),&language,app.config.streaming);}
                        else {enqueue(&mut pending,vec![normalize::apply(&seen,app.config.german_normalization,&language).into_owned()],&language,app.config.streaming);}
                        input_done=true;
                    },
                    _=>anyhow::bail!("unexpected streaming-input event"),
                }
            },
            else=>break,
        }
        if !prefetched.is_empty()
            && (started
                || prefill_samples >= target_prefill
                || prefetched.len() >= 2
                || audio_rx.is_closed())
        {
            if !started {
                wire::write(
                    writer,
                    "audio-start",
                    json!({"rate":44100,"width":2,"channels":1}),
                    &[],
                )
                .await?;
                started = true;
                audio_start_seconds = received.elapsed().as_secs_f64();
            }
            while let Some(block) = prefetched.pop_front() {
                audio_seconds += block.len() as f64 / 44100.0;
                for samples in block.chunks(app.config.chunk_bytes / 2) {
                    audio::pcm_into(samples, &mut pcm);
                    if pcm.len() == app.config.chunk_bytes {
                        writer.write_all(&full_prefix).await?;
                    } else {
                        writer.write_all(&audio::chunk_prefix(pcm.len())).await?;
                    }
                    writer.write_all(&pcm).await?;
                    writer.flush().await?;
                    ttfa_seconds.get_or_insert(received.elapsed().as_secs_f64());
                }
            }
        }
        if audio_rx.is_closed() && audio_rx.is_empty() {
            break;
        }
    }
    worker.await?;
    ensure!(started, "no audio produced");
    wire::write(writer, "audio-stop", json!({}), &[]).await?;
    if input_stream {
        wire::write(writer, "synthesize-stopped", json!({}), &[]).await?;
    }
    tracing::info!(
        audio_start_seconds,
        ttfa_seconds = ttfa_seconds.unwrap_or(0.0),
        total_seconds = received.elapsed().as_secs_f64(),
        audio_seconds,
        "request complete"
    );
    Ok(())
}
