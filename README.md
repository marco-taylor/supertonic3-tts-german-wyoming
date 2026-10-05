# Supertonic 3 TTS for Home Assistant

<img src="icons/supertonic3-tts-german-wyoming.png" alt="Supertonic 3 TTS German Wyoming" width="160">

Fast, local **German text-to-speech for Home Assistant**, written in Rust and powered by **Supertonic 3**, **ONNX Runtime** and a **native Wyoming server**.

Designed for efficient CPU-only operation and tested on an **Intel N100**. After the initial model and voice download, synthesis runs locally and can operate offline. No Python sidecar, GPU provider, quantization or audio resampling is required.

## Highlights

- 🇩🇪 **German by default**
- 🏠 **Native Wyoming integration** for Home Assistant
- ⚡ **Optimized and tested on Intel N100**
- 🔊 **10 standard voices:** F1–F5 and M1–M5
- ➕ **Custom Supertonic 3 voice JSONs** detected automatically
- 🚀 Optional **low-latency phrase streaming**
- 🔢 German normalization for numbers, temperatures, times, dates, currencies and common units
- 🦀 Native **Rust** service — no Python runtime in the final image
- 💾 Persistent model and voice directories
- ❤️ HTTP health endpoint and Docker healthcheck
- 🔒 Local synthesis after the initial asset download

Audio output is **44.1 kHz, mono, signed 16-bit little-endian PCM**.

## Quick start

```sh
docker volume create supertonic-models
docker volume create supertonic-voices

docker run -d --name supertonic3-tts \
  --restart unless-stopped --stop-timeout 35 \
  -p 10200:10200 \
  -p 8881:8881 \
  -v supertonic-models:/app/models \
  -v supertonic-voices:/app/voices \
  -e TTS_LANGUAGE=de \
  -e TTS_VOICE=F1 \
  -e TTS_SPEED=1.0 \
  -e TTS_STEPS=6 \
  -e TTS_THREADS=4 \
  -e TTS_CONCURRENT_REQUESTS=1 \
  -e TTS_GERMAN_NORMALIZATION=true \
  -e TTS_STREAMING=true \
  -e TTS_STREAM_PREFILL_MS=3500 \
  ghcr.io/marco-taylor/supertonic3-tts-german-wyoming:latest
```

The Docker image defaults to UID/GID **10001:10001**. Named volumes are initialized with writable image directories. For bind mounts, grant that UID access or run the container with a UID/GID matching the directory owner. The included Unraid template overrides the image default with `--user=99:100` for Unraid `nobody:users`. Both persistent folders must be readable and writable by UID/GID 99:100.

First startup requires internet access to download approximately **380 MiB** of official model and voice assets. Existing files are reused and only missing files are downloaded. Downloads are written to temporary files before being moved into place on success.

Models and voices are **not bundled in the image or Git repository**. Official assets are pinned to Supertonic 3 revision `3cadd1ee6394adea1bd021217a0e650ede09a323`.

Supported release architecture: **linux/amd64**.

## Home Assistant

In Home Assistant open:

**Settings → Devices & services → Add integration → Wyoming Protocol**

Enter the hostname or IP address of the Docker host and Wyoming port **10200**. Then select Supertonic as the TTS service in the voice assistant pipeline.

HTTP port **8881** is used for health and voice discovery and is not required by the Wyoming integration itself.

The Wyoming implementation supports:

- Describe
- Synthesize
- AudioStart
- AudioChunk
- AudioStop

The service also supports streamed text input, including Home Assistant's compatibility Synthesize event.

> Home Assistant's buffered TTS playback path may wait for a complete WAV before playback. A streaming-capable playback path is required to benefit from earlier PCM delivery. Client-side TTFA therefore does not necessarily equal actual speaker start time.

## Recommended Intel N100 settings

| Setting | Recommended value |
|---|---:|
| Language | `de` |
| Voice | `F1` |
| Speed | `1.0` |
| Steps | `6` |
| Threads | `4` |
| Concurrent requests | `1` |
| German normalization | `true` |
| Streaming | `true` |
| Streaming prefill | `3500 ms` |

