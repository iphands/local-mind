#!/usr/bin/env bash
# Follow-up to tmp_sweep_batched: MAX_BATCHED_TOKENS {4096 6019 8192} with a ~100k-token prompt.
# The 12k prompt could not tell these three apart. A 100k prefill takes ~25 steps at 4096 vs ~13
# at 8192, so any fixed per-step cost (and the longer attention per chunk late in the prompt)
# should show here if it matters.
#
#   REPLACE=1 ./tmp_followup.sh                     # stops a running vllm first (otherwise it refuses)
#   XL_REPS=40000 ROUNDS=5 ./tmp_followup.sh        # bigger prompt, more rounds
#   STAGGER=0 ./tmp_followup.sh                     # all prefills at once
#
# How: tmp_sweep_batched (-> scripts/flashnext-conc-sweep) with CONC_TOOL pointed at a small
# wrapper around bench/conc that replaces bench's "long" prompt with the same filler repeated
# XL_REPS times (~3 tokens each, so ~100k tokens) and XL_MAX_TOKENS out. The tables still say
# "long"; in this run it means the ~100k prompt. The wrapper also logs the server's own
# prompt_tokens / cached_tokens per request to xl-usage.jsonl, so the prompt size is measured,
# not assumed, and a prefix-cache hit (cached_tokens > 0) would show.
#
# STAGGER defaults to 8 s and XL_MAX_TOKENS to 2048: a 100k prefill takes several seconds, so
# with the 12k sweep's 2 s the prefills would just queue behind each other; 8 s lets each later
# prefill land while the earlier streams decode. Stream 0's TTFT is the uncontended 100k prefill.
# Roughly 30-40 min (3 boots). Nothing is left running.
# Output: bench/bench-logs/flashnext-batched-100k-<UTC stamp>/
set -uo pipefail
cd "$(dirname "$0")" || exit 1
HERE=$PWD

export XL_REPS=${XL_REPS:-33000}
export XL_MAX_TOKENS=${XL_MAX_TOKENS:-2048}
[[ $XL_REPS =~ ^[1-9][0-9]*$ && $XL_MAX_TOKENS =~ ^[1-9][0-9]*$ ]] || {
  echo "## XL_REPS and XL_MAX_TOKENS must be positive integers"; exit 2; }
export VALUES=${VALUES:-4096 6019 8192}
export PROMPTS=long
export CONC=${CONC:-1,2,4}
export STAGGER=${STAGGER:-8}
export ROUNDS=${ROUNDS:-3}
export OUT=${OUT:-$HERE/bench/bench-logs/flashnext-batched-100k-$(date -u +%Y%m%d-%H%M%S)}
mkdir -p "$OUT"
export XL_CONC=$HERE/bench/conc
export XL_USAGE=$OUT/xl-usage.jsonl
: > "$XL_USAGE"

# The wrapper is written into the output dir, so the run records exactly what it ran.
export CONC_TOOL=$OUT/conc-xlong
cat > "$CONC_TOOL" <<'PY'
#!/usr/bin/env python3
"""bench/conc with its "long" prompt replaced by XL_REPS filler lines; logs usage per request."""
import json, os, runpy, sys, threading

g = runpy.run_path(os.environ["XL_CONC"], run_name="conc_xlong")   # main() not run
g["PROMPTS"]["long"] = ("Summarize the history of computing.\n\n" + "context line. " * int(os.environ["XL_REPS"]),
                        int(os.environ["XL_MAX_TOKENS"]))           # same dict object one_round reads
G = g["one_round"].__globals__
_orig, _lock, _usage = G["stream_chat"], threading.Lock(), os.environ.get("XL_USAGE")


def stream_chat(*a, **k):
    r = _orig(*a, **k)
    if _usage:
        rec = {"label": sys.argv[1], "prompt_tokens": r.get("prompt_tokens"),
               "cached_tokens": r.get("cached_tokens"), "ttft_ms": round(r["ttft"] * 1000, 1)}
        with _lock, open(_usage, "a") as f:
            f.write(json.dumps(rec) + "\n")
    return r


G["stream_chat"] = stream_chat
g["main"]()
PY
chmod +x "$CONC_TOOL"

echo "## tmp_followup: ~100k prompt (XL_REPS=$XL_REPS, $XL_MAX_TOKENS tokens out), MAX_BATCHED_TOKENS {$VALUES}, streams {$CONC}, stagger ${STAGGER}s"
rc=0
./tmp_sweep_batched "$@" || rc=$?

# What the prompt actually was, from the server's usage counts.
if [[ -s $XL_USAGE ]]; then
  python3 -I - "$XL_USAGE" <<'PY' | tee -a "$OUT/summary.md"
import json, sys
rs = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
pt = [r["prompt_tokens"] for r in rs if r.get("prompt_tokens")]
ct = [r["cached_tokens"] or 0 for r in rs]
print("\n## Prompt (tmp_followup)\n")
print("In this run, prompt \"long\" is the ~100k filler prompt, not bench's 12k one.")
print(f"Server-reported prompt_tokens over {len(rs)} requests: "
      + (f"{min(pt):,}–{max(pt):,}" if pt else "not reported"))
hits = sum(1 for c in ct if c > 0)
print(f"Prefix-cache hits (cached_tokens > 0): {hits}" + (" -- TTFT for those requests is NOT a cold prefill" if hits else ""))
PY
fi
exit "$rc"
