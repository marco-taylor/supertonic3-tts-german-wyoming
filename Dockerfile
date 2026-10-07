# Pinned multi-platform base manifests; supported release target is linux/amd64.
FROM rust:1.93.1-trixie@sha256:ecbe59a8408895edd02d9ef422504b8501dd9fa1526de27a45b73406d734d659 AS builder
ARG DEBIAN_SNAPSHOT=20260918T000000Z
RUN sed -i "s|http://deb.debian.org/debian-security|https://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT}|; s|http://deb.debian.org/debian|https://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT}|" /etc/apt/sources.list.d/debian.sources && \
    apt-get -o Acquire::Check-Valid-Until=false update && \
    apt-get -o Acquire::Check-Valid-Until=false install -y --no-install-recommends libasound2-dev pkg-config ca-certificates jq && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock LICENSE ./
COPY src ./src
RUN cargo test --release --locked && cargo build --release --locked --bin supertonic3-tts-german-wyoming --bin probe && \
    mkdir -p /artifacts/licenses && \
    cp target/release/supertonic3-tts-german-wyoming target/release/probe /artifacts/ && strip /artifacts/supertonic3-tts-german-wyoming /artifacts/probe && \
    rustc --version > /artifacts/rust-version.txt && cp LICENSE /artifacts/licenses/PROJECT-MIT && \
    cargo metadata --locked --format-version 1 > /tmp/dependencies.json && \
    jq -r '.packages[] | [.name,.version,(.license // "UNSPECIFIED")] | @tsv' /tmp/dependencies.json > /artifacts/licenses/DEPENDENCIES.tsv && \
    jq -r '.packages[] | .manifest_path' /tmp/dependencies.json | while read -r manifest; do \
      directory="$(dirname "$manifest")"; name="$(basename "$directory")"; \
      mkdir -p "/artifacts/licenses/$name"; \
      find "$directory" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'LICENCE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' \) -exec cp '{}' "/artifacts/licenses/$name/" \; ; \
    done

FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a
ARG DEBIAN_SNAPSHOT=20260918T000000Z
# HTTP bootstrap uses Debian's signed archive metadata; no unverified packages.
RUN sed -i "s|http://deb.debian.org/debian-security|http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT}|; s|http://deb.debian.org/debian|http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT}|" /etc/apt/sources.list.d/debian.sources && \
    apt-get -o Acquire::Check-Valid-Until=false update && \
    apt-get -o Acquire::Check-Valid-Until=false install -y --no-install-recommends ca-certificates libstdc++6 && \
    rm -rf /var/lib/apt/lists/* && mkdir -p /app/models /app/voices && chown -R 10001:10001 /app
WORKDIR /app
COPY --from=builder /artifacts/supertonic3-tts-german-wyoming /artifacts/probe /usr/local/bin/
COPY --from=builder /build/Cargo.lock /app/Cargo.lock
COPY --from=builder /artifacts/rust-version.txt /app/rust-version.txt
COPY --from=builder /artifacts/licenses /usr/share/doc/supertonic3-tts-german-wyoming/licenses
COPY licenses/ONNXRuntime-MIT /usr/share/doc/supertonic3-tts-german-wyoming/licenses/ONNXRuntime-MIT
ARG VCS_REF=local
LABEL org.opencontainers.image.title="supertonic3-tts-german-wyoming" \
      org.opencontainers.image.version="1.0.0" \
      org.opencontainers.image.source="https://github.com/marco-taylor/supertonic3-tts-german-wyoming" \
      org.opencontainers.image.licenses="MIT" org.opencontainers.image.revision="$VCS_REF"
ENV LD_LIBRARY_PATH=/usr/local/lib TTS_LANGUAGE=de TTS_VOICE=F1 TTS_SPEED=1.0 TTS_STEPS=5 TTS_THREADS=4 TTS_CONCURRENT_REQUESTS=1 WYOMING_PORT=10200 HTTP_PORT=8881 ORT_CONFIGURED=false ORT_THREAD_POOL=global ORT_INTER_THREADS=1 ORT_SPINNING=false ORT_EXECUTION_MODE=sequential TTS_OPTIMIZED_DATAPATH=true WYOMING_BUFFERED_TCP=true WYOMING_CHUNK_BYTES=16384 TTS_STREAMING=true TTS_STREAMING_PREFILL_MS=3500 TTS_GERMAN_NORMALIZATION=true
EXPOSE 10200 8881
VOLUME ["/app/models","/app/voices"]
USER 10001:10001
HEALTHCHECK --interval=15s --timeout=5s --start-period=15m --retries=3 CMD ["supertonic3-tts-german-wyoming","--healthcheck"]
STOPSIGNAL SIGTERM
ENTRYPOINT ["supertonic3-tts-german-wyoming"]
