#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
test_id="$1"
name="${BENCH_CONTAINER:-supertonic3-release-test}"
http_port="${BENCH_HTTP_PORT:-8882}"
result="benchmarks/regression/$test_id"
mkdir -p "$result" "samples/perf/regression/$test_id"
docker exec --user 0 "$name" /app/tools/probe describe > "$result/describe.json"
docker exec --user 0 "$name" /app/tools/probe reject > "$result/reject.txt"
curl -fsS http://127.0.0.1:$http_port/health > "$result/health.json"
curl -fsS http://127.0.0.1:$http_port/v1/audio/voices > "$result/voices.json"
for voice in F1 F2 F3 F4 F5 M1 M2 M3 M4 M5 N100-Custom-Test; do
    docker exec --user 0 "$name" /app/tools/probe "$voice" "/app/samples/perf/regression/$test_id/$voice.wav" >> "$result/voices.jsonl"
done
docker exec --user 0 -e BENCH_STREAMING_INPUT=true "$name" /app/tools/probe F1 "/app/samples/perf/regression/$test_id/ha-stream-input.wav" > "$result/ha-stream-input.json"
docker exec "$name" supertonic3-tts-german-wyoming --healthcheck
docker logs "$name" > "$result/server.log" 2>&1
node - "$result" <<'JS'
const fs=require('fs'),dir=process.argv[2],h=JSON.parse(fs.readFileSync(dir+'/health.json'));
if(!h.voices.includes('N100-Custom-Test')||h.voices.includes('Invalid-Test'))throw Error('voice validation failed');
if(!fs.readFileSync(dir+'/server.log','utf8').includes('ignoring invalid voice'))throw Error('missing invalid-voice warning');
const normal=JSON.parse(fs.readFileSync(dir+'/voices.jsonl','utf8').trim().split('\n')[0]);
const streamed=JSON.parse(fs.readFileSync(dir+'/ha-stream-input.json'));
if(Math.abs(normal.audio_seconds-streamed.audio_seconds)>0.02)throw Error('duplicated/incomplete HA streaming output');
JS
start="$(date +%s%N)"
docker stop "$name" >/dev/null
end="$(date +%s%N)"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker logs "$name" > "$result/server-sigterm.log" 2>&1
node -e "console.log(JSON.stringify({sigterm_seconds:($end-$start)/1e9,exit_code:0}))" > "$result/sigterm.json"
docker start "$name" >/dev/null
for ((n=0;n<300;n++)); do if curl -fsS http://127.0.0.1:$http_port/health >/dev/null 2>&1;then break;fi;sleep 0.05;done
docker exec "$name" supertonic3-tts-german-wyoming --healthcheck
printf 'Regression passed: %s\n' "$test_id"
