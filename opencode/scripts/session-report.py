#!/usr/bin/env python3
"""Compare OpenCode sessions side by side from opencode.db (stdlib only; run with python3 -I).

  scripts/session-report.py --latest 4                         # the 4 newest top-level sessions
  scripts/session-report.py --match "ask_jev to find" --latest 3
  scripts/session-report.py ses_e4c6bda2affe... ses_e4cca30e1ffe... --keys 1899,proxy_passthrough
  scripts/session-report.py --list 10                          # newest sessions with their prompts

It reads a COPY of the database (the .db, -wal and -shm files are copied to a temp dir first),
so OpenCode keeps running undisturbed. Per session, including its reviewer sub-sessions:

  wall time (prompt -> last answer), main-agent steps, tool calls (reads/greps with the characters
  they put in context), main-agent prompt tokens and peak context, totals including reviewers,
  reviewer times, ask_jev calls with judged tokens and seconds, requests per jev server, the ask_jev
  router's actions with p(grep), and optionally which --keys strings the final answer mentions.
"""

import argparse
import collections
import datetime
import json
import os
import re
import shutil
import sqlite3
import sys
import tempfile

DEFAULT_DB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "container_data", "local", "share", "opencode", "opencode.db")


def open_copy(db):
    tmp = tempfile.mkdtemp(prefix="session-report-")
    for suffix in ("", "-wal", "-shm"):
        if os.path.exists(db + suffix):
            shutil.copy2(db + suffix, os.path.join(tmp, "opencode.db" + suffix))
    return sqlite3.connect(os.path.join(tmp, "opencode.db")), tmp


def first_prompt(c, sid):
    row = c.execute("select data from session_message where session_id=? and type='user' order by seq limit 1", (sid,)).fetchone()
    return json.loads(row[0]).get("text", "") if row else ""


def clock(ms):
    return datetime.datetime.fromtimestamp(ms / 1000).strftime("%H:%M:%S")


def analyse(c, sid):
    rows = c.execute("select type, data from session_message where session_id=? order by seq", (sid,)).fetchall()
    users = [json.loads(d)["time"]["created"] for t, d in rows if t == "user"]
    start = users[0] if users else 0
    a = {"steps": 0, "tools": collections.Counter(), "chars": collections.Counter(), "ctx_peak": 0, "prompt": 0, "gen": 0,
         "jev_s": 0.0, "jev_tok": 0, "servers": collections.Counter(), "router": [], "final": "", "last": start}
    for t, d in rows:
        if t != "assistant":
            continue
        m = json.loads(d)
        a["steps"] += 1
        tk = m.get("tokens", {})
        p = tk.get("input", 0) + tk.get("cache", {}).get("read", 0)
        a["prompt"] += p
        a["ctx_peak"] = max(a["ctx_peak"], p)
        a["gen"] += tk.get("output", 0) + tk.get("reasoning", 0)
        tm = m.get("time", {})
        a["last"] = max(a["last"], tm.get("completed") or tm.get("created") or 0)
        text = "".join(x.get("text", "") for x in m.get("content", []) if x.get("type") == "text")
        if text.strip():
            a["final"] = text
        for it in m.get("content", []):
            if it.get("type") != "tool":
                continue
            a["tools"][it["name"]] += 1
            st = it.get("state", {})
            out = "".join(x.get("text", "") for x in st.get("content", []) or [] if isinstance(x, dict))
            a["chars"][it["name"]] += len(out)
            if it["name"] == "ask_jev":
                hit = re.search(r"([\d,]+) tokens judged in ([\d.]+) s", out)
                if hit:
                    a["jev_tok"] += int(hit.group(1).replace(",", ""))
                    a["jev_s"] += float(hit.group(2))
                meta = st.get("metadata") or {}
                for url, n in (meta.get("servers") or {}).items():
                    a["servers"][url] += n
                if meta.get("router"):
                    a["router"].append(meta["router"])
    a["wall"] = (a["last"] - start) / 1000 if start else 0
    a["start"] = start
    return a


