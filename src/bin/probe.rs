// Independent Wyoming client: protocol and PCM checks, WAV artifacts, benchmark metrics.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
const TEXT: &str =
    "Guten Morgen. Dies ist ein Test der deutschen Sprachausgabe mit Supertonic drei.";
fn send(stream: &mut TcpStream, kind: &str, data: Value) -> Result<()> {
    let body = serde_json::to_vec(&data)?;
    writeln!(stream, "{}", json!({"type":kind,"data_length":body.len()}))?;
    stream.write_all(&body)?;
    Ok(())
}
fn receive(reader: &mut BufReader<TcpStream>) -> Result<(Value, Vec<u8>)> {
    let mut line = String::new();
    ensure!(reader.read_line(&mut line)? > 0, "unexpected EOF");
    let mut event: Value = serde_json::from_str(&line)?;
    let length = event["data_length"].as_u64().unwrap_or(0) as usize;
    ensure!(length <= 1048576, "oversized data");
    if length > 0 {
        let mut data = vec![0; length];
        reader.read_exact(&mut data)?;
        event["data"] = serde_json::from_slice(&data)?;
    }
    let length = event["payload_length"].as_u64().unwrap_or(0) as usize;
    ensure!(length <= 1048576, "oversized payload");
    let mut pcm = vec![0; length];
    reader.read_exact(&mut pcm)?;
    Ok((event, pcm))
}
fn rss() -> u64 {
    std::fs::read_to_string("/proc/1/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
        * 1024
}
fn memory() -> u64 {
    std::fs::read_to_string("/sys/fs/cgroup/memory.current")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}
fn cpu() -> u64 {
    std::fs::read_to_string("/sys/fs/cgroup/cpu.stat")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("usage_usec "))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}
