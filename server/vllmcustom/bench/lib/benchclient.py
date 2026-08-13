"""Shared measurement client for the bench-* drivers.

One copy of the SSE streaming chat client, the GPU0 watts reader, and the
metric/jsonl helpers — used by ./bench, lib/measure.py (bench-wrapper) and
lib/ctxbench.py (bench-context).
"""
import json
import subprocess
import time
import urllib.request
from datetime import datetime, timezone


def read_watts():
    """GPU 0 current power cap (W, int) via nvidia-smi. Read-only; None on failure."""
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=power.limit",
             "--format=csv,noheader,nounits", "-i", "0"],
            capture_output=True, text=True, timeout=10)
        return int(round(float(out.stdout.strip().splitlines()[0])))
    except Exception:
        return None


def server_version(host, timeout=10):
    """vLLM /version string (jsonl provenance); None on failure."""
    try:
        with urllib.request.urlopen(f"http://{host}/version", timeout=timeout) as r:
            return json.load(r).get("version")
    except Exception:
        return None


def stream_chat(host, api_model, messages, max_tokens, *, temperature=0.6,
                ignore_eos=False, timeout=1800):
    """POST a streaming chat completion and time it. Returns a dict:

      ttft              first token of ANY kind (reasoning or answer), seconds
      answer_ttft       first non-reasoning token, seconds (None if all thinking)
      total             wall time to [DONE], seconds
      prompt_tokens / completion_tokens / cached_tokens   from usage
      content           concatenated answer text (reasoning excluded)

    With a reasoning parser the stream carries thinking deltas (`reasoning` in
    this vLLM build; `reasoning_content` in others) until the answer starts, so
    TTFT must fire on those too: counting only `content` misattributes all
    thinking time (or, when the whole generation is thinking, the prefill too)
    to "before the first token".
    """
    body = {"model": api_model, "messages": messages, "max_tokens": max_tokens,
            "temperature": temperature, "stream": True,
            "stream_options": {"include_usage": True}}
    if ignore_eos:
        body["ignore_eos"] = True   # vLLM extension: always generate max_tokens
    req = urllib.request.Request(f"http://{host}/v1/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    start = time.perf_counter()
    ttft = answer_ttft = None
    ptok = ctok = cached = None
    parts = []
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        for raw in resp:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            data = line[5:].strip()
            if data == "[DONE]":
                break
            ch = json.loads(data)
            choices = ch.get("choices")
            if choices:
                delta = choices[0].get("delta", {})
                if ttft is None and (delta.get("content") or delta.get("reasoning")
                                     or delta.get("reasoning_content")):
                    ttft = time.perf_counter() - start
                if delta.get("content"):
                    if answer_ttft is None:
                        answer_ttft = time.perf_counter() - start
                    parts.append(delta["content"])
            if ch.get("usage"):
                u = ch["usage"]
                ptok = u.get("prompt_tokens")
                ctok = u.get("completion_tokens")
                cached = (u.get("prompt_tokens_details") or {}).get("cached_tokens")
    total = time.perf_counter() - start
    if not ctok:
        raise RuntimeError("no-usage-reported")
    return {"ttft": ttft or 0.0, "answer_ttft": answer_ttft, "total": total,
            "prompt_tokens": ptok, "completion_tokens": ctok,
            "cached_tokens": cached, "content": "".join(parts)}


def metrics(r):
    """(ttft_ms, prefill_tps, tg_tps) from a stream_chat() result."""
    ttft, total = r["ttft"], r["total"]
    gen_time = max(total - ttft, 1e-6)
    prefill = (r["prompt_tokens"] / ttft) if (ttft > 0 and r["prompt_tokens"]) else 0.0
    return round(ttft * 1000, 1), round(prefill, 1), round(r["completion_tokens"] / gen_time, 2)


def jsonl_appender(path, base):
    """Return append(rec): stamps ts + base fields, appends one JSON line to path.

    No-op if path is falsy. base fields are per-run constants (config, watts, ...).
    """
    def append(rec):
        if not path:
            return
        out = {"ts": datetime.now(timezone.utc).isoformat(timespec="seconds"),
               **base, **rec}
        with open(path, "a") as f:
            f.write(json.dumps(out) + "\n")
    return append


def result_fields(r):
    """Common ok-record jsonl fields for a stream_chat() result."""
    ttft_ms, prefill, tg = metrics(r)
    rec = {"status": "ok", "ttft_ms": ttft_ms, "prefill_tps": prefill, "tg_tps": tg,
           "prompt_tokens": r["prompt_tokens"], "completion_tokens": r["completion_tokens"]}
    if r.get("answer_ttft") is not None:
        rec["answer_ttft_ms"] = round(r["answer_ttft"] * 1000, 1)
    if r.get("cached_tokens") is not None:
        rec["cached_tokens"] = r["cached_tokens"]
    return rec