`TTS_STREAMING=false` restores full-context synthesis.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `TTS_LANGUAGE` | `de` | Supertonic language; German regional codes map to `de` |
| `TTS_VOICE` | `F1` | Default installed voice |
| `TTS_SPEED` | `1.0` | Speech speed factor, supported range 0.25–4.0 |
| `TTS_STEPS` | `6` | Inference steps; upstream supports 5–12 |
| `TTS_THREADS` | `4` | ONNX intra-op threads, 1–64 |
| `TTS_CONCURRENT_REQUESTS` | `1` | Independent requests/engines, 1–16; higher values use more RAM |
| `TTS_GERMAN_NORMALIZATION` | `true` | German number/unit preprocessing |
| `TTS_STREAMING` | `true` | Phrase/sentence streaming pipeline |
| `TTS_STREAM_PREFILL_MS` | unset → `3500` | Audio prefill target, 0–5000 ms |
| `TTS_STREAMING_PREFILL_MS` | `3500` | Backward-compatible prefill variable |
| `WYOMING_PORT` | `10200` | Wyoming TCP port |
| `HTTP_PORT` | `8881` | HTTP health/voice endpoint |
| `MODEL_DIR` | `/app/models` | Persistent model directory |
| `VOICE_DIR` | `/app/voices` | Persistent standard/custom voice directory |
| `ORT_CONFIGURED` | `false` | Enables the optional tested ONNX tuning wrapper |
| `ORT_THREAD_POOL` | `global` | `global` or `session` |
| `ORT_INTER_THREADS` | `1` | ONNX inter-op threads |
| `ORT_SPINNING` | `false` | ONNX thread spinning |
| `ORT_EXECUTION_MODE` | `sequential` | `sequential` or `parallel` |
| `TTS_OPTIMIZED_DATAPATH` | `true` | Bounded PCM conversion/output pipeline |
| `WYOMING_BUFFERED_TCP` | `true` | Buffered TCP writer and TCP_NODELAY |
| `WYOMING_CHUNK_BYTES` | `16384` | PCM chunk size in bytes |
| `RUST_LOG` | `info` | Rust logging filter |

Environment variables are read at process startup.

## Streaming

Supertonic 3 / `supertonic3-tts` 1.3.0 does not expose a native incremental diffusion/vocoder PCM API. This service therefore synthesizes complete sentence or phrase blocks, emits ready PCM immediately, and prepares later blocks while earlier audio can play.

The default **3500 ms prefill** was the conservative setting in N100 testing. It avoided simulated playback underruns in the tested phrases at 5, 6 and 8 inference steps. This does not guarantee gap-free playback for every text, system load or network condition.

No fades, silence trimming or other DSP is applied.

## German text normalization

When `TTS_GERMAN_NORMALIZATION=true`, normalization runs before segmentation and expands common spoken values such as:

- integers and decimal numbers
- negative values
- percentages
- °C / °F
- clock times
- valid German dates
- Euro / Cent
- W, kW, Wh, kWh, V, A
- km, m, cm, mm

Examples:

- `21,5 °C` → “einundzwanzig Komma fünf Grad Celsius”
- `18:30 Uhr` → “achtzehn Uhr dreißig”
- `49,99 €` → “neunundvierzig Euro und neunundneunzig Cent”

IPs, URLs, mixed letter/digit IDs and filenames are intentionally kept opaque where possible. This is a practical German TTS normalizer, not a general-purpose language parser; ambiguous numeric IDs, scientific notation and arbitrary grammatical inflections remain known limitations.

## Voices

### Standard voices

The ten official Supertonic 3 voice styles are supported and downloaded automatically when missing:

**Female:** F1, F2, F3, F4, F5  
**Male:** M1, M2, M3, M4, M5

All ten were tested with German synthesis during release validation.

### Custom voices

Compatible Supertonic 3 voice JSON files can be copied into `/app/voices`. Restart the container and valid files are discovered automatically — no source-code change or image rebuild is required.

The filename stem becomes the voice name exposed through Wyoming Describe and `/v1/audio/voices`.

Validation requires float32 `style_ttl` shape `[1,50,256]` and `style_dp` shape `[1,8,16]`, valid nested data and finite values. Invalid or oversized files are logged and ignored rather than crashing the service.

Voice creation or cloning is **not** provided by this project. Always respect the voice owner's permissions and applicable model terms.

## Health and shutdown