def report(c, sids, keys):
    res = []
    for sid in sids:
        a = analyse(c, sid)
        kids = [analyse(c, k) for (k,) in c.execute("select id from session_v2 where parent_id=?", (sid,))]
        a["tot_prompt"] = a["prompt"] + sum(k["prompt"] for k in kids)
        a["tot_gen"] = a["gen"] + sum(k["gen"] for k in kids)
        a["reviewers"] = [round(k["wall"]) for k in kids]
        a["kid_jev"] = sum(k["tools"].get("ask_jev", 0) for k in kids)
        a["label"] = f"{clock(a['start'])} `{sid[:14]}…`" if a["start"] else sid
        a["prompt_text"] = first_prompt(c, sid)
        res.append(a)

    def row(name, f):
        print(f"| {name} | " + " | ".join(f(a) for a in res) + " |")

    print("| | " + " | ".join(a["label"] for a in res) + " |")
    print("|---|" + "---|" * len(res))
    row("Prompt", lambda a: a["prompt_text"][:70].replace("|", "/") + ("…" if len(a["prompt_text"]) > 70 else ""))
    row("Wall time", lambda a: f"{a['wall']:.0f} s")
    row("Main-agent steps", lambda a: str(a["steps"]))
    row("Reads / greps / shell", lambda a: f"{a['tools'].get('read', 0)} ({a['chars'].get('read', 0) // 1000}k) / "
                                           f"{a['tools'].get('grep', 0)} / {a['tools'].get('shell', 0)}")
    row("ask_jev calls", lambda a: str(a["tools"].get("ask_jev", 0)) + (f" (+{a['kid_jev']} by reviewers)" if a["kid_jev"] else ""))
    row("ask_jev judged", lambda a: f"{a['jev_tok']:,} tok in {a['jev_s']:.1f} s" if a["jev_tok"] else "—")
    row("Requests per jev server", lambda a: ", ".join(f"{u.rsplit(':', 1)[-1]}: {n}" for u, n in a["servers"].items()) or "—")
    row("Router (action p(grep))", lambda a: ", ".join(
        f"{r.get('action')} {r['p_grep']:.2f}" if r.get("p_grep") is not None else f"{r.get('action')} ({r.get('mode')})"
        for r in a["router"]) or "—")
    row("Main prompt tokens (peak ctx)", lambda a: f"{a['prompt'] // 1000}k ({a['ctx_peak'] // 1000}k)")
    row("Total incl. reviewers", lambda a: f"{a['tot_prompt'] // 1000}k / {a['tot_gen'] // 1000}k gen")
    row("Reviewers", lambda a: " + ".join(f"{x} s" for x in a["reviewers"]) or "—")
    if keys:
        row(f"Keys in answer (of {len(keys)})", lambda a: f"{sum(k in a['final'] for k in keys)}"
            + (f" (missing: {', '.join(k for k in keys if k not in a['final'])})" if any(k not in a["final"] for k in keys) else ""))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("sessions", nargs="*", help="session ids (top-level ones; reviewer sub-sessions are added automatically)")
    p.add_argument("--db", default=DEFAULT_DB)
    p.add_argument("--latest", type=int, help="take the N newest top-level sessions (after --match)")
    p.add_argument("--match", help="only sessions whose first prompt contains this text")
    p.add_argument("--keys", help="comma-separated strings to look for in each final answer")
    p.add_argument("--list", type=int, metavar="N", help="list the N newest top-level sessions with their prompts and stop")
    a = p.parse_args()
    if not os.path.exists(a.db):
        sys.exit(f"no database at {a.db} (pass --db)")
    c, tmp = open_copy(a.db)
    try:
        top = [sid for (sid,) in c.execute("select id from session_v2 where parent_id is null order by time_created desc")]
        if a.list:
            for sid in top[: a.list]:
                tc = c.execute("select time_created from session_v2 where id=?", (sid,)).fetchone()[0]
                print(f"{clock(tc)}  {sid}  {first_prompt(c, sid)[:100]}")
            return
        sids = list(a.sessions)
        if not sids:
            pool = [s for s in top if not a.match or a.match in first_prompt(c, s)]
            sids = pool[: a.latest or 2]
        if not sids:
            sys.exit("no sessions matched")
        report(c, sids, [k for k in (a.keys or "").split(",") if k])
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
