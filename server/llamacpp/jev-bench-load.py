#!/usr/bin/env python3
"""Load generator and summarizer for ./run-jev-benchmark (stdlib only; run with python3 -I).

  jev-bench-load.py run --url http://127.0.0.1:8702 --out cfg.json [--target 6000] [--slot-ctx N] [--scale 1.0]
  jev-bench-load.py summarize DIR [DIR ...]        # one dir: writes DIR/summary.md; several: one combined table

Request shapes mirror what the jev OpenCode plugin really sends
(opencode/container_data/config/opencode/plugins/jev.ts):

  gate     F3 shell check     ~120-token state, 2 questions (shell_risk, reversibility)
  chunk    F4 ask_jev chunk   8000 chars of code (~2k tokens), 1 yes/no question
  verdict  F2 shell verdict   16000 chars of output (~4k tokens), 1 question
  review   F1 pre-screen      24000 chars of diff (~6k tokens), 3 questions

QUESTIONS below is copied from jev.ts (__test.RISK_QUESTIONS, VERDICT_QUESTIONS, REVIEW_QUESTIONS,
yesNoCriteria). Regenerate it from the plugin when those change.

Long state (alone phase only): long27k, one yes/no question over ~100k chars of code (~27k tokens),
just under the endpoint's state cap. /v1/systemone clips any state to ~32k tokens (the model card's
32,000-token state) silently: the server log still says `truncated = 0`. cap_probe sends ~210k chars
once and records how many tokens the server actually kept, so a build that lifts the cap shows up.

Phases (per server config):
  alone        each scenario: 1 warmup + N sequential requests -> p50 latency
  throughput   chunk requests from C = 1, 2, 4, 8 concurrent clients -> req/s, prefill tok/s, p50/p95
  mixed        2 chunk loopers + 1 review looper in the background, a gate request every 0.5 s in the
               foreground -> gate p50/p95/max: is the shell gate still fast while scans and reviews run?
  consistency  one fixed payload per scenario -> probabilities, compared across configs (drift)
"""

import argparse
import glob
import http.client
import json
import os
import statistics
import sys
import threading
import time
import urllib.parse

QUESTIONS = json.loads(r"""
{
 "gate": {
  "shell_risk": {"type": "choice", "criteria": {
    "routine": "routine work inside the project: build, test, run, read, search, create, edit, move or delete project files, clean build output, install dependencies, curl a localhost or .lan service, git add, commit, stash or checkout",
    "destructive": "irreversible loss outside the project or of history: recursive delete of a home, system or root directory, wiping a disk or database, git reset --hard, git clean, force push, deleting branches",
    "network": "runs or fetches code from the internet: piping a downloaded script into a shell, downloading model or binary files, git push",
    "system": "uses sudo, or changes system configuration, services, users, permissions or disks, or kills processes it did not start"}},
  "reversibility": {"type": "choice", "criteria": {
    "read_only": "only reads or inspects and changes nothing: ls, cat, grep, find without -delete, git status, git log, git diff",
    "reversible": "changes that git, a rebuild or a reinstall can undo: edits, moves or deletes of project files or build output (rm -rf node_modules, target/, dist/), git add, commit, stash, checkout, installing dependencies",
    "irreversible": "destroys something nothing can restore: deletes in a home, system or root directory or of untracked user data, find -delete or rm -rf outside the project, dd, mkfs, truncating files or databases, git reset --hard, git clean -fd, git push --force"}}
 },
 "verdict": {
  "verdict": {"type": "choice", "criteria": {
    "pass": "the command succeeded and its output shows no errors or failures",
    "fail": "the command failed: errors, failing tests, crashes or panics",
    "warnings": "the command succeeded but printed warnings worth reading",
    "inconclusive": "the output does not show whether the command succeeded"}}
 },
 "review": {
  "neckbeard_flag": {"type": "choice", "criteria": {
    "none": "no correctness, safety or security problem in this code",
    "type_suppression": "suppresses the type checker: as any, @ts-ignore, type: ignore, unchecked casts",
    "empty_catch": "silently swallows an error: empty catch or except, ignored error result, || true",
    "weakened_test": "deletes, skips or weakens a test or an assertion",
    "hardcoded_secret": "hardcodes a password, token, API key or other credential",
    "injection": "builds a shell command, SQL query or file path from untrusted input without escaping",
    "null_unhandled": "uses a value that may be null, undefined, None or missing without checking it",
    "logic_bug": "has a logic error, an off-by-one, or an unhandled edge case"}},
  "hoodie_flag": {"type": "choice", "criteria": {
    "none": "no performance or usability problem in this code",
    "quadratic": "nested loops or repeated scans where one pass or a lookup table would do",
    "io_in_loop": "makes a network, database, file or subprocess call inside a loop",
    "unbounded_load": "loads a whole file, table or dataset into memory where it could stream or paginate",
    "missing_cache": "recomputes or refetches an expensive result that could be cached",
    "blocking_sync": "blocks the main thread or event loop with synchronous I/O or sleeps",
    "poor_error_message": "error messages that do not say what failed or how to fix it"}},
  "risk": {"type": "choice", "criteria": {
    "trivial": "only comments, docs, formatting, renames or log text change; behaviour is unchanged",
    "standard": "an ordinary code or configuration change",
    "risky": "touches security, auth, data deletion, concurrency, migrations or production config"}}
 },
 "chunk": {
  "answer": {"type": "choice", "criteria": {
    "yes": "this part of the file shows the answer to the question is yes",
    "no": "the answer is no, or this part of the file has nothing to do with the question"}}
 }
}
""")

