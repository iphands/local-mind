#!/usr/bin/env python3
"""One measured chat request against a running server (bench-wrapper's meter).

Bash-friendly CLI: prints "ttft_ms prefill_tps tg_tps ptok ctok" on success,
"ERROR ..." on failure. --print-watts prints the GPU0 power cap and exits.
Optionally appends a full record (incl. watt cap + provenance) to a jsonl.
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from benchclient import (jsonl_appender, read_watts, result_fields,  # noqa: E402
                         server_version, stream_chat)

ap = argparse.ArgumentParser()
ap.add_argument("host", nargs="?"); ap.add_argument("api_model", nargs="?")
ap.add_argument("prompt", nargs="?"); ap.add_argument("max_tokens", nargs="?", type=int)
ap.add_argument("--print-watts", action="store_true")  # just print GPU0 cap and exit
ap.add_argument("--jsonl")                 # if set, append one record here
ap.add_argument("--config", default="")
ap.add_argument("--nvfp4", default="")
ap.add_argument("--moe", default="")
ap.add_argument("--mtp", default="")
ap.add_argument("--model-name", default="")
ap.add_argument("--run", default="")
ap.add_argument("--mode", default="single")   # tag in jsonl (single|convo|pad)
ap.add_argument("--image", default="")        # docker image provenance
ap.add_argument("--ignore-eos", action="store_true")  # always generate max_tokens
a = ap.parse_args()

if a.print_watts:
    w = read_watts()
    print(w if w is not None else "-"); sys.exit(0)

if not (a.host and a.api_model and a.prompt and a.max_tokens):
    print("ERROR missing-args"); sys.exit(2)

append_jsonl = jsonl_appender(a.jsonl, {
    "mode": a.mode, "config": a.config, "nvfp4_backend": a.nvfp4 or "auto",
    "moe_backend": a.moe or "auto", "mtp": a.mtp, "watts_cap": read_watts(),
    "model": a.model_name, "run": a.run, "image": a.image or None,
    "vllm_version": server_version(a.host)})

try:
    r = stream_chat(a.host, a.api_model, [{"role": "user", "content": a.prompt}],
                    a.max_tokens, ignore_eos=a.ignore_eos, timeout=900)
except Exception as e:
    append_jsonl({"status": "error", "error": str(e)})
    print(f"ERROR {e}"); sys.exit(1)

rec = result_fields(r)
append_jsonl(rec)
print(f"{rec['ttft_ms']:.1f} {rec['prefill_tps']:.1f} {rec['tg_tps']:.2f} "
      f"{r['prompt_tokens']} {r['completion_tokens']}")