### `GET /health`

Reports engine/version, model state, language, default voice, available voices, steps, threads, speed, normalization and streaming settings.

### `GET /v1/audio/voices`

Returns all discovered voices.

Docker's healthcheck runs the Rust server's built-in healthcheck mode. A generous startup grace period allows the first model download to complete.

SIGTERM stops new requests and drains active synthesis where possible. A **35-second Docker stop timeout** is recommended.

The service is intended for a trusted local network. HTTP and Wyoming endpoints do not implement authentication or TLS.

## Intel N100 measurements

These are measurements from the development/test system and are **not performance guarantees or a voice-quality ranking**.

Test configuration: Intel N100, four threads, six steps, F1, speed 1.0, German normalization enabled, phrase streaming enabled, 3500 ms prefill; median of three warm runs after warm-up using an eight-sentence German sensor report.

| Measurement | Result |
|---|---:|
| TTFA | 2.261 s |
| Total synthesis/transfer time | 19.817 s |
| Generated audio duration | 32.229 s |
| Real-time factor | **0.615** |
| Peak process RSS | ~483 MiB |
| CPU | ~204%* |

\* 100% represents one fully utilized CPU core.

The German normalization pass itself took approximately **6 microseconds** for that test input. Shorter texts, other voices and different settings can produce different results; no general sub-second TTFA claim is made.

## Unraid

An Unraid template is included at:

`unraid/supertonic3-tts-german-wyoming.xml`

Default container ports:

- Wyoming: `10200`
- HTTP: `8881`

Persistent paths:

- `/app/models`
- `/app/voices`

The important TTS, streaming and German-normalization settings are exposed as environment variables so they can be changed without rebuilding the image.

## Building from source

```sh
docker build --platform linux/amd64 -t supertonic3-tts-german-wyoming:local .
```

For Rust tests:

```sh
cargo test --release --locked
```

`scripts/release-test.sh IMAGE` performs an isolated container regression covering persistent downloads, all standard voices, custom/invalid voice fixtures, streaming and full synthesis, Wyoming framing and fragmentation, HTTP, healthcheck and SIGTERM.

## Acknowledgements & Credits

This project would not be possible without the work of the open-source community. Special thanks to the maintainers and contributors of:

- **[Supertone – Supertonic 3](https://huggingface.co/Supertone/supertonic-3)** — the TTS model and official voice styles that form the foundation of the speech synthesis used here.
- **[David Valin – supertonic3-tts](https://github.com/DavidValin/supertonic3-tts)** — the Rust Supertonic 3 implementation/runtime that makes native Rust inference possible in this project.
- **[Microsoft – ONNX Runtime](https://github.com/microsoft/onnxruntime)** — the inference runtime used for CPU execution.
- **[pykeio – ORT](https://github.com/pykeio/ort)** — Rust bindings for ONNX Runtime.
- **[Rhasspy – Wyoming Protocol](https://github.com/rhasspy/wyoming)** — the protocol used for native voice-service communication with Home Assistant.
- **[Home Assistant](https://www.home-assistant.io/)** — the open-source home-automation and voice ecosystem this integration was primarily designed for.

Thank you to everyone contributing to these projects and to the wider open-source voice community.

`supertonic3-tts-german-wyoming` is an independent community project and is **not affiliated with or endorsed by Supertone or Home Assistant**.

## Licenses

Project code: **MIT** — see [LICENSE](LICENSE).

The software dependencies retain their own licenses, including:

- `supertonic3-tts`: MIT
- ORT: MIT / Apache-2.0
- ONNX Runtime: MIT

Dependency notices are included in the runtime image under `/usr/share/doc/supertonic3-tts-german-wyoming/licenses`.

The official **Supertonic 3 model and voice styles** use the **BigScience Open RAIL-M license**, including its use restrictions. They are **not relicensed under this project's MIT license**. The original model license is downloaded into the persistent model directory alongside the model assets.

See the [pinned Supertonic 3 model license](https://huggingface.co/Supertone/supertonic-3/blob/3cadd1ee6394adea1bd021217a0e650ede09a323/LICENSE) for the applicable terms.

---

Built for local German voice output with Home Assistant, with a focus on efficient CPU inference, transparent configuration and reproducible behavior on low-power hardware.