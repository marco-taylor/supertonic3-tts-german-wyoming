#!/usr/bin/env python3
"""Dependency-free Wyoming parameter/profile regression client for an isolated server.

Run --profiles to obtain the TTS_VOICE_PROFILES fixture. Server defaults must be
TTS_SPEED=1.0, TTS_STEPS=5, F1/de, with German normalization enabled. Run once with
TTS_STREAMING=true and once with false; --input-stream tests SynthesizeStart.
"""
import argparse
import json
import pathlib
import socket
import time
import wave

TEXT = (
    "Das ist ein Geschwindigkeitstest für die lokale Sprachausgabe von Home Assistant. "
    "Heute beträgt die Temperatur einundzwanzig Grad Celsius. "
    "Das Licht im Wohnzimmer wird eingeschaltet."
)
PROFILES = {f"test-speed-{s:.1f}": {"voice": "F1", "speed": s} for s in [0.8, 0.9, 1.0, 1.1, 1.2]}
PROFILES.update({f"test-steps-{s}": {"voice": "F1", "steps": s} for s in [5, 6, 12]})
PROFILES["F1-ruhig"] = {"voice": "F1", "speed": 0.9, "steps": 5}


def send(sock, kind, data):
    body = json.dumps(data, ensure_ascii=False).encode()
    header = json.dumps({"type": kind, "data_length": len(body)}).encode()
    sock.sendall(header + b"\n" + body)


def read(stream):
    line = stream.readline()
    if not line:
        raise AssertionError("server disconnected before terminal event")
    header = json.loads(line)
    raw = stream.read(header.get("data_length", 0))
    data = json.loads(raw) if raw else header.get("data", {})
    audio = stream.read(header.get("payload_length", 0))
    return header["type"], data, audio


def request(host, port, voice, text=TEXT, input_stream=False, extra=None):
    events, chunks, pcm = [], [], bytearray()
    start = time.perf_counter()
    with socket.create_connection((host, port), timeout=120) as sock, sock.makefile("rb") as stream:
        data = {"voice": {"name": voice}}
        data.update(extra or {})
        if input_stream:
            send(sock, "synthesize-start", data)
            # Deliberately fragment German words and normalization tokens.
            for i in range(0, len(text), 11):
                send(sock, "synthesize-chunk", {"text": text[i:i + 11]})
            send(sock, "synthesize", {**data, "text": text})
            send(sock, "synthesize-stop", {})
        else:
            send(sock, "synthesize", {**data, "text": text})
        while True:
            kind, fields, audio = read(stream)
            events.append(kind)
            if kind == "error":
                return {"error": fields, "events": events}
            if kind == "audio-start":
                assert fields == {"rate": 44100, "width": 2, "channels": 1}, fields
            elif kind == "audio-chunk":
                assert fields == {"rate": 44100, "width": 2, "channels": 1}, fields
                assert audio and len(audio) % 2 == 0
                chunks.append({"time": time.perf_counter() - start, "bytes": len(audio)})
                pcm.extend(audio)
            if kind == ("synthesize-stopped" if input_stream else "audio-stop"):
                break
    elapsed = time.perf_counter() - start
    assert events[0] == "audio-start" and events.count("audio-start") == 1
    assert events.count("audio-stop") == 1 and chunks
    assert all(e == "audio-chunk" for e in events[1:events.index("audio-stop")])
    if input_stream:
        assert events[-2:] == ["audio-stop", "synthesize-stopped"]
    duration = len(pcm) / (44100 * 2)
    return {"voice": voice, "text": text, "ttfa": chunks[0]["time"], "total": elapsed,
            "audio": duration, "rtf": elapsed / duration, "chunks": chunks,
            "events": events, "pcm": pcm}


def save(result, directory, label, run):
    assert "error" not in result, result
    pcm = result.pop("pcm")
    result.update(label=label, run=run)
    with (directory / "runs.jsonl").open("a") as out:
        out.write(json.dumps(result, ensure_ascii=False) + "\n")
    if run == 1:
        with wave.open(str(directory / f"{label}_{result['voice']}.wav"), "wb") as out:
            out.setparams((1, 2, 44100, 0, "NONE", "not compressed"))
            out.writeframes(pcm)
    print(json.dumps({k: v for k, v in result.items() if k not in ["events", "chunks", "text"]}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profiles", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=19201)
    parser.add_argument("--out", type=pathlib.Path)
    parser.add_argument("--label", default="stream")
    parser.add_argument("--input-stream", action="store_true")
    parser.add_argument("--baseline", action="store_true")
    parser.add_argument("--quick", action="store_true")
    args = parser.parse_args()
    if args.profiles:
        print(json.dumps(PROFILES))
        return
    assert args.out, "--out required"
    args.out.mkdir(parents=True, exist_ok=True)
    voices = ["F1"] if args.baseline else ["F1", *PROFILES]
    if args.quick and not args.baseline:
        voices = ["F1", "test-speed-0.8", "test-speed-1.2", "test-steps-6"]
    lengths = {}
    for voice in voices:
        count = 1 if args.quick or voice == "test-steps-12" else 3
        for run in range(count + 1):
            result = request(args.host, args.port, voice, input_stream=args.input_stream)
            if run:
                lengths[voice] = result["audio"]
                save(result, args.out, args.label, run)
    if not args.baseline and not args.quick:
        ordered = [lengths[f"test-speed-{s:.1f}"] for s in [0.8, 0.9, 1.0, 1.1, 1.2]]
        assert all(a > b for a, b in zip(ordered, ordered[1:])), ordered
    if not args.baseline:
        failures = []
        for extra in [{"speed": 0.79}, {"speed": 1.21}, {"steps": 4}, {"steps": 13}, {"voice": {"name": "missing"}}, {"options": {"steps": 5}}]:
            result = request(args.host, args.port, "F1", input_stream=args.input_stream, extra=extra)
            assert "error" in result and result["events"] == ["error"], result
            failures.append({"request": extra, "response": result})
        (args.out / f"{args.label}-invalid-requests.json").write_text(json.dumps(failures, indent=2))
        # Numeric/unit input and fully spoken equivalent must normalize identically.
        a = request(args.host, args.port, "F1-ruhig", "Heute beträgt die Temperatur 21 °C.", args.input_stream)
        b = request(args.host, args.port, "F1-ruhig", "Heute beträgt die Temperatur einundzwanzig Grad Celsius.", args.input_stream)
        assert "error" not in a and "error" not in b
        assert abs(a["audio"] - b["audio"]) < 0.03, (a["audio"], b["audio"])
        (args.out / f"{args.label}-normalization.json").write_text(json.dumps({"numeric_audio": a["audio"], "spoken_audio": b["audio"]}))
    print("PASS", args.label, flush=True)


if __name__ == "__main__":
    main()
