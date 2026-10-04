# supertonic3-tts-german-wyoming

![Project and Unraid CA icon](icons/supertonic3-tts-german-wyoming.png)

Local German text-to-speech for Home Assistant, written in Rust, using Supertonic 3 and ONNX Runtime with a native Wyoming server. CPU only; tested on an Intel N100 (four cores). No Python sidecar, GPU provider, quantization or resampling.

Audio is **44100 Hz, mono, signed 16-bit little-endian PCM**. Ten standard voices: **F1–F5 and M1–M5**. Compatible custom voice JSON files are discovered at startup without rebuilding the image.

## Docker

The intended image is `ghcr.io/marco-taylor/supertonic3-tts-german-wyoming:v1.0.0` (also `:latest`). It is prepared locally; availability requires a separate publication step.

```sh
docker volume create supertonic-models
docker volume create supertonic-voices
docker run -d --name supertonic3-tts \
  --restart unless-stopped --stop-timeout 35 \
  -p 10200:10200 -p 8881:8881 \
  -v supertonic-models:/app/models \
  -v supertonic-voices:/app/voices \
  -e TTS_LANGUAGE=de -e TTS_VOICE=F1 -e TTS_SPEED=1.0 \
  -e TTS_STEPS=6 -e TTS_THREADS=4 -e TTS_CONCURRENT_REQUESTS=1 \
  -e TTS_GERMAN_NORMALIZATION=true -e TTS_STREAMING=true \
  -e TTS_STREAM_PREFILL_MS=3500 \
  ghcr.io/marco-taylor/supertonic3-tts-german-wyoming:v1.0.0
```

The image runs as UID/GID **10001:10001**. Named volumes are initialized with writable image directories. For bind mounts, grant that UID access, or supply `--user=UID:GID` matching the directory owner. The prepared Unraid template uses `--user=99:100` for the usual nobody/users ownership. Do not mount the same model/voice directory into simultaneous first-start downloaders.

First startup needs internet access to download approximately 380 MiB of official models and the ten voice JSON files. Assets are pinned to revision `3cadd1ee6394adea1bd021217a0e650ede09a323`. Existing files are reused; only missing files download, first to temporary files, then renamed on success. Models/voices are not bundled in the image or repository. Once complete, synthesis is local and can run offline. Corrupt existing model files are not silently replaced: restore those files manually after examining logs.

For a local build:

```sh
docker build --platform linux/amd64 -t supertonic3-tts-german-wyoming:local .
```

Supported release architecture: **linux/amd64**. No CPU-specific compiler flags; Intel N100 is a tested target rather than a required CPU model.

## Home Assistant

Add **Settings → Devices & services → Add integration → Wyoming Protocol**. Enter your Docker host's reachable hostname and port **10200**. Select Supertonic as TTS in the voice assistant pipeline. HTTP port **8881** is for health/voice discovery, not the Wyoming integration. No Home Assistant configuration is changed automatically.

Wyoming supports Describe, Synthesize, AudioStart/AudioChunk/AudioStop and streamed text input including Home Assistant's compatibility Synthesize event. The default HA buffered TTS path may wait for the entire WAV; a streaming-capable playback path is needed to benefit from earlier PCM delivery. Client TTFA is not a measurement of actual speaker start.

## Configuration

Values below are **Docker image defaults**, with the recommended N100 configuration enabled. Standalone binary fallback defaults for normalization/phrase streaming remain false. Environment is read at process startup.

| Variable | Default | Meaning |
|---|---|---|
| TTS_LANGUAGE | de | Supertonic language; German regional codes map to de |
| TTS_VOICE | F1 | Installed voice name |
| TTS_SPEED | 1.0 | Speed factor; supported 0.25–4.0 |
| TTS_STEPS | 6 | Inference steps, upstream supports 5–12; 5/6/8 tested |
| TTS_THREADS | 4 | ONNX intra-op threads; 1–64 |
| TTS_CONCURRENT_REQUESTS | 1 | Independent requests/engines, 1–16; increasing uses more RAM |
| TTS_GERMAN_NORMALIZATION | true | German sensor-value preprocessing; false passes original text |
| TTS_STREAMING | true | Optional German phrase/sentence pipeline; false restores full-context synthesis |
| TTS_STREAM_PREFILL_MS | unset → 3500 | Audio prefill target, 0–5000 ms; overrides legacy name |
| TTS_STREAMING_PREFILL_MS | 3500 | Backward-compatible prefill name |
| WYOMING_PORT | 10200 | Container Wyoming TCP port; match Docker port mapping |
| HTTP_PORT | 8881 | Container HTTP TCP port; match Docker port mapping |
| MODEL_DIR | /app/models | Persistent models, including onnx/ and model LICENSE |
| VOICE_DIR | /app/voices | Persistent standard/custom JSON voices |
| ORT_CONFIGURED | false | Preserve upstream engine; true enables previously tested tuning wrapper |
| ORT_THREAD_POOL | global | global or session; legacy wrapper requires global |
| ORT_INTER_THREADS | 1 | Inter-op threads, 1–16 |
| ORT_SPINNING | false | ONNX thread spinning |
| ORT_EXECUTION_MODE | sequential | sequential or parallel; parallel requires ORT_CONFIGURED=true |
| TTS_OPTIMIZED_DATAPATH | true | Bounded PCM conversion/output pipeline |
| WYOMING_BUFFERED_TCP | true | Buffered TCP writer and TCP_NODELAY |
| WYOMING_CHUNK_BYTES | 16384 | Even PCM chunk size, 512–262144 bytes |
| RUST_LOG | info | Logging filter |