fn connect(port: u16) -> Result<BufReader<TcpStream>> {
    let stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(180)))?;
    Ok(BufReader::new(stream))
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port = std::env::var("WYOMING_PORT")
        .unwrap_or("10200".into())
        .parse()?;
    let mut reader = connect(port)?;
    send(reader.get_mut(), "describe", json!({}))?;
    let (info, _) = receive(&mut reader)?;
    ensure!(info["type"] == "info", "expected info");
    let voices = info["data"]["tts"][0]["voices"]
        .as_array()
        .context("missing voices")?;
    for id in ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"] {
        ensure!(
            voices.iter().any(|v| v["name"] == id
                && v["languages"]
                    .as_array()
                    .is_some_and(|l| l.contains(&json!("de")))),
            "missing German {id}"
        );
    }
    if args.get(1).is_some_and(|a| a == "describe") {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "reject") {
        send(
            reader.get_mut(),
            "synthesize",
            json!({"text":TEXT,"voice":{"name":"nonexistent"}}),
        )?;
        let (event, _) = receive(&mut reader)?;
        ensure!(event["type"] == "error", "expected error");
        send(reader.get_mut(), "describe", json!({}))?;
        ensure!(
            receive(&mut reader)?.0["type"] == "info",
            "server did not survive rejected request"
        );
        println!("invalid request rejected; same connection remains usable");
        return Ok(());
    }
    let text = std::env::var("BENCH_TEXT").unwrap_or_else(|_| TEXT.into());
    let streaming_input = std::env::var("BENCH_STREAMING_INPUT").is_ok_and(|v| v == "true");
    let voice = args.get(1).map(String::as_str).unwrap_or("F1");
    let path = args
        .get(2)
        .context("usage: probe VOICE output.wav | describe | reject")?;
    let running = Arc::new(AtomicBool::new(true));
    let peak_rss = Arc::new(AtomicU64::new(rss()));
    let peak_memory = Arc::new(AtomicU64::new(memory()));
    let (r, p, m) = (running.clone(), peak_rss.clone(), peak_memory.clone());
    let monitor = std::thread::spawn(move || {
        while r.load(Ordering::Relaxed) {
            p.fetch_max(rss(), Ordering::Relaxed);
            m.fetch_max(memory(), Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    let initial_cpu = cpu();
    let start = Instant::now();
    if streaming_input {
        send(
            reader.get_mut(),
            "synthesize-start",
            json!({"voice":{"name":voice}}),
        )?;
        // Exercise fragmented UTF-8 text input plus Home Assistant's compatibility
        // Synthesize message. The full text must not be synthesized twice.
        let chars: Vec<char> = text.chars().collect();
        for fragment in chars.chunks(11) {
            send(
                reader.get_mut(),
                "synthesize-chunk",
                json!({"text":fragment.iter().collect::<String>()}),
            )?;
        }
        send(
            reader.get_mut(),
            "synthesize",
            json!({"text":text,"voice":{"name":voice}}),
        )?;
        send(reader.get_mut(), "synthesize-stop", json!({}))?;
    } else {
        send(
            reader.get_mut(),
            "synthesize",
            json!({"text":text,"voice":{"name":voice}}),
        )?;
    }
    let mut audio = Vec::new();
    let mut started = false;
    let mut first_audio = None;
    let mut first_chunk = None;
    let mut ttfa = None;
    let mut first_nonzero = None;
    let mut chunks = Vec::new();
    let mut playback_end = 0.0f64;
    let mut underrun_seconds = 0.0f64;
    let mut underruns = 0usize;
    loop {
        let (event, pcm) = receive(&mut reader)?;
        match event["type"].as_str().unwrap_or("") {
            "audio-start" => {
                ensure!(!started, "duplicate start");
                started = true;
                ensure!(
                    event["data"]["rate"] == 44100
                        && event["data"]["width"] == 2
                        && event["data"]["channels"] == 1,
                    "invalid audio metadata"
                );
                first_audio = Some(start.elapsed().as_secs_f64());
            }
            "audio-chunk" => {
                ensure!(
                    started && !pcm.is_empty() && pcm.len() % 2 == 0,
                    "invalid PCM"
                );
                ensure!(
                    event["data"]["rate"] == 44100
                        && event["data"]["width"] == 2
                        && event["data"]["channels"] == 1,
                    "invalid chunk format"
                );
                let received = start.elapsed().as_secs_f64();
                first_chunk.get_or_insert(received);
                // A fully received, format-validated block containing >=20 ms of PCM is playable.
                if ttfa.is_none() && audio.len() + pcm.len() >= 1764 {
                    ttfa = Some(received);
                }
                if first_nonzero.is_none()
                    && pcm
                        .chunks_exact(2)
                        .any(|c| i16::from_le_bytes([c[0], c[1]]).abs() > 32)
                {
                    first_nonzero = Some(received);
                }
                let duration = pcm.len() as f64 / 88200.;
                if playback_end > 0.0 && received > playback_end + 0.002 {
                    underruns += 1;
                    underrun_seconds += received - playback_end;
                }
                playback_end = playback_end.max(received) + duration;
                chunks.push(
                    json!({"arrival_seconds":received,"bytes":pcm.len(),"audio_seconds":duration}),
                );
                audio.extend_from_slice(&pcm);
            }
            "audio-stop" => {
                ensure!(started && !audio.is_empty(), "missing audio");
                if !streaming_input {
                    break;
                }
            }
            "synthesize-stopped" if streaming_input => {
                ensure!(started && !audio.is_empty(), "missing audio");
                break;
            }
            _ => anyhow::bail!("unexpected response: {event}"),
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    let cpu_percent = (cpu() - initial_cpu) as f64 / 1e6 / elapsed * 100.;
    running.store(false, Ordering::Relaxed);
    monitor.join().unwrap();
    let samples: Vec<i16> = audio
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    ensure!(samples.iter().any(|v| *v != 0), "silent PCM");
    let duration = samples.len() as f64 / 44100.;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        wav.write_sample(sample)?;
    }
    wav.finalize()?;
    let saved = hound::WavReader::open(path)?;
    ensure!(
        saved.spec() == spec && saved.duration() as usize == audio.len() / 2,
        "WAV validation failed"
    );
    println!(
        "{}",
        json!({"voice":voice,"text":text,"ttfa_seconds":ttfa,"first_chunk_seconds":first_chunk,"first_nonzero_pcm_seconds":first_nonzero,"chunks":chunks,"playback_underruns_zero_buffer":underruns,"playback_underrun_seconds_zero_buffer":underrun_seconds,"streaming_input":streaming_input,"elapsed_seconds":elapsed,"synthesis_to_audio_start_seconds":first_audio,"audio_seconds":duration,"rtf":elapsed/duration,
        "peak_process_rss_bytes":peak_rss.load(Ordering::Relaxed),"peak_cgroup_memory_bytes":peak_memory.load(Ordering::Relaxed),"cpu_percent_one_core":cpu_percent,"wav":path,
        "steps":std::env::var("TTS_STEPS").unwrap_or("6".into()),"threads":std::env::var("TTS_THREADS").unwrap_or("4".into()),"speed":1.0,"language":"de","sample_rate":44100,"channels":1,"bits":16})
    );
    Ok(())
}