COMMANDS = [
    "git push --force origin main",
    "cargo build --release",
    "rm -rf ~/projects/old",
    "npm test -- --watch=false",
    "find ~ -name '*.log' -delete",
    "cd src && git status",
    "dd if=/dev/zero of=/dev/sda bs=1M",
    "make clean && make -j16",
]

DEFAULT_CORPUS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "llama-proxy", "src")

# Long single-request state (1 yes/no question): chars, and the per-request token room it needs.
# This corpus runs ~3.5-3.7 chars/token (100k chars -> ~27k tokens measured), under the ~32k cap.
# Bigger states are pointless to time: 2026-11-06 on b12169, "60k" and "114k" states both arrived
# as ~32.1k tokens, so they measured the cap, not long contexts.
LONG = {"long27k": (100_000, 32_768)}
CAP_PROBE_CHARS = 210_000  # ~60k tokens of text; the server keeps ~32.1k of it
LEGACY_LONG = ("long28k",)  # the same test under its old name, in results from before the cap was found


# ---------------------------------------------------------------- payloads

class Payloads:
    """Builds realistic states; each call takes a different slice so slot prompt reuse can't flatter a config."""

    def __init__(self, corpus_dir):
        files = sorted(glob.glob(os.path.join(corpus_dir, "**", "*.rs"), recursive=True))
        if not files:
            sys.exit(f"no *.rs files under {corpus_dir} (pass --corpus)")
        self.text = "\n".join(open(f, encoding="utf-8", errors="replace").read() for f in files)
        self.n = 0
        self.lock = threading.Lock()

    def _slice(self, size, fixed):
        with self.lock:
            i = 0 if fixed else self.n
            self.n += 1
        start = (i * 7919) % max(1, len(self.text) - size)
        return self.text[start:start + size]

    def build(self, scenario, fixed=False):
        if scenario in LONG:
            body = self._slice(LONG[scenario][0], fixed)
            state = ("A coding agent asks a question about one long file. Answer from the whole text.\n\n"
                     "Question: Does this file use a mutex or read-write lock?\n\n"
                     f"File: src/big.rs (all of it)\n\n{body}")
            return {"state": state, "questions": QUESTIONS["chunk"]}
        if scenario == "gate":
            with self.lock:
                cmd = COMMANDS[0 if fixed else self.n % len(COMMANDS)]
                self.n += 1
            state = f"A coding agent wants to run this shell command without asking the user.\n\n$ {cmd}"
        elif scenario == "chunk":
            body = self._slice(8000, fixed)
            state = ("A coding agent asks a question about one file. Answer from this part of the file only.\n\n"
                     "Question: Does this file use a mutex or read-write lock?\n\n"
                     f"File: src/lib.rs (lines 1-200 of 900, part 1 of 4)\n\n{body}")
        elif scenario == "verdict":
            body = self._slice(16000, fixed)
            state = ("A coding agent ran a shell command. Judge from the output whether it really succeeded.\n\n"
                     f"$ cargo test --all\nexit code: 0\n\n{body}")
        elif scenario == "review":
            body = "\n".join("+" + line for line in self._slice(24000, fixed).split("\n"))
            state = ("A coding agent wrote the change below and is sending it to code review. "
                     f"Judge the change itself.\n\n{body}")
        else:
            raise ValueError(scenario)
        return {"state": state, "questions": QUESTIONS[scenario]}


