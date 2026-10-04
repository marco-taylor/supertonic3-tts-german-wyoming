#!/usr/bin/env bash
# One isolated, reproducible benchmark case. Extra args are Docker "-e KEY=VALUE" pairs.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
case_id="$1"; image="$2"; steps="$3"; threads="$4"; shift 4
case_dir="benchmarks/$case_id"
mkdir -p "$case_dir" "samples/perf/$case_id"
for thermal in /sys/class/thermal/thermal_zone*/temp; do printf '%s ' "$thermal";cat "$thermal";done > "$case_dir/thermal-before.txt"
cat /proc/loadavg > "$case_dir/load-before.txt"
name="${BENCH_CONTAINER:-supertonic3-release-test}"
http_port="${BENCH_HTTP_PORT:-8882}"
wyoming_port="${BENCH_WYOMING_PORT:-10201}"
root="$PWD"
asset_mode=",readonly"
if test "${BENCH_ASSET_READONLY:-true}" = false;then asset_mode="";fi
if docker inspect "$name" >/dev/null 2>&1; then
    test "$(docker inspect -f '{{index .Config.Labels "local.project"}}' "$name")" = supertonic3-tts-german-wyoming
    docker stop "$name" >/dev/null
    test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
    docker rm "$name" >/dev/null
fi
start_ns="$(date +%s%N)"
docker run -d --name "$name" --label local.project=supertonic3-tts-german-wyoming \
    --restart=no --stop-timeout=35 -p "$wyoming_port:10200" -p "$http_port:8881" \
    -e TTS_LANGUAGE=de -e TTS_VOICE=F1 -e TTS_SPEED=1.0 -e TTS_STEPS="$steps" \
    -e TTS_THREADS="$threads" -e TTS_CONCURRENT_REQUESTS=1 \
    --mount "type=bind,src=$root/data/models,dst=/app/models$asset_mode" \
    --mount "type=bind,src=$root/data/voices,dst=/app/voices$asset_mode" \
    --mount "type=bind,src=$root/samples,dst=/app/samples" \
    --mount "type=bind,src=$root/benchmarks/tools,dst=/app/tools,readonly" "$@" "$image" >/dev/null
ready=false
for ((attempt=0;attempt<600;attempt++)); do
    if curl -fsS --max-time 1 http://127.0.0.1:$http_port/health > "$case_dir/health.json" 2>/dev/null; then ready=true;break;fi
    if test "$(docker inspect -f '{{.State.Running}}' "$name")" != true;then docker logs "$name" >&2;exit 1;fi
    sleep 0.05
done
test "$ready" = true
end_ns="$(date +%s%N)"
printf '{"startup_to_http_seconds":%s,"poll_resolution_seconds":0.05}\n' "$(node -e "console.log(($end_ns-$start_ns)/1e9)")" > "$case_dir/startup.json"
docker exec --user 0 "$name" /app/tools/probe F1 "/app/samples/perf/$case_id/cold.wav" > "$case_dir/cold.json"
sleep "${BENCH_IDLE_SECONDS:-0}"
docker exec --user 0 "$name" /app/tools/probe F1 "/app/samples/perf/$case_id/warmup.wav" > "$case_dir/warmup.json"
: > "$case_dir/warm.jsonl"
for ((run=1;run<=${BENCH_RUNS:-3};run++)); do
    sleep "${BENCH_IDLE_SECONDS:-0}"
    docker exec --user 0 "$name" /app/tools/probe F1 "/app/samples/perf/$case_id/warm-$run.wav" >> "$case_dir/warm.jsonl"
done
docker exec --user 0 "$name" /app/tools/probe describe > "$case_dir/describe.json"
curl -fsS http://127.0.0.1:$http_port/v1/audio/voices > "$case_dir/voices.json"
docker exec "$name" supertonic3-tts-german-wyoming --healthcheck
docker logs "$name" > "$case_dir/server.log" 2>&1
docker inspect "$name" > "$case_dir/container.json"
docker image inspect "$image" > "$case_dir/image.json"
for thermal in /sys/class/thermal/thermal_zone*/temp; do printf '%s ' "$thermal";cat "$thermal";done > "$case_dir/thermal-after.txt"
printf 'Completed %s\n' "$case_id"
