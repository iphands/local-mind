#!/usr/bin/env python3
"""Drive the large-context benchmark against a running server (bench-context).

One process per backend config: a realistic 3-turn convo (primes -> python ->
static quake2 turn from data/test-convo.json) followed by pad probes at exact
context depths. Prints tables and appends per-measurement jsonl records.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from benchclient import (jsonl_appender, metrics, read_watts, result_fields,  # noqa: E402
                         server_version, stream_chat)

ap = argparse.ArgumentParser()
ap.add_argument("host"); ap.add_argument("api_model")
ap.add_argument("--jsonl"); ap.add_argument("--config", default="")
ap.add_argument("--nvfp4", default=""); ap.add_argument("--moe", default="")
ap.add_argument("--mtp", default="")
ap.add_argument("--model-name", default="")
ap.add_argument("--image", default="")
ap.add_argument("--pad-sizes", default="8192 32768 65536 131072")
ap.add_argument("--testconvo", default="")     # data/test-convo.json (static quake2 turn)
ap.add_argument("--gen-convo", type=int, default=1024)
ap.add_argument("--gen-summary", type=int, default=512)
ap.add_argument("--gen-pad", type=int, default=256)
a = ap.parse_args()

append_jsonl = jsonl_appender(a.jsonl, {
    "config": a.config, "nvfp4_backend": a.nvfp4 or "auto",
    "moe_backend": a.moe or "auto", "mtp": a.mtp, "watts_cap": read_watts(),
    "model": a.model_name, "image": a.image or None,
    "vllm_version": server_version(a.host)})

SEED = ("Write a Sieve of Eratosthenes that finds all primes up to 10,000,000 in Bash. "
        "Include comments explaining each step.")
PORT = "Port that program to Python."

# Fixed 3-turn flow: primes (dynamic) -> python (dynamic) -> quake2 (static big turn).
# turn = (label, [user messages to append], gen_tokens). quake2 messages come from the
# generated test-convo.json; if absent, that turn is skipped (non-fatal).
turns = [("primes", [{"role": "user", "content": SEED}], a.gen_convo),
         ("python", [{"role": "user", "content": PORT}], a.gen_convo)]
if a.testconvo:
    try:
        with open(a.testconvo) as f:
            tc = json.load(f)
        turns.append(("quake2", tc["messages"], a.gen_summary))
    except FileNotFoundError:
        print(f"  [convo] {a.testconvo} not found -- run ./make-test-convo first; "
              f"skipping the quake2 turn")
    except Exception as e:
        print(f"  [convo] failed to load {a.testconvo}: {e}; skipping quake2 turn")

# ---- convo mode ----
print(f"  [convo] primes/python (gen {a.gen_convo}) -> quake2 (gen {a.gen_summary})")
print(f"    {'TURN':<8}{'CTX(tok)':>10}{'GEN(tok)':>10}{'TTFT(ms)':>10}{'prefill':>10}{'TG(t/s)':>10}")
messages = []
for idx, (label, user_msgs, gen) in enumerate(turns):
    messages.extend(user_msgs)
    try:
        r = stream_chat(a.host, a.api_model, messages, gen, timeout=1800)
    except Exception as e:
        print(f"    {label:<8} FAILED: {e}")
        append_jsonl({"mode": "convo", "turn": idx, "turn_label": label,
                      "status": "error", "error": str(e)})
        break
    messages.append({"role": "assistant", "content": r["content"]})
    ttft_ms, prefill, tg = metrics(r)
    print(f"    {label:<8}{r['prompt_tokens']:>10}{r['completion_tokens']:>10}"
          f"{ttft_ms:>10}{prefill:>10}{tg:>10}")
    append_jsonl({"mode": "convo", "turn": idx, "turn_label": label,
                  "context_tokens": r["prompt_tokens"], **result_fields(r)})

# ---- pad mode ----
print(f"  [pad] sizes={a.pad_sizes} gen {a.gen_pad}/probe")
print(f"    {'TARGET':>8}{'CTX(tok)':>10}{'GEN(tok)':>10}{'TTFT(ms)':>10}{'prefill':>10}{'TG(t/s)':>10}")
PHRASE = "the quick brown fox jumps over the lazy dog. "   # ~10 tokens/phrase
for size in [int(s) for s in a.pad_sizes.split()]:
    reps = max(1, size // 10)
    # size-unique leading marker so prefix caching can't serve one size from another
    filler = f"[pad-{size}] " + (PHRASE * reps)
    msgs = [{"role": "user", "content": filler +
             "\n\nIgnore the text above. In exactly one word, what is two plus two?"}]
    try:
        # ignore_eos: pad probes always generate exactly gen_pad tokens (stable TG)
        r = stream_chat(a.host, a.api_model, msgs, a.gen_pad,
                        ignore_eos=True, timeout=1800)
    except Exception as e:
        print(f"    {size:>8}  FAILED: {e}")
        append_jsonl({"mode": "pad", "target_tokens": size, "status": "error", "error": str(e)})
        continue
    ttft_ms, prefill, tg = metrics(r)
    print(f"    {size:>8}{r['prompt_tokens']:>10}{r['completion_tokens']:>10}"
          f"{ttft_ms:>10}{prefill:>10}{tg:>10}")
    append_jsonl({"mode": "pad", "target_tokens": size,
                  "context_tokens": r["prompt_tokens"], **result_fields(r)})