# ---------------------------------------------------------------- client

class Client:
    def __init__(self, url, timeout):
        u = urllib.parse.urlparse(url)
        self.host, self.port, self.timeout = u.hostname, u.port or 80, timeout
        self.local = threading.local()

    def _conn(self):
        c = getattr(self.local, "conn", None)
        if c is None:
            c = self.local.conn = http.client.HTTPConnection(self.host, self.port, timeout=self.timeout)
        return c

    def _post(self, body):
        # A kept-alive connection the server closed while idle fails on reuse; retry once on a fresh one
        for attempt in (0, 1):
            fresh = getattr(self.local, "conn", None) is None
            c = self._conn()
            try:
                c.request("POST", "/v1/systemone", body, {"content-type": "application/json"})
                r = c.getresponse()
                return r, r.read()
            except (http.client.RemoteDisconnected, BrokenPipeError, ConnectionResetError):
                self.local.conn = None
                if fresh or attempt:
                    raise

    def ask(self, scenario, payload):
        """One /v1/systemone request -> record dict. Never raises."""
        body = json.dumps(payload).encode()
        t0 = time.monotonic()
        rec = {"scenario": scenario, "t": time.time(), "ok": False}
        try:
            r, data = self._post(body)
            rec["status"] = r.status
            if r.status != 200:
                rec["error"] = f"HTTP {r.status}: {data[:160].decode(errors='replace')}"
            else:
                d = json.loads(data)
                answers = d.get("answers") or {}
                missing = [q for q in payload["questions"] if not isinstance(answers.get(q, {}).get("probabilities"), dict)]
                if missing:
                    rec["error"] = f"malformed answer for {missing[0]}"
                else:
                    rec["ok"] = True
                    rec["probs"] = {q: answers[q]["probabilities"] for q in payload["questions"]}
                rec["tokens"] = (d.get("usage") or {}).get("input_tokens", 0)
        except Exception as e:  # noqa: BLE001 - record every failure
            rec["error"] = f"{type(e).__name__}: {e}"
            self.local.conn = None
        rec["ms"] = round((time.monotonic() - t0) * 1000, 1)
        return rec


def pct(values, p):
    if not values:
        return None
    s = sorted(values)
    k = max(0, min(len(s) - 1, int(round(p / 100 * (len(s) - 1)))))
    return round(s[k], 1)


def stats(recs, wall_s=None):
    ok = [r for r in recs if r["ok"]]
    ms = [r["ms"] for r in ok]
    out = {"n": len(recs), "errors": len(recs) - len(ok), "p50_ms": pct(ms, 50), "p95_ms": pct(ms, 95),
           "max_ms": max(ms) if ms else None, "tokens": sum(r.get("tokens", 0) for r in ok)}
    if wall_s:
        out["req_s"] = round(len(ok) / wall_s, 2)
        out["tok_s"] = round(out["tokens"] / wall_s, 1)
    if len(recs) > len(ok):
        out["first_error"] = next(r.get("error") for r in recs if not r["ok"])
    return out


def looper(client, payloads, scenario, until, sink):
    while time.monotonic() < until:
        sink.append(client.ask(scenario, payloads.build(scenario)))


