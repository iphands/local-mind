#!/usr/bin/env python3
"""Load generator and summarizer for ./run-jev-4060-benchmark (stdlib only; run with python3 -I).

  jev-bench-load.py run --url http://127.0.0.1:8702 --out cfg.json [--scale 1.0] [--phases ...]
  jev-bench-load.py summarize RESULTS_DIR          # writes RESULTS_DIR/summary.md and prints it

Request shapes mirror what the jev OpenCode plugin really sends
(opencode/container_data/config/opencode/plugins/jev.ts):

  gate     F3 shell check     ~120-token state, 2 questions (shell_risk, reversibility)
  chunk    F4 ask_jev chunk   8000 chars of code (~2k tokens), 1 yes/no question
  verdict  F2 shell verdict   16000 chars of output (~4k tokens), 1 question
  review   F1 pre-screen      24000 chars of diff (~6k tokens), 3 questions

QUESTIONS below is copied from jev.ts (__test.RISK_QUESTIONS, VERDICT_QUESTIONS, REVIEW_QUESTIONS,
yesNoCriteria). Regenerate it from the plugin when those change.

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

def phase_alone(client, payloads, reps):
    out = {}
    for sc in ("gate", "chunk", "verdict", "review"):
        client.ask(sc, payloads.build(sc))  # warmup
        recs = [client.ask(sc, payloads.build(sc)) for _ in range(reps)]
        s = stats(recs)
        if s["p50_ms"]:
            s["tok_per_s_single"] = round(s["tokens"] / max(1, len([r for r in recs if r["ok"]])) / (s["p50_ms"] / 1000), 1)
        out[sc] = s
        print(f"  alone {sc:8} p50 {s['p50_ms']} ms  ({s['errors']} errors)", flush=True)
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
    result = {"url": a.url, "config": a.config, "scale": a.scale, "started": time.strftime("%Y-%m-%d %H:%M:%S")}
    if "alone" in phases:
        result["alone"] = phase_alone(client, payloads, reps)
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


def cmd_summarize(a):
    rows = []
    for path in sorted(glob.glob(os.path.join(a.dir, "cfg-*.json"))):
        with open(path) as f:
            rows.append(json.load(f))
    if not rows:
        sys.exit(f"no cfg-*.json in {a.dir}")
    base = next((r.get("consistency") for r in rows if r.get("consistency")), None)
    lines = [
        f"# jev server benchmark ({os.path.basename(os.path.abspath(a.dir))})", "",
        "Config is NP:KV_UNIFIED:UB:CTX. Latencies in ms; tok/s is prefill tokens per second summed over clients.", "",
        "| config | VRAM peak | gate alone | review alone | chunk tok/s C=1 / 2 / 4 / 8 | mixed gate p50 / p95 / max | mixed bg tok/s | drift vs #1 | errors |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    candidates = []
    for r in rows:
        cfg = r.get("config", "?")
        if r.get("failed"):
            lines.append(f"| `{cfg}` | — | failed to start: {r['failed']} | | | | | | |")
            continue
        al, tp, mx = r.get("alone", {}), r.get("throughput", {}), r.get("mixed", {})
        tps = " / ".join(str(tp.get(c, {}).get("tok_s", "—")) for c in ("1", "2", "4", "8"))
        g = mx.get("gate", {})
        errs = sum(v.get("errors", 0) for v in al.values()) + sum(v.get("errors", 0) for v in tp.values()) \
            + g.get("errors", 0) + mx.get("background", {}).get("errors", 0)
        d = drift(base, r.get("consistency"))
        vram = r.get("vram_peak_mib")
        lines.append(
            f"| `{cfg}` | {vram if vram is not None else '—'} MiB | {al.get('gate', {}).get('p50_ms')} | {al.get('review', {}).get('p50_ms')} | "
            f"{tps} | {g.get('p50_ms')} / {g.get('p95_ms')} / {g.get('max_ms')} | {mx.get('background', {}).get('tok_s')} | "
            f"{d if d is not None else '—'} | {errs} |")
        best_tp = max((tp.get(c, {}).get("tok_s") or 0) for c in ("4", "2", "1"))
        candidates.append({"cfg": cfg, "errs": errs, "drift": d, "vram": vram, "gate_p95": g.get("p95_ms"),
                           "tp4": tp.get("4", {}).get("tok_s") or best_tp, "bg": mx.get("background", {}).get("tok_s")})
    # Judge throughput under the mixed load jev really produces, not raw single-scenario prefill:
    # extra slots cost ~5% of raw chunk tok/s but run multi-question requests far faster.
    top_bg = max((c["bg"] or 0) for c in candidates) if candidates else 0
    ok = [c for c in candidates if c["errs"] == 0 and (c["drift"] is None or c["drift"] <= 0.02)
          and (c["vram"] is None or c["vram"] <= 7782) and c["gate_p95"] is not None and (c["bg"] or 0) >= 0.9 * top_bg]
    lines.append("")
    if ok:
        pick = min(ok, key=lambda c: c["gate_p95"])
        lines.append(f"**Suggestion: `{pick['cfg']}`**: lowest mixed-gate p95 ({pick['gate_p95']} ms) among configs with no errors, "
                     f"drift ≤ 0.02, VRAM ≤ 7.6 GiB and mixed-load throughput within 10% of the best ({top_bg} tok/s).")
    else:
        lines.append("**No config met every rule** (no errors, drift ≤ 0.02, VRAM ≤ 7.6 GiB, mixed throughput within 10% of the best). Read the table.")
    text = "\n".join(lines) + "\n"
    with open(os.path.join(a.dir, "summary.md"), "w") as f:
        f.write(text)
    print(text)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--url", default="http://127.0.0.1:8702")
    r.add_argument("--out")
    r.add_argument("--config", default="?", help="label stored in the result, e.g. 4:1:512:40960")
    r.add_argument("--scale", type=float, default=1.0, help="phase length multiplier (--quick uses 0.33)")
    r.add_argument("--phases", default="alone,throughput,mixed,consistency")
    r.add_argument("--corpus", default=DEFAULT_CORPUS)
    r.add_argument("--timeout", type=float, default=60.0)
    s = sub.add_parser("summarize")
    s.add_argument("dir")
    a = p.parse_args()
    if a.cmd == "run":
        cmd_run(a)
    else:
        cmd_summarize(a)


if __name__ == "__main__":
    main()