`TTS_STREAMING=false` is the full-context fallback. Supertonic's own long-text splitting still applies. Keeping both streaming and normalization false reproduces the original text preparation. Inference steps, voice files and the model remain unchanged by these modes.

## Streaming and German normalization

Supertonic 3 / supertonic3-tts 1.3.0 has no native incremental diffusion/vocoder PCM API. This service synthesizes complete sentence/phrase blocks, emits their ready PCM immediately, and prepares later blocks while earlier audio can play. Requests remain serialized by default; text/audio queues and prefill are bounded. Prefill waits for the target audio duration, at most two completed blocks, or completion; it is not an added fixed sleep.

Conservative German segmentation protects abbreviations, decimals, times and dates. Normalization happens **before segmentation**, also across streamed input fragments. It expands integers, decimal commas, negative values, percentages, °C/°F, clock times, valid German dates, Euro/Cent and W/kW/Wh/kWh/V/A/km/m/cm/mm.

Examples: `21,5 °C` → “einundzwanzig Komma fünf Grad Celsius”; `18:30 Uhr` → “achtzehn Uhr dreißig”; `49,99 €` → “neunundvierzig Euro und neunundneunzig Cent”. IPs, URLs, mixed letter/digit IDs and filenames remain opaque; explicit Port/ID/Version/Modell context protects following digit tokens. Unlabelled numeric IDs/date-shaped versions can be ambiguous. Dot-grouped numbers, ISO dates, scientific notation, compound units and arbitrary grammatical inflections are not a general language parser.

Phrase synthesis changes context and may change prosody, audio duration and natural pauses. Prefill 3500 ms avoided simulated playback underruns in the tested phrases at 5/6/8 steps, but does not guarantee gap-free audio for every text/load/network. No fades, silence trimming or other DSP is applied. Listening and a real Home Assistant playback test remain necessary.

## Custom voices

Copy a compatible Supertonic 3 JSON into the voice mount, then restart. Filename stem becomes the voice name returned by Describe and `/v1/audio/voices`. Validation requires float32 `style_ttl` shape `[1,50,256]` and `style_dp` shape `[1,8,16]`, valid nested data and finite values. Invalid files are logged and ignored; one broken custom voice does not crash startup. Files larger than 8 MiB and nonregular files are ignored. Voice creation/cloning is not provided by this server. Respect the voice owner's permission and applicable model terms.

## Health and shutdown

`GET /health` reports engine/version, model state, language, default voice, voices, steps/threads/speed, normalization and streaming/prefill settings. `GET /v1/audio/voices` lists discovered voices. Neither endpoint synthesizes audio.

Docker's built-in healthcheck invokes the Rust server's healthcheck mode every 15 seconds, with a 15-minute startup grace for downloads. SIGTERM stops accepting requests and drains active syntheses where possible; use a 35-second Docker stop timeout. An incomplete streamed request can be rejected on shutdown. A local trusted network is expected: no HTTP/Wyoming authentication or TLS is implemented.

## Measured Intel N100 results

These are local measurements, not guarantees or a voice-quality ranking. Four threads, six steps, F1, speed 1.0, phrase streaming and 3500 ms prefill; median of three warm runs after warm-up. For an eight-sentence German sensor report: TTFA 2.261 s, total 19.817 s, audio 32.229 s, RTF 0.615, peak process RSS about 483 MiB. CPU measurement about 204%, where 100% is one core. The normalization pass alone took about 6 microseconds for that input. Shorter texts and different settings produce different results; no general subsecond TTFA claim.

## Tests and release preparation

`cargo test --release --locked` runs unit tests, including normalization, protected expressions, arbitrary text-fragment boundaries, PCM conversion and Wyoming framing. `scripts/release-test.sh IMAGE` performs isolated container regression on ports 10201/8882 with fresh persistent downloads, all standard voices, custom/invalid fixtures, both synthesis modes, fragmentation, HTTP and SIGTERM. It creates local ignored data/benchmarks/samples and requires Docker, curl, Node.js and Bash. It never uses the normal service container name or public default ports.

A local Unraid template is under `unraid/supertonic3-tts-german-wyoming.xml`. The workflow is prepared for main and version-tag pushes using GitHub's GITHUB_TOKEN; preparing files does not publish anything. Build/runtime manifests, Debian package snapshot and dependencies are pinned. Package indexes, crate downloads and model downloads still require their upstream services to be reachable; byte-for-byte image reproducibility is not claimed.

## License and credits

Project code: **MIT**, see LICENSE. Rust SDK [supertonic3-tts](https://github.com/DavidValin/supertonic3-tts): MIT. [ORT](https://github.com/pykeio/ort): MIT/Apache-2.0. [ONNX Runtime](https://github.com/microsoft/onnxruntime): MIT. Dependency notices are included in the runtime image under `/usr/share/doc/supertonic3-tts-german-wyoming/licenses`.

The official [Supertonic 3 model and voice styles](https://huggingface.co/Supertone/supertonic-3) use the **BigScience Open RAIL-M license**, including use restrictions; they are not relicensed under this project's MIT license. The original license downloads to the model mount. [Pinned model license](https://huggingface.co/Supertone/supertonic-3/blob/3cadd1ee6394adea1bd021217a0e650ede09a323/LICENSE).

Credits: Supertone/Supertonic 3, David Valin's Rust SDK, pykeio ORT, Microsoft ONNX Runtime, the [Wyoming protocol](https://github.com/rhasspy/wyoming), and Home Assistant. This is an independent integration, not an official Supertone or Home Assistant product.