# ---------------------------------------------------------------- phases

def phase_alone(client, payloads, reps, slot_ctx=0, long_reps=2):
    out = {}
    for sc in ("gate", "chunk", "verdict", "review"):
        client.ask(sc, payloads.build(sc))  # warmup
        recs = [client.ask(sc, payloads.build(sc)) for _ in range(reps)]
        s = stats(recs)
        if s["p50_ms"]:
            s["tok_per_s_single"] = round(s["tokens"] / max(1, len([r for r in recs if r["ok"]])) / (s["p50_ms"] / 1000), 1)
        out[sc] = s
        print(f"  alone {sc:8} p50 {s['p50_ms']} ms  ({s['errors']} errors)", flush=True)
    # Long states, one request at a time, only where the config has room for them
    for sc, (_, need) in LONG.items():
        if slot_ctx and slot_ctx < need:
            continue
        recs = [client.ask(sc, payloads.build(sc)) for _ in range(long_reps)]
        s = stats(recs)
        if s["p50_ms"]:
            s["tok_per_s_single"] = round(s["tokens"] / max(1, len([r for r in recs if r["ok"]])) / (s["p50_ms"] / 1000), 1)
        out[sc] = s
        print(f"  alone {sc:8} p50 {s['p50_ms']} ms, {s.get('tok_per_s_single')} tok/s  ({s['errors']} errors)", flush=True)
    # How much of an oversized state the server keeps (the cap), when the config could hold it
    if not slot_ctx or slot_ctx >= 40_960:
        state = payloads.build("long27k")
        state["state"] = state["state"][:-LONG["long27k"][0]] + payloads._slice(CAP_PROBE_CHARS, False)
        r = client.ask("cap_probe", state)
        out["cap_probe"] = {"ok": r["ok"], "chars": CAP_PROBE_CHARS, "tokens_kept": r.get("tokens"), "ms": r["ms"],
                            "error": r.get("error")}
        print(f"  state cap: {CAP_PROBE_CHARS} chars sent, server kept {r.get('tokens')} tokens", flush=True)
    return out


