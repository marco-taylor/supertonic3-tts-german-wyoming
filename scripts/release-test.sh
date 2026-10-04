#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
image="${1:?usage: release-test.sh IMAGE}"
export BENCH_CONTAINER=supertonic3-release-test BENCH_HTTP_PORT=8882 BENCH_WYOMING_PORT=10201 BENCH_IDLE_SECONDS=1
mkdir -p data/models data/voices benchmarks/tools benchmarks/test-voices benchmarks/release samples/perf/release
# Only test-owned newly created data directories; existing installation data is never removed.
if test -n "$(ls -A data/models)" || test -n "$(ls -A data/voices)";then echo 'Run from a fresh checkout: data/models and data/voices must initially be empty.' >&2;exit 1;fi
chown 10001:10001 data/models data/voices
if docker inspect "$BENCH_CONTAINER" >/dev/null 2>&1;then echo 'Isolated test name already exists; no container changed.' >&2;exit 1;fi
docker run -d --name "$BENCH_CONTAINER" --label local.project=supertonic3-tts-german-wyoming --stop-timeout=35 -p 10201:10200 -p 8882:8881 --mount "type=bind,src=$PWD/data/models,dst=/app/models" --mount "type=bind,src=$PWD/data/voices,dst=/app/voices" "$image" >/dev/null
for ((n=0;n<1200;n++));do
 if curl -fsS http://127.0.0.1:8882/health > benchmarks/release/download-health.json 2>/dev/null;then break;fi
 if test "$(docker inspect -f '{{.State.Running}}' "$BENCH_CONTAINER")" != true;then docker logs "$BENCH_CONTAINER";exit 1;fi
 sleep 1
done
test -s benchmarks/release/download-health.json
docker logs "$BENCH_CONTAINER" > benchmarks/release/download.log 2>&1
docker cp "$BENCH_CONTAINER:/usr/local/bin/probe" benchmarks/tools/probe
cp data/voices/*.json benchmarks/test-voices/
cp data/voices/F1.json benchmarks/test-voices/N100-Custom-Test.json
printf '{}\n' > benchmarks/test-voices/Invalid-Test.json
chmod -R a+rX benchmarks/test-voices benchmarks/tools
for mode in full phrase;do
 stream=false;test "$mode" != phrase || stream=true
 scripts/perf-case.sh "release/$mode" "$image" 6 4 --mount "type=bind,src=$PWD/benchmarks/test-voices,dst=/app/test-voices,readonly" -e VOICE_DIR=/app/test-voices -e TTS_STREAMING="$stream" -e TTS_GERMAN_NORMALIZATION=true -e TTS_STREAM_PREFILL_MS=3500
 scripts/perf-regression.sh "release-$mode"
 for part in header boundary json payload tiny;do
  arg=();test "$mode" != full || arg=(full)
  FRAGMENT_CASE="$part" node scripts/perf-fragmented-input.mjs "benchmarks/regression/release-$mode/fragment-$part.json" "samples/perf/regression/release-$mode/fragment-$part.wav" "${arg[@]}"
 done
 node scripts/perf-shutdown.mjs idle "benchmarks/regression/release-$mode/idle.json"
 node scripts/perf-shutdown.mjs active "benchmarks/regression/release-$mode/active.json"
 docker exec --user 0 -e 'BENCH_TEXT=Im Wohnzimmer sind es 21,5 °C. Es ist 18:30 Uhr. Der Preis beträgt 49,99 €.' "$BENCH_CONTAINER" /app/tools/probe F1 "/app/samples/perf/release/$mode/normalized.wav" > "benchmarks/release/normalized-$mode.json"
done
# Restart reuse test: missing-only preparation must not download present assets.
docker restart "$BENCH_CONTAINER" >/dev/null
for ((n=0;n<100;n++));do if curl -fsS http://127.0.0.1:8882/health > benchmarks/release/restart-health.json 2>/dev/null;then break;fi;sleep 0.2;done
docker exec "$BENCH_CONTAINER" supertonic3-tts-german-wyoming --healthcheck
printf 'Release regression passed.\n'