def phase_throughput(client, payloads, clients, seconds):
    out = {}
    for c in clients:
        sink, until = [], time.monotonic() + seconds
        t0 = time.monotonic()
        threads = [threading.Thread(target=looper, args=(client, payloads, "chunk", until, sink)) for _ in range(c)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        s = stats(sink, time.monotonic() - t0)
        out[str(c)] = s
        print(f"  throughput C={c}: {s['tok_s']} tok/s, {s['req_s']} req/s, p50 {s['p50_ms']} p95 {s['p95_ms']} ms ({s['errors']} errors)", flush=True)
    return out


def phase_mixed(client, payloads, seconds, interval=0.5):
    bg, until = [], time.monotonic() + seconds
    threads = [threading.Thread(target=looper, args=(client, payloads, sc, until, bg)) for sc in ("chunk", "chunk", "review")]
    t0 = time.monotonic()
    for t in threads:
        t.start()
    time.sleep(1.0)  # let the background reach the server first
    gate, nxt = [], time.monotonic()
    while time.monotonic() < until:
        gate.append(client.ask("gate", payloads.build("gate")))
        nxt += interval
        time.sleep(max(0.0, nxt - time.monotonic()))
    for t in threads:
        t.join()
    wall = time.monotonic() - t0
    g = stats(gate)
    g["latencies_ms"] = [r["ms"] for r in gate if r["ok"]]
    out = {"gate": g, "background": stats(bg, wall),
           "background_by_scenario": {sc: stats([r for r in bg if r["scenario"] == sc]) for sc in ("chunk", "review")}}
    print(f"  mixed: gate p50 {g['p50_ms']} p95 {g['p95_ms']} max {g['max_ms']} ms; background {out['background']['tok_s']} tok/s ({g['errors'] + out['background']['errors']} errors)", flush=True)
    return out


def phase_consistency(client, payloads):
    out = {}
    for sc in ("gate", "chunk", "verdict", "review"):
        r = client.ask(sc, payloads.build(sc, fixed=True))
        out[sc] = r.get("probs") if r["ok"] else {"error": r.get("error")}
    return out


def cmd_run(a):
    payloads = Payloads(a.corpus)
    client = Client(a.url, a.timeout)
    phases = a.phases.split(",")
    reps = max(1, round(5 * a.scale))
    secs = lambda base: max(5.0, base * a.scale)  # noqa: E731
    result = {"url": a.url, "target": a.target, "config": a.config, "slot_ctx": a.slot_ctx, "scale": a.scale,
              "started": time.strftime("%Y-%m-%d %H:%M:%S")}
    if "alone" in phases:
        result["alone"] = phase_alone(client, payloads, reps, a.slot_ctx, long_reps=max(1, round(2 * a.scale)))
    if "throughput" in phases:
        clients = [1, 2, 4, 8] if a.scale >= 0.5 else [1, 4]
        result["throughput"] = phase_throughput(client, payloads, clients, secs(20))
    if "mixed" in phases:
        result["mixed"] = phase_mixed(client, payloads, secs(30))
    if "consistency" in phases:
        result["consistency"] = phase_consistency(client, payloads)
    if a.out:
        with open(a.out, "w") as f:
            json.dump(result, f, indent=1)
    return result


# ---------------------------------------------------------------- summary

def drift(base, other):
    """Largest absolute probability difference over every scenario/question/label."""
    worst = 0.0
    for sc, qs in (base or {}).items():
        oq = (other or {}).get(sc)
        if not isinstance(qs, dict) or not isinstance(oq, dict) or "error" in qs or "error" in oq:
            return None
        for q, labels in qs.items():
            for label, p in labels.items():
                worst = max(worst, abs(p - oq.get(q, {}).get(label, 0.0)))
    return round(worst, 3)


VRAM_LIMIT_MIB = {"4060": 7782}  # the 4060 must fit whole; the 6000's limit is the run's reserve guard


def target_of(r, path):
    """Older results (before the 6000 existed) carry no target and were all 4060 runs."""
    return r.get("target") or ("6000" if "jev-6000-" in path else "4060")


def cmd_summarize(a):
    rows = []
    for d in a.dirs:
        for path in sorted(glob.glob(os.path.join(d, "cfg-*.json"))):
            with open(path) as f:
                r = json.load(f)
            r["_target"] = target_of(r, path)
            rows.append(r)
    if not rows:
        sys.exit(f"no cfg-*.json in {' '.join(a.dirs)}")
    base = next((r.get("consistency") for r in rows if r.get("consistency")), None)
    title = ", ".join(os.path.basename(os.path.abspath(d)) for d in a.dirs)
    lines = [
        f"# jev server benchmark ({title})", "",
        "Config is NP:KV_UNIFIED:UB:CTX[:BATCH]. Latencies in ms; tok/s is prefill tokens per second summed over clients. "
        "VRAM is the server process's own. long 27k = one request with a ~27k-token state. "
        "state cap = tokens the server kept of a ~60k-token state (/v1/systemone clips states at ~32k, silently). "
        "Drift is against the first config listed.", "",
        "| gpu | config | VRAM | gate alone | review alone | long 27k | state cap | chunk tok/s C=1 / 2 / 4 / 8 | mixed gate p50 / p95 / max | mixed bg tok/s | drift | errors |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    candidates = []
    for r in rows:
        cfg, tgt = r.get("config", "?"), r["_target"]
        if r.get("failed") and not r.get("alone"):
            lines.append(f"| {tgt} | `{cfg}` | — | failed: {r['failed']} | | | | | | | | |")
            continue
        al, tp, mx = r.get("alone", {}), r.get("throughput", {}), r.get("mixed", {})
        tps = " / ".join(str(tp.get(c, {}).get("tok_s", "—")) for c in ("1", "2", "4", "8"))
        long_row = next((al[k] for k in (*LONG, *LEGACY_LONG) if al.get(k, {}).get("p50_ms")), {})
        longs = str(long_row.get("p50_ms", "—"))
        cap = al.get("cap_probe", {}).get("tokens_kept")
        if cap is None and al.get("long60k", {}).get("n"):  # older results: the "60k" test was the cap
            cap = round(al["long60k"]["tokens"] / al["long60k"]["n"])
        g = mx.get("gate", {})
        errs = sum(v.get("errors", 0) for k, v in al.items() if k != "cap_probe") + sum(v.get("errors", 0) for v in tp.values()) \
            + g.get("errors", 0) + mx.get("background", {}).get("errors", 0)
        d = drift(base, r.get("consistency"))
        vram = r.get("vram_peak_mib")
        note = f" ({r['failed']})" if r.get("failed") else ""
        lines.append(
            f"| {tgt} | `{cfg}`{note} | {vram if vram is not None else '—'} MiB | {al.get('gate', {}).get('p50_ms')} | "
            f"{al.get('review', {}).get('p50_ms')} | {longs} | {cap if cap is not None else '—'} | {tps} | "
            f"{g.get('p50_ms')} / {g.get('p95_ms')} / {g.get('max_ms')} | "
            f"{mx.get('background', {}).get('tok_s')} | {d if d is not None else '—'} | {errs} |")
        candidates.append({"cfg": cfg, "tgt": tgt, "errs": errs + (1 if r.get("failed") else 0), "drift": d, "vram": vram,
                           "gate_p95": g.get("p95_ms"), "bg": mx.get("background", {}).get("tok_s")})
    # Judge throughput under the mixed load jev really produces, not raw single-scenario prefill:
    # extra slots cost ~5% of raw chunk tok/s but run multi-question requests far faster.
    lines.append("")
    for tgt in sorted({c["tgt"] for c in candidates}):
        group = [c for c in candidates if c["tgt"] == tgt]
        top_bg = max((c["bg"] or 0) for c in group)
        limit = VRAM_LIMIT_MIB.get(tgt)
        ok = [c for c in group if c["errs"] == 0 and (c["drift"] is None or c["drift"] <= 0.02)
              and (limit is None or c["vram"] is None or c["vram"] <= limit) and c["gate_p95"] is not None
              and (c["bg"] or 0) >= 0.9 * top_bg]
        rule = f"no errors, drift ≤ 0.02{f', VRAM ≤ {limit} MiB' if limit else ''}, mixed-load throughput within 10% of the best ({top_bg} tok/s)"
        if ok:
            pick = min(ok, key=lambda c: c["gate_p95"])
            lines.append(f"**Suggestion for the {tgt}: `{pick['cfg']}`**: lowest mixed-gate p95 ({pick['gate_p95']} ms) among configs with {rule}.")
        else:
            lines.append(f"**No {tgt} config met every rule** ({rule}). Read the table.")
    text = "\n".join(lines) + "\n"
    if len(a.dirs) == 1:
        with open(os.path.join(a.dirs[0], "summary.md"), "w") as f:
            f.write(text)
    print(text)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--url", default="http://127.0.0.1:8702")
    r.add_argument("--out")
    r.add_argument("--config", default="?", help="label stored in the result, e.g. 4:1:512:40960")
    r.add_argument("--target", default="?", help="GPU label stored in the result: 4060 or 6000")
    r.add_argument("--slot-ctx", type=int, default=0,
                   help="tokens one request may use (CTX when unified, CTX/NP otherwise); long states that don't fit are skipped")
    r.add_argument("--scale", type=float, default=1.0, help="phase length multiplier (--quick uses 0.33)")
    r.add_argument("--phases", default="alone,throughput,mixed,consistency")
    r.add_argument("--corpus", default=DEFAULT_CORPUS)
    r.add_argument("--timeout", type=float, default=180.0, help="per request; the 114k-token state needs room")
    s = sub.add_parser("summarize", help="one or more result dirs; several dirs give one combined table")
    s.add_argument("dirs", nargs="+")
    a = p.parse_args()
    if a.cmd == "run":
        cmd_run(a)
    else:
        cmd_summarize(a)


if __name__ == "__main__":
    main()
