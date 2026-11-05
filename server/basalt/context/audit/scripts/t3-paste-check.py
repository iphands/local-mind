#!/usr/bin/env python3
"""Re-verify every claim in context/audit/wave0/t3-negctl-rechecks.txt. Two modes.

  --tree-only    the three families that need nothing but repo bytes: I (rebuild the ten control
                 programs from the controls file and recompute § 3b), K (machine-format values
                 stated once outside transcript blocks), L (each control block's REBUILT_SHA= equal
                 to § 3b's column 2). This is the mode that survives a scratch sweep.
  (default)      all twelve families. SEVEN of them read the batch transcripts this paste documents
                 — /tmp/opencode/recheck2 (the batch), /tmp/opencode/recheck (the earlier batch),
                 /tmp/opencode/degraded (the degraded-state model), /tmp/opencode/basalt-audit/t3
                 (run-real artifacts) — which do not survive. If they are absent this mode refuses
                 rather than reporting a short green. Override the locations with
                 VP_BATCH_NEW / VP_BATCH_OLD / VP_DEGRADED / VP_T3 / VP_C2 / VP_SNAPSHOT.
  --self-test    mutate a copy of the paste (or of the controls file) twenty-two ways and require
                 the checker to exit 1 AND name the family that owns each defect; then run the clean
                 file and require exit 0. This is why "shown capable of firing" is not a remembered
                 number. With --tree-only it runs the nine cases whose family needs no transcripts,
                 children in --tree-only too, and prints how many it did not run.
                 Only ever runs when asked: each case re-executes this file, so an unguarded call is
                 a fork bomb (it took the host down on 2026-11-05; see run_selftest()).

This checker exists because the first version of the evidence file was hand-typed and four of its
ten GENERATOR_SHA256 values were fabricated from scrollback, and because a later cut asserted in
prose that a degraded run exits 0 when the generator exits 1. Nothing here asks whether a rendered
number looks plausible: every check re-derives its value from a file, a subprocess, or the
generator source, and every empty extraction is a failure rather than a skip.

EXIT: 0 all checks passed, 1 a check failed, 2 usage or a missing corpus in the default mode.

WHY THE CONTROLS FILE'S FIRST BLOCK SITS 200 LINES DOWN, since a metric once tried to "fix" that:
the banners above it are warnings attached to the evidence below, and a layout score that rewarded
moving them below the evidence would reward a file for under-warning. So the coordinate is reported
as a re-derive command (`grep -n '^### C1 '`), never as a number, and the order stays.
"""
import collections
import hashlib
import os
import pathlib
import re
import subprocess
import sys
import atexit
import shutil
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[3]
TREE_ONLY = "--tree-only" in sys.argv
SELFTEST = "--self-test" in sys.argv
# set in every self-test child's environment; run_selftest() refuses to start when it is present
CHILD_ENV = "T3_PASTE_CHECK_SELFTEST_CHILD"
# The batch transcripts. Overridable so a re-run against a saved batch is possible; absent by
# default on a clean machine, which is what --tree-only exists for.
W = pathlib.Path(os.environ.get("VP_BATCH_NEW", "/tmp/opencode/recheck2"))
V = pathlib.Path(os.environ.get("VP_BATCH_OLD", "/tmp/opencode/recheck"))
V_C2 = pathlib.Path(os.environ.get("VP_C2", "/tmp/opencode/c2/C2.out"))
DEG = pathlib.Path(os.environ.get("VP_DEGRADED", "/tmp/opencode/degraded"))
T3 = pathlib.Path(os.environ.get("VP_T3", "/tmp/opencode/basalt-audit/t3"))
GEN = ROOT / "context/audit/scripts/t3-census.sh"
APPLIER = ROOT / "context/audit/scripts/t3-make-mutant.py"
CTRL = pathlib.Path(os.environ.get("VP_CONTROLS",
                             str(ROOT / "context/audit/wave0/t3-census-controls.txt")))
REAL = ROOT / "context/audit/wave0/t3-sentinels-real.txt"
PASTE = pathlib.Path(os.environ.get("VP_PASTE",
                                    str(ROOT / "context/audit/wave0/t3-negctl-rechecks.txt")))
SNAPF = pathlib.Path(os.environ.get("VP_SNAPSHOT", "/tmp/opencode/rederive/controls.snapshot.txt"))
REFIRE = pathlib.Path(tempfile.mkdtemp(prefix="t3-paste-check-refire."))
# a family may sys.exit from anywhere, so cleanup cannot live at the bottom of the file
atexit.register(shutil.rmtree, REFIRE, ignore_errors=True)
PIN = "1bab1ec5e7933c90a80a97aa875218351052280445d585bfce6645f036128106"
NS = os.environ.get("VP_T3", "/tmp/opencode/basalt-audit/t3")
ELL = "\u2026"
ELL = "\u2026"

ORDER = ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"]
NEGCTL = [c for c in ORDER if c != "C2"]
TARGETS = {
    "C1": "GENERATOR_SHA_IS_64_HEX", "C2": "SENTINEL_TOTAL_AT_OR_ABOVE_FLOOR",
    "C3": "DECLARED_LABELS_EQ_FILES_COLUMN", "C4": "COLLAPSE_COUNT_MATCHES_HITS_IN_FILE",
    "C5": "SITE_ROWS_VERDICT_MATCHES_TN", "C6": "UNIVERSE_LIST_IS_SORTED",
    "C7": "SORT_DEFAULT_ORDER_EQ_C", "C8": "S14_DENOM_PY_FILES_GE_1",
    "C9": "PREDICATE_SHA_SAME_AS_REAL", "C10": "INPUTS_SHA_SAME_AS_REAL",
}

bad = 0
lines = PASTE.read_text().split("\n")
joined = "\n".join(lines)


def fail(msg):
    global bad
    bad += 1
    print("  BAD " + msg)


def ok(msg):
    print("  ok  " + msg)


def die(msg):
    print("  CANNOT CONTINUE: " + msg)
    sys.exit(4)


def rd(p):
    if not p.exists():
        fail("MISSING FILE " + str(p))
        return ""
    return p.read_text()


def outp(c):
    return W / (c + ".out")


def find(p, pat):
    m = re.search(pat, rd(p), re.M)
    if not m:
        fail("no match " + repr(pat) + " in " + p.name)
        return ""
    return m.group(1) if m.groups() else m.group(0)


def line_with(p, prefix):
    for l in rd(p).split("\n"):
        if l.strip().startswith(prefix):
            return l.strip()
    fail("no line starting " + repr(prefix) + " in " + p.name)
    return ""


TS = re.compile(r"^== (?:t3-census\.sh )?(?:started|finished)=\d{4}-\d{2}-\d{2}")


def norm(p):
    """transcript minus its two timestamp lines; the shed count is asserted, not assumed"""
    ls = rd(p).split("\n")
    kept = [l for l in ls if not TS.match(l)]
    if len(ls) - len(kept) != 2:
        fail("%s: timestamp filter shed %d lines, expected 2 — dead filter" % (p.name, len(ls) - len(kept)))
    return "\n".join(kept)


def cmp_of(sent_line):
    """same rendering rule the emitter uses, reimplemented so the two can disagree"""
    m = re.search(r"actual=(\S+)\s+(\S+?)(?:\s+(\S+))?\s*:\s+\w+\s*$", sent_line)
    if not m:
        return None
    a, op, e = m.groups()
    ab = lambda v: v if len(v) <= 14 else v[:12] + ELL
    return ab(a) + " " + op + ("" if e is None else " " + ab(e))


def claim(pat):
    """All values the paste asserts for a pattern; they must agree and one must exist.

    findall, not search: this file legitimately contains a *described* wrong figure, and an evidence
    file that states a value twice must have both statements checked.
    """
    vals = re.findall(pat, joined)
    if not vals:
        fail("no paste claim matching " + repr(pat))
        return None
    if len(set(vals)) > 1:
        fail("paste states %s more than once with different values: %s" % (pat, sorted(set(vals))))
        return None
    return vals[0] if not isinstance(vals[0], tuple) else vals[0]



# The § 3b rebuilt-program hashes are recomputed by this checker, not remembered: families I
# and A both consume REBUILT, so it is built once here from repo files alone. --expect-sha is
# pinned: an unpinned rebuild would happily rebase the whole table against a moved generator.
REBUILT = {}
for c in ORDER:
    o = REFIRE / (c + ".sh")
    r = subprocess.run([sys.executable, str(APPLIER), str(CTRL), str(GEN), str(o), c,
                        "--expect-sha=" + PIN], capture_output=True, text=True)
    if r.returncode != 0:
        fail("applier refused %s: %s" % (c, (r.stdout + r.stderr).strip()[:120]))
        continue
    REBUILT[c] = hashlib.sha256(o.read_bytes()).hexdigest()

# An absent corpus is not a pass: seven families would otherwise report a short green.
if not TREE_ONLY:
    _absent = [str(d) for d in (W, V, DEG) if not d.exists()]
    if _absent:
        print("the batch transcripts this mode needs are absent:", file=sys.stderr)
        for _a in _absent:
            print("  " + _a, file=sys.stderr)
        print("  run with --tree-only for the three families that need only repo bytes,",
              file=sys.stderr)
        print("  or point VP_BATCH_NEW / VP_BATCH_OLD / VP_DEGRADED at a saved batch",
              file=sys.stderr)
        sys.exit(2)


# ---------------------------------------------------------------- self-test
# Every family must be shown capable of firing. Each case is a deliberate, minimal corruption of a
# COPY of the paste (target "paste") or of the controls file (target "ctrl"); it passes only if the
# checker exits 1 AND its output names the family that owns that defect, so a case cannot pass by
# tripping an unrelated family. Then the clean files must exit 0. If any anchor below stops
# matching, the case reports that instead of quietly passing: a corruption that was not made is not
# a fired check.


def corrupt(text, op, args):
    ls = text.split("\n")
    if op == "sub":
        old, new = args
        if text.count(old) < 1:
            raise AssertionError("no occurrence of " + old[:44])
        return text.replace(old, new, 1)
    if op in ("line_sub", "line_append", "line_delete"):
        key = args[0]
        idx = [i for i, l in enumerate(ls) if key in l]
        if len(idx) != 1:
            raise AssertionError("anchor %r matches %d lines, needs 1" % (key[:44], len(idx)))
        if op == "line_delete":
            del ls[idx[0]]
        elif op == "line_sub":
            if args[1] not in ls[idx[0]]:
                raise AssertionError("anchor line lacks " + args[1][:44])
            ls[idx[0]] = ls[idx[0]].replace(args[1], args[2], 1)
        else:
            ls[idx[0]] = ls[idx[0]] + args[1]
        return "\n".join(ls)
    if op == "line_regex":
        rx = re.compile(args[0])
        idx = [i for i, l in enumerate(ls) if rx.search(l)]
        if len(idx) != 1:
            raise AssertionError("pattern matches %d lines, needs 1" % len(idx))
        ls[idx[0]] = rx.sub(args[1], ls[idx[0]], count=1)
        return "\n".join(ls)
    raise AssertionError("unknown op " + op)


PINHEX = PIN
VERDICT = "T3_CENSUS_VERDICT=FAIL"
LIVE_SHA = hashlib.sha256(CTRL.read_bytes()).hexdigest()
CASES = [
    # name, target, op, args, needle (a fragment of the owning family's BAD line)
    ("hex-tail-flip", "paste", "sub",
     ("3c62f607516e82e1", "3c62f607516e82e2"), "not printed or recomputed"),
    ("reorder-failed-list", "paste", "sub",
     ("FAILED: GENERATOR_SHA_IS_64_HEX GENERATOR_SHA_SAME_AS_REAL",
      "FAILED: GENERATOR_SHA_SAME_AS_REAL GENERATOR_SHA_IS_64_HEX"), "FAILED:"),
    ("repro-count", "paste", "sub",
     ("REPRODUCIBLE_STDOUT=10/10", "REPRODUCIBLE_STDOUT=9/10"), "stdout: paste="),
    ("filecount", "paste", "sub",
     ("232 in 11 directories", "209 in 11 directories"), "run-real counts"),
    ("invented-label", "paste", "sub",
     ("MANIFEST_ROWS_EQ_862", "MANIFEST_ROWS_EQ_999"), "partition block lists"),
    ("snapshot-sha", "paste", "sub",
     ("1a3baa984f36c3cc011a592a24f5d4fd064453820a7dae576d249697aeefaedf",
      "1a3baa984f36c3cc011a592a24f5d4fd064453820a7dae576d249697aeefade0"),
     "does not quote the snapshot"),
    ("census-35-to-36", "paste", "sub",
     ("(35 leading `chk`", "(36 leading `chk`"), "emission-sites line"),
    ("degraded-total", "paste", "sub",
     ("SENTINEL_TOTAL=23 SENTINEL_FAILS=2 FAILED: TREE_DIFF",
      "SENTINEL_TOTAL=28 SENTINEL_FAILS=2 FAILED: TREE_DIFF"), "not in the saved transcript"),
    ("prose-exits-0", "paste", "sub",
     ("It does not exit 0.", "It exits 0 in practice."), "exit"),
    ("panel-comparison", "paste", "sub",
     ("`34 ge 999`", "`34 ge 998`"), "panel: comparison"),
    ("panel-total", "paste", "sub", ("| 28/2 |", "| 29/2 |"), "panel: total"),
    ("unlabel-group", "paste", "sub",
     ("seen in `negctl` only      (5)", "                              "), "unlabelled"),
    ("refire-prog-sha", "paste", "line_sub",
     ("| C7 | `811575be64d1b244", "811575be64d1b244", "811575be64d1b245"),
     "not printed or recomputed"),
    ("refire-match-col", "paste", "line_sub",
     ("| C1 | `7be65", "**no \u2014 see note**", "yes"), "match-column"),
    ("refire-stdout-sha", "paste", "line_sub",
     ("| C5 | `51da43896c11c13a", "`ee89b2e76dc9a94c", "`ee89b2e76dc9a94d"),
     "not printed or recomputed"),
    ("k-kv-duplicate", "paste", "line_append",
     ("matching the pristine", ", so `SENTINEL_TOTAL=34` again"), "machine-format value"),
    ("k-digit-initial-key", "paste", "line_append",
     ("with `" + VERDICT + "`", " (`" + VERDICT + "` repeated here)"), "machine-format value"),
    ("k-hex-duplicate", "paste", "line_append",
     ("Base: `context/audit/scripts/t3-census.sh`", " (`%s`)" % PINHEX), "machine-format value"),
    ("L-rebuilt-tampered", "ctrl", "sub",
     ("REBUILT_SHA=811575be64d1b244d28f98f11191e4a6713f44dbce3e0b8d3d15443e2994d09c",
      "REBUILT_SHA=811575be64d1b244d28f98f11191e4a6713f44dbce3e0b8d3d15443e2994d09d"),
     "controls block REBUILT_SHA="),
    # needle is L's own line; the bare "states no REBUILT_SHA=" also matched the APPLIER's VACUOUS
    # refusal in the setup loop, so the case was scored before L ever ran
    ("L-key-removed", "ctrl", "line_delete", ("REBUILT_SHA=205259f6",),
     "block states no REBUILT_SHA="),
    ("L-key-truncated", "ctrl", "line_regex",
     (r"REBUILT_SHA=(cde74a45)[0-9a-f]{56}", r"REBUILT_SHA=\g<1>"), "truncated in blocks"),
    # added 2026-11-05 with the --tree-only split: the live-controls-hash check had no control of its
    # own (the ctrl-target cases trip it only incidentally, and are scored on another family's needle)
    ("live-controls-sha", "paste", "sub",
     (LIVE_SHA, LIVE_SHA[:-1] + ("0" if LIVE_SHA[-1] != "0" else "1")), "live controls hash"),
]

# --tree-only --self-test: only the cases whose owning check runs without the transcripts, each
# scored on the line THAT mode prints. Every case left out is a family --tree-only skips; it is
# not a pass, and the summary line says how many were not run.
TREE_NEEDLE = {
    "refire-prog-sha": "re-fire: rebuilt",
    "refire-match-col": "match column reads yes",
    "k-kv-duplicate": "machine-format value",
    "k-digit-initial-key": "machine-format value",
    "k-hex-duplicate": "machine-format value",
    "L-rebuilt-tampered": "controls block REBUILT_SHA=",
    "L-key-removed": "block states no REBUILT_SHA=",
    "L-key-truncated": "truncated in blocks",
    "live-controls-sha": "live controls hash",
}


def run_selftest():
    """The subprocesses are independent and each one rebuilds ten mutant programs, so the battery is
    run in parallel; results are printed in CASES order regardless of completion order.

    Every child re-executes this file, so a self-test reachable from a child is a fork bomb: 22
    children x 22 x ... (it OOMed the host on 2026-11-05, between 00:18 and 03:05 PST). Children
    carry CHILD_ENV and refuse to self-test even if the guard at the call site is ever lost again."""
    if os.environ.get(CHILD_ENV):
        print("refusing --self-test inside a self-test child (%s is set)" % CHILD_ENV,
              file=sys.stderr)
        return 2
    from concurrent.futures import ThreadPoolExecutor
    src = {"paste": PASTE.read_text(), "ctrl": CTRL.read_text()}
    tmp = pathlib.Path(tempfile.mkdtemp(prefix="t3-paste-check-selftest."))
    atexit.register(shutil.rmtree, tmp, ignore_errors=True)

    # Children run in the parent's mode: a tree-only parent with default-mode children would score
    # every case on an exit 2 from the absent-corpus refusal.
    child_argv = [sys.executable, __file__] + (["--tree-only"] if TREE_ONLY else [])
    if TREE_ONLY:
        cases = [(n, t, o, a, TREE_NEEDLE[n]) for (n, t, o, a, _) in CASES if n in TREE_NEEDLE]
        if len(cases) != len(TREE_NEEDLE):
            print("TREE_NEEDLE names a case CASES does not have", file=sys.stderr)
            return 1
    else:
        cases = CASES

    def one(case):
        name, target, op, args, needle = case
        try:
            body = corrupt(src[target], op, args)
        except AssertionError as e:
            return (name, None, "*** CORRUPTION NOT MADE: %s ***" % e, "", False)
        if body == src[target]:
            return (name, None, "*** CORRUPTION CHANGED NOTHING ***", "", False)
        p = tmp / (name + "." + target + ".txt")
        p.write_text(body)
        env = dict(os.environ, **{CHILD_ENV: "1"})
        env[{"paste": "VP_PASTE", "ctrl": "VP_CONTROLS"}[target]] = str(p)
        r = subprocess.run(child_argv, capture_output=True, text=True, env=env)
        # Score on FAILURE lines only. Matching any output line let K's three cases "fire" on K's
        # section header, which prints on every run -- a check that passed by construction
        # (found 2026-11-05, the first time this battery ran to completion).
        named = [l.strip() for l in (r.stdout + r.stderr).split("\n")
                 if needle in l and l.strip().startswith(("BAD ", "CANNOT CONTINUE"))]
        hit = r.returncode == 1 and bool(named)
        return (name, r.returncode, "FIRED" if hit else "*** MISSED ***",
                (named[0][:86] if named else ""), hit)

    print()
    print("=" * 78)
    print("SELF-TEST%s: %d corruptions, each must exit 1 and name its own family"
          % (" (--tree-only)" if TREE_ONLY else "", len(cases)))
    print("=" * 78)
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(one, cases))
    fired = 0
    for name, rc, verdict, detail, hit in results:
        fired += 1 if hit else 0
        print("  %-24s exit=%-4s %-6s %s" % (name, rc, verdict, detail))
    r = subprocess.run(child_argv, capture_output=True, text=True,
                       env=dict(os.environ, **{CHILD_ENV: "1"}))
    clean = r.returncode == 0
    print("  %-24s exit=%-4s %s" % ("CLEAN FILE", r.returncode, "OK" if clean else "*** NOT CLEAN ***"))
    print("\n  fired %d/%d corruptions; clean file %s"
          % (fired, len(cases), "clean" if clean else "NOT CLEAN"))
    if len(cases) != len(CASES):
        print("  NOT RUN: %d of %d cases own a family --tree-only skips; that is not a pass"
              % (len(CASES) - len(cases), len(CASES)))
    return 0 if (fired == len(cases) and clean) else 1


if SELFTEST:
    sys.exit(run_selftest())
if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- corpus
    CORPUS = set()
    for d in (W, V, DEG):
        if not d.exists():
            continue
        for f in sorted(d.iterdir()):
            if f.is_file():
                for l in f.read_text().split("\n"):
                    CORPUS.add(l.strip())
    if V_C2.exists():
        for l in V_C2.read_text().split("\n"):
            CORPUS.add(l.strip())
    print("=" * 78)
    print("0. corpus: %d distinct lines from %s + %s + %s + run-real tokens" % (len(CORPUS), W, V, DEG))
    print("=" * 78)
    if len(CORPUS) < 500:
        fail("corpus suspiciously small — the scratch batch may be gone; nothing below is meaningful")

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- A. hashes
    print()
    print("=" * 78)
    print("A. every sha-shaped token in the paste: is it a value some file printed or recomputes?")
    print("=" * 78)
    auth = set()
    for d in (W, V, DEG):
        if d.exists():
            for f in sorted(d.iterdir()):
                if f.is_file():
                    auth |= set(re.findall(r"[0-9a-f]{64}", f.read_text()))
    auth.add(PIN)
    auth.add("6d0c70b8f6bd1c5c1e1496903c9e4ced2e3ddf70631f858c6e3d4b7c33462c23")   # EXPECTED_UNIVERSE_SHA
    auth.add("b209ba002cc8e87a9494ce345150f123294fb15d243838381def8d1df0d75a4e")   # restore digest
    auth.add("1a3baa984f36c3cc011a592a24f5d4fd064453820a7dae576d249697aeefaedf")   # controls snapshot
    if SNAPF.exists():
        auth.add(hashlib.sha256(SNAPF.read_bytes()).hexdigest())
    auth.add(hashlib.sha256(CTRL.read_bytes()).hexdigest())
    auth.add(hashlib.sha256(GEN.read_text().replace(NS, "/tmp/opencode/degraded/ns").encode()).hexdigest())
    for f in sorted((T3 / "run-real").glob("*.sha256")):
        auth.add(f.read_text().strip())
    GENLINES = rd(GEN)
    auth |= set(re.findall(r"[0-9a-f]{64}", GENLINES))
    # the § 3b rebuilt-program hashes are computed by the checker itself in section I; allow them here
    # only after they have been recomputed, so populate from a fresh rebuild
    for c in ORDER:
        auth.add(REBUILT[c])
        auth.add(hashlib.sha256(norm(outp(c)).encode()).hexdigest())
    print("  authoritative 64-hex values known to files or recomputed: %d" % len(auth))
    checked = 0
    for i, ln in enumerate(lines, 1):
        for tok in re.findall(r"\b[0-9a-f]{64}\b", ln):
            checked += 1
            if tok not in auth:
                fail(":%d 64-hex token not printed or recomputed by any file: %s" % (i, tok))
    for i, ln in enumerate(lines, 1):
        for tok in re.findall(r"\b[0-9a-f]{9,63}\b", ln):
            if len(tok) == 40 and re.fullmatch(r"dc047a77[0-9a-f]{32}|3f7b69ae[0-9a-f]{32}", tok):
                continue
            if len(tok) not in (8, 12, 14, 16, 20):
                fail(":%d odd-length hex token (%d): %s" % (i, len(tok), tok))
    print("  checked %d full 64-hex tokens for membership; plus a length screen on the rest" % checked)
    if checked < 22:
        fail("only %d full-length hashes in the paste — expected >= 22 (10 printed + 10 rebuilt + pins)"
             % checked)
    for c in ORDER:
        r_ = find(outp(c), r"^GENERATOR_SHA256=([0-9a-f]+)")
        hits = [l for l in lines if l.strip().startswith("GENERATOR_SHA256=" + r_)]
        if not hits:
            fail("%s: its own run's sha %s… does not appear in the paste" % (c, r_[:16]))
        else:
            ok("%s printed sha %s… present (%d occurrence(s))" % (c, r_[:16], len(hits)))

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- B. verbatim
    print()
    print("=" * 78)
    print("B. every indented transcript line: does a file contain it EXACTLY?")
    print("=" * 78)
    BARE_LABELS = []


    def is_composed(s):
        """lines the emitter computes rather than copies — each family is checked by name elsewhere"""
        if s.startswith(("$", "mutate:", "# state 1", "# state 2")):
            return True
        if IN_FENCE:
            return True          # a ``` fence is a command to run, not raw output
        fixed = ("exit=", "PARENT-EXPECTED-LEG-OK=", "REPRODUCIBLE_", "files in run-real",
                 "path-relative digest", "diff -r backup", "generator token after",
                 "source emission sites", "distinct labels", "labels in", "partition",
                 "seen in `real` only", "seen in `negctl` only", "in both", "after the floor commit",
                 "`real`,", "`negctl`", "`degraded negctl`", "controls file the batch read",
                 "controls file at generation time", "the ten `mutate:` pairs", "giving `")
        if s.startswith(fixed):
            return True
        if re.fullmatch(r"[A-Z][A-Z0-9_]{6,}(?:  +[A-Z][A-Z0-9_]{6,})*", s):
            BARE_LABELS.extend(s.split())
            return True
        if re.fullmatch(r"[A-Z][A-Z0-9_]{6,}", s):
            BARE_LABELS.append(s)
            return True
        return False


    n_v = n_c = 0
    IN_FENCE = False
    for i, ln in enumerate(lines, 1):
        if ln.startswith("```"):
            IN_FENCE = not IN_FENCE
            continue
        if not ln.startswith("    ") or not ln.strip():
            continue
        s = ln.strip()
        if is_composed(s):
            n_c += 1
            continue
        if s in CORPUS:
            n_v += 1
        elif re.match(r"^C\d+ {1,3}\S", s) and re.sub(r"^C\d+ {1,3}", "", s) in CORPUS:
            n_v += 1          # a control id glued in front of a verbatim line (§ 2 parentage rows)
        else:
            fail(":%d line is not a verbatim line of any saved output:\n       >>> %s" % (i, s[:120]))
    print("  verbatim-matched %d composed/whitelisted %d" % (n_v, n_c))
    print("  bare labels composed into listings: %d (checked in F)" % len(BARE_LABELS))
    if n_v < 40:
        fail("only %d transcript lines matched — expected one per control plus parentage, clean and "
             "degraded legs" % n_v)

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- C. panel
    print()
    print("=" * 78)
    print("C. the panel: each cell against the control's own files, including the comparison column")
    print("=" * 78)
    panel = {}
    for ln in lines:
        m = re.match(r"^\| (C\d+) \| `(\w+)` \| `(\w+)` \| `(.+)` \| (\d+)/(\d+) \|$", ln)
        if m:
            panel[m.group(1)] = m.groups()
    if len(panel) != 10:
        fail("panel table has %d control rows, expected 10" % len(panel))
    for c in ORDER:
        if c not in panel:
            continue
        _, tag, asserted_target, asserted_cmp, total, fails_ = panel[c]
        want_tag = rd(W / (c + ".tag")).strip()
        want_total = find(outp(c), r"^SENTINEL_TOTAL=(\d+)")
        want_fails = find(outp(c), r"^SENTINEL_TOTAL=\d+ SENTINEL_FAILS=(\d+)")
        sent = line_with(outp(c), "SENT " + TARGETS[c])
        errs = []
        if asserted_target != TARGETS[c]:
            errs.append("assertion %s != %s" % (asserted_target, TARGETS[c]))
        if tag != want_tag:
            errs.append("tag %s!=%s" % (tag, want_tag))
        if total != want_total:
            errs.append("total %s!=%s" % (total, want_total))
        if fails_ != want_fails:
            errs.append("fails %s!=%s" % (fails_, want_fails))
        got_cmp = cmp_of(sent)
        if got_cmp is None:
            errs.append("cannot parse the run's own comparison")
        elif got_cmp != asserted_cmp:
            errs.append("comparison %r != run's %r" % (asserted_cmp, got_cmp))
        if " : FAIL" not in sent:
            errs.append("the run's target SENT line does not read FAIL")
        if errs:
            fail(c + " panel: " + "; ".join(errs))
        else:
            ok("%s panel row = files (tag=%s `%s` %s/%s)" % (c, tag, asserted_cmp, total, fails_))
        f_ = ""
        for l_ in rd(outp(c)).split("\n"):
            if " FAILED: " in l_:
                f_ = l_.strip()
        if not f_:
            fail(c + ": no footer line containing ' FAILED: ' in its output")
        elif TARGETS[c] not in f_:
            fail(c + ": FAILED: line does not name the target assertion: " + f_[:100])
        if rd(W / (c + ".exit")).strip() != "1":
            fail(c + ": the paste's preamble claims every control exited 1; file says "
                 + rd(W / (c + ".exit")).strip())
        if len(rd(W / (c + ".err"))) != 0:
            fail(c + ": the preamble claims 0 bytes of stderr; file says %d" % len(rd(W / (c + ".err"))))
        if find(outp(c), r"^T3_CENSUS_VERDICT=(\w+)") != "FAIL":
            fail(c + ": the preamble claims verdict FAIL for all ten")

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- D. parentage
    print()
    print("=" * 78)
    print("D. parentage table: the 9/9 claim against the nine negctl runs")
    print("=" * 78)
    n_ok = 0
    for c in NEGCTL:
        l = line_with(outp(c), "SENT GENERATOR_SHA_SAME_AS_REAL")
        rows = [x for x in lines if x.strip().startswith(c + " ") and "GENERATOR_SHA_SAME_AS_REAL" in x]
        if not rows:
            fail(c + ": no parentage row in the paste")
        elif not any(l in x for x in rows):
            fail(c + ": parentage row is not its own line verbatim\n       paste: %s\n       run:   %s"
                 % (rows[0].strip()[:110], l[:110]))
        if re.search(r"eq\s+" + PIN[:8], l):
            n_ok += 1
    print("  measured %d/9 with a %s… expected leg" % (n_ok, PIN[:8]))
    pc = claim(r"PARENT-EXPECTED-LEG-OK=(\d+)/9")
    if pc is not None and pc != str(n_ok):
        fail("paste claims %s/9, measured %d/9" % (pc, n_ok))
    else:
        if pc is not None:
            ok("claim %s/9 matches" % pc)

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- E. clean run
    print()
    print("=" * 78)
    print("E. the clean run: footer, stderr, and the name list")
    print("=" * 78)
    co = rd(W / "CLEAN.out")
    names_real = sorted(set(re.findall(r"^SENT ([A-Z0-9_]+)", co, re.M)))
    print("  clean run prints %d names" % len(names_real))
    start = next((i for i, l in enumerate(lines) if "names, sorted" in l), None)
    if start is None:
        fail("no name-list section found")
    else:
        body = []
        for l in lines[start + 1:]:
            if l.startswith(">") or l.startswith("## "):
                break
            body.append(l)
        names_paste = sorted(set(re.findall(r"\b[A-Z][A-Z0-9_]{6,}\b", "\n".join(body))))
        print("  paste name list: %d names" % len(names_paste))
        missing = sorted(set(names_real) - set(names_paste))
        extra = sorted(set(names_paste) - set(names_real))
        if missing:
            fail("missing from the paste: %s" % missing)
        if extra:
            fail("in the paste but not in the run (invented?): %s" % extra)
        if not missing and not extra:
            ok("name sets identical")
    for pat, why in [(r"SENTINEL_TOTAL=28 SENTINEL_FAILS=0", "clean footer"),
                     (r"T3_CENSUS_VERDICT=PASS_PENDING_CANARY", "clean verdict")]:
        in_paste = bool(re.search(pat, joined))
        in_run = bool(re.search(pat, co, re.M))
        if in_paste != in_run:
            fail("%s: paste=%s run=%s" % (why, in_paste, in_run))
        else:
            ok("%s agrees (paste=%s run=%s)" % (why, in_paste, in_run))
    nine_identical = 0
    for c in NEGCTL:
        ns = sorted(set(re.findall(r"^SENT ([A-Z0-9_]+)", rd(outp(c)), re.M)))
        if ns == names_real:
            nine_identical += 1
    if nine_identical != 9:
        fail("the nine negctl runs were not all 28-name-identical to the clean run: %d/9" % nine_identical)
    else:
        ok("identical name sets across the nine negctl runs: 9/9")

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- F. census
    print()
    print("=" * 78)
    print("F. the producer census: recomputed from the generator, compared to the paste")
    print("=" * 78)
    src = GENLINES.split("\n")
    leading = [i for i, l in enumerate(src, 1) if re.match(r"^\s*(chk|runpy) [A-Z0-9_]+", l)]
    inline = [i for i, l in enumerate(src, 1)
              if re.search(r"\|\|\s+chk\s+[A-Z0-9_]+", l) and i not in leading]
    labels = [re.search(r"(chk|runpy)\s+([A-Z0-9_]+)", src[i - 1]).group(2) for i in leading + inline]
    runpy_n = sum(1 for i in leading if re.search(r"\brunpy\b", src[i - 1]))
    chk_lead_n = len(leading) - runpy_n
    real_names = sorted(set(re.findall(r"^SENT ([A-Z0-9_]+)", rd(REAL), re.M)))
    seen = sorted(set(real_names) | set(names_real))
    unseen = sorted(set(labels) - set(seen))
    C = {"sites": len(leading) + len(inline), "labels": len(set(labels)),
         "real": len(real_names), "neg": len(names_real), "seen": len(seen),
         "real_only": sorted(set(real_names) - set(names_real)),
         "neg_only": sorted(set(names_real) - set(real_names)),
         "chk_lead": chk_lead_n, "inline": len(inline), "runpy": runpy_n}
    C["base"] = C["neg"] - len(C["neg_only"])
    print("  recomputed: sites=%d labels=%d real=%d neg=%d seen=%d real_only=%d neg_only=%d base=%d "
          "(chk_lead=%d inline=%d runpy=%d)" % (C["sites"], C["labels"], C["real"], C["neg"], C["seen"],
                                                len(C["real_only"]), len(C["neg_only"]), C["base"],
                                                chk_lead_n, len(inline), runpy_n))
    if chk_lead_n + len(inline) + runpy_n != C["sites"]:
        fail("buckets are not disjoint: %d+%d+%d != %d" % (chk_lead_n, len(inline), runpy_n, C["sites"]))
    if unseen != ["MANIFEST_COL6_RESIDUE_EQ_BINARIES"]:
        fail("the single never-printed label changed: %s" % unseen)
    m = re.search(r"source emission sites\s+(\d+)\s+\((\d+) leading `chk` \+ (\d+) inline.*?(\d+) `runpy`",
                  joined)
    if not m:
        fail("no parseable emission-sites line")
    elif [int(x) for x in m.groups()] != [C["sites"], C["chk_lead"], C["inline"], C["runpy"]]:
        fail("emission-sites line %s != recomputed %s"
             % (m.groups(), (C["sites"], C["chk_lead"], C["inline"], C["runpy"])))
    else:
        ok("emission sites %s = %s+%s+%s" % m.groups())
    for k, v in [("distinct labels", C["labels"]), ("labels in some run", C["seen"])]:
        mm = re.search(re.escape(k) + r"\s+(\d+)", joined)
        if not mm:
            fail("no paste line for %r" % k)
        elif int(mm.group(1)) != v:
            fail("%s: paste=%s recomputed=%s" % (k, mm.group(1), v))
        else:
            ok("%s = %d" % (k, v))
    known = set(labels) | set(real_names) | set(names_real)
    invented = sorted(set(BARE_LABELS) - known)
    if invented:
        fail("composed label listings contain names that are neither emitted nor printed: %s" % invented)
    else:
        ok("all %d composed labels are real labels or real printed names" % len(set(BARE_LABELS)))


    def lastint(s):
        return int(re.findall(r"\d+", s)[-1])


    def int_after_eq(line):
        """the asserted figure = the integer right after the line's LAST '='.

        Last, not first: `= 23 + 12 = 35   (parenthetical)` asserts 35, and `= 23   measured` asserts 23.
        An earlier version required the integer at end-of-line, which made two honest lines with trailing
        annotations unparseable and silently skipped them.
        """
        mm = re.match(r"\s*(\d+)", line.rsplit("=", 1)[-1]) if "=" in line else None
        return int(mm.group(1)) if mm else None


    def assert_line(substr, want, why, cands=None):
        cs = [l for l in lines if substr in l and int_after_eq(l) is not None]
        if cands is not None:
            cs = cands
        if not cs:
            fail("no parseable paste line containing %r" % substr)
            return
        got = {int_after_eq(l) for l in cs}
        if want not in got:
            fail("%s: paste=%s recomputed=%s  [%s]" % (why, sorted(got), want, cs[0].strip()[:70]))
        else:
            ok("%-22s = %d" % (why, want))


    assert_line("`real`, pre-fill", C["base"] + len(C["real_only"]), "real pre-fill")
    assert_line("`negctl`            =", C["base"] + len(C["neg_only"]), "negctl")
    assert_line("`real`, POST-fill", C["base"] + len(C["real_only"]) + 1, "real POST-fill")
    assert_line("after the floor commit, `negctl`", C["base"] + 1 + len(C["neg_only"]), "post-floor negctl")
    # the degraded figure must equal the measurement, not an arithmetic derivation
    d_lines = [l for l in lines if "`degraded negctl`" in l]
    if not d_lines:
        fail("no degraded-total line in § 6")
    else:
        dt = lastint(d_lines[0])
        meas = find(DEG / "degraded.out", r"^SENTINEL_TOTAL=(\d+)")
        if meas and dt != int(meas):
            fail("§ 6 degraded total %d != measured %s" % (dt, meas))
        else:
            ok("%-22s = %d (measured)" % ("degraded negctl", dt))
    for name in unseen:
        if name not in joined:
            fail("the by-absence label %s is not named in the paste" % name)
    ok("by-absence label named: %s" % ", ".join(unseen))
    # the partition listing must be exactly the two set-differences
    p_start = next((i for i, l in enumerate(lines) if l.strip().startswith("seen in `real` only")), None)
    if p_start is None:
        fail("no partition block found")
    else:
        block = [l.strip() for l in lines[p_start:p_start + 12]]
        listed = sorted(set(w for l in block for w in re.findall(r"\b[A-Z][A-Z0-9_]{6,}\b", l)))
        want_p = sorted(C["real_only"] + C["neg_only"])
        if listed != want_p:
            fail("partition block lists %s, recomputed %s" % (listed, want_p))
        else:
            ok("partition block = the two set-differences exactly (%d names)" % len(listed))
        # hoodie's finding: the negctl-only group must be labelled, not a bare run-on list
        if not any(l.strip().startswith("seen in `negctl` only") for l in lines):
            fail("the negctl-only group is unlabelled")
        else:
            ok("both partition groups are labelled")

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- G. reproducibility
    print()
    print("=" * 78)
    print("G. two-batch reproducibility, recomputed")
    print("=" * 78)
    p_ok = o_ok = 0
    for c in ORDER:
        v1 = V_C2 if c == "C2" else V / (c + ".out")
        if not v1.exists():
            fail("earlier batch's %s is gone — reproducibility is unverifiable" % c)
            continue
        if find(v1, r"^GENERATOR_SHA256=([0-9a-f]+)") == find(outp(c), r"^GENERATOR_SHA256=([0-9a-f]+)"):
            p_ok += 1
        if norm(v1) == norm(outp(c)):
            o_ok += 1
    for pat, val, why in [(r"REPRODUCIBLE_PROGRAMS=(\d+)/10", p_ok, "programs"),
                          (r"REPRODUCIBLE_STDOUT=(\d+)/10", o_ok, "stdout")]:
        got = claim(pat)
        if got is None:
            pass
        elif int(got) != val:
            fail("%s: paste=%s recomputed=%s" % (why, got, val))
        else:
            ok("%s reproducible %d/10" % (why, val))

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- H. degraded states
    print()
    print("=" * 78)
    print("H. the degraded-state blocks, against the saved model transcripts")
    print("=" * 78)
    for which, label in [("degraded", "state 1"), ("partial", "state 2")]:
        f = DEG / (which + ".out")
        if not f.exists():
            fail("degraded transcript %s absent — § 7 cannot be checked" % f)
            continue
        body = []
        i0 = next((i for i, l in enumerate(lines) if l.strip().startswith("# " + label)), None)
        if i0 is None:
            fail("no %r block in the paste" % ("# " + label))
            continue
        for l in lines[i0 + 1:]:
            if not l.startswith("    "):
                break
            body.append(l.strip())
        if not body:
            fail("%s block is empty" % label)
            continue
        missing = [b for b in body if b not in CORPUS]
        if missing:
            fail("%s: %d line(s) not in the saved transcript: %s" % (label, len(missing), missing[0][:90]))
        else:
            ok("%s: %d lines all verbatim (total=%s fails=%s verdict=%s)"
               % (label, len(body), find(f, r"^SENTINEL_TOTAL=(\d+)"),
                  find(f, r"^SENTINEL_TOTAL=\d+ SENTINEL_FAILS=(\d+)"),
                  find(f, r"^T3_CENSUS_VERDICT=(\w+)")))
        if "exit 0" in "\n".join(lines[i0:i0 + 14]).lower():
            fail("%s block still claims a degraded run exits 0" % label)
    # An evidence file may quote a claim it then refutes, but the quote must be *marked* as one and
    # refuted adjacent to it: otherwise a later grep cannot tell the refuted claim from an asserted one.
    ex_lines = [(i, l) for i, l in enumerate(lines) if re.search(r"exits 0\b", l)]
    bad_ex = []
    for i, l in ex_lines:
        quoted = "\u201c" in l
        refuted = i + 1 < len(lines) and "does not exit 0" in lines[i + 1]
        if not (quoted and refuted):
            bad_ex.append(":%d %s" % (i + 1, l.strip()[:80]))
    if bad_ex:
        fail("'exits 0' appears without quotation + adjacent refutation:\n       " + "\n       ".join(bad_ex))
    else:
        ok("%d 'exits 0' mention(s), each quoted and refuted on the next line" % len(ex_lines))
    if re.search(r"SENTINEL_TOTAL=23 SENTINEL_FAILS=0", joined):
        fail("the paste asserts a clean footer for a degraded run (FAILS=0 with TOTAL=23)")
    else:
        ok("no clean footer asserted for a degraded run")

# ---------------------------------------------------------------- I. re-fire + provenance
print()
print("=" * 78)
print("I. the re-fire table: rebuilt programs and normalized-stdout digests, recomputed here")
print("=" * 78)
rows = {}
for ln in lines:
    m = re.match(r"^\| (C\d+) \| `([0-9a-f]{64})` \| (.+?) \| `([0-9a-f]{64})` \|$", ln)
    if m:
        rows[m.group(1)] = m.groups()
if len(rows) != 10:
    fail("re-fire table has %d rows, expected 10" % len(rows))
for c in ORDER:
    if c not in rows or c not in REBUILT:
        continue
    _, prog, says_yes, stdout_sha = rows[c]
    errs = []
    if prog != REBUILT[c]:
        errs.append("rebuilt %s… != recomputed %s…" % (prog[:12], REBUILT[c][:12]))
    # The stdout digest and the printed identity are read out of the batch transcripts: they are
    # not repo bytes, so --tree-only must not touch them (it used to, and printed 20 MISSING FILE
    # lines on a machine whose /tmp had been wiped).
    if TREE_ONLY:
        if errs:
            fail(c + " re-fire: " + "; ".join(errs))
        else:
            ok("%s re-fire row's program hash reproduces from the tree (prog=%s; stdout digest and "
               "match column need the transcripts)" % (c, prog[:12]))
        continue
    want_stdout = hashlib.sha256(norm(outp(c)).encode()).hexdigest()
    if stdout_sha != want_stdout:
        errs.append("stdout digest != recomputed")
    printed = find(outp(c), r"^GENERATOR_SHA256=([0-9a-f]+)")
    expect_yes = "yes" if printed == REBUILT[c] else "**no \u2014 see note**"
    if says_yes != expect_yes:
        errs.append("match-column says %r, expected %r (printed identity %s…)"
                    % (says_yes, expect_yes, printed[:12]))
    if errs:
        fail(c + " re-fire: " + "; ".join(errs))
    else:
        ok("%s re-fire row reproduces from the tree (prog=%s… stdout=%s…)"
           % (c, prog[:12], stdout_sha[:12]))
if "yes" not in [rows.get(c, ("", "", "", ""))[2] for c in ORDER[:1]]:
    pass
n_yes = sum(1 for v in rows.values() if v[2] == "yes")
if n_yes != 9:
    fail("re-fire match column reads yes on %d rows; expected 9 (C1's print is the poison)" % n_yes)
elif TREE_ONLY:
    ok("the paste's match column reads yes on 9/10 rows, C1 the stated exception -- counted only; "
       "comparing it to each printed identity needs the transcripts")
else:
    ok("9/10 rebuilt hashes equal the printed identity; C1 is the stated exception")
# the recipe text: snapshot vs live, and the paste's own mutate: lines
print()
print("  --- recipe provenance: the paste quotes the LIVE controls file; the batch read a SNAPSHOT ---")


def pairs_of(text):
    blocks, cur, name = {}, [], None
    for line in text.split("\n"):
        m = re.match(r"^### (C\d+)\b", line)
        if m:
            if name:
                blocks[name] = "\n".join(cur)
            name, cur = m.group(1), []
            continue
        if name:
            cur.append(line)
    if name:
        blocks[name] = "\n".join(cur)
    got = {}
    for c, body in blocks.items():
        seg = re.search(r"^[ \t]*mutate:[ \t]*(.*?)^[ \t]*GENERATOR_SHA=([0-9a-f]+)", body, re.S | re.M)
        if not seg:
            continue
        phys = [re.sub(r"^[ \t]+", "", l).rstrip() for l in seg.group(1).strip("\n").split("\n")]
        o, n = "\n".join(phys).split("\u2192", 1)
        got[c] = (o.strip(), n.strip())
    return got


# The snapshot lives under /tmp with the transcripts, so only the full mode reads it. The live
# controls file is repo bytes, and its checks below run in both modes.
_snap_ok = True
if TREE_ONLY:
    print("  (snapshot-vs-live comparison skipped in --tree-only: the snapshot is not repo bytes)")
elif not SNAPF.exists():
    fail("snapshot absent: cannot check that the paste's recipes are the ones that ran")
    _snap_ok = False
else:
    snap_sha = hashlib.sha256(SNAPF.read_bytes()).hexdigest()
    ps = pairs_of(SNAPF.read_text())
    if len(ps) != 10:
        fail("parsed %d snapshot recipe pairs, expected 10" % len(ps))
    if snap_sha not in joined:
        fail("the paste does not quote the snapshot controls hash %s" % snap_sha[:12])
    else:
        ok("paste quotes the snapshot controls hash: %s" % snap_sha[:12])
if _snap_ok:
    live_sha = hashlib.sha256(CTRL.read_bytes()).hexdigest()
    if live_sha not in joined:
        fail("the paste does not quote the live controls hash %s" % live_sha[:12])
    else:
        ok("paste quotes the live controls hash: %s" % live_sha[:12])
    pl = pairs_of(CTRL.read_text())
    if len(pl) != 10:
        fail("parsed %d live recipe pairs, expected 10" % len(pl))
    for c in ORDER:
        if c not in pl:
            fail("live controls file has no mutate: pair for " + c)
            continue
        sides = []
        for side in pl[c]:
            if ELL in side:
                fail("C-recipe in the LIVE controls file still contains an ellipsis (%s) — the tree "
                     "must state complete fixtures" % c)
            sides.append(side)
        rows_ = [l for l in lines if l.strip().startswith("mutate:")]
        if not [l for l in rows_ if all(s in l for s in sides)]:
            fail(c + ": the mutate: line in the paste is not the pair from the controls file")
    ok("every paste mutate: line equals the controls-file pair, with no ellipsis left (10 blocks)")
    if re.search(r"^\s*→.*" + ELL, rd(CTRL), re.M):
        fail("an arrow-ellipsis recipe survives in the controls file")
    else:
        ok("no truncated mutate: right-hand side survives in the controls file")
# the applier in the tree must be the thing the paste tells readers to run
if "context/audit/scripts/t3-make-mutant.py" not in joined:
    fail("the paste does not name the in-tree applier it depends on")
elif not APPLIER.exists():
    fail("the named applier is not in the tree: " + str(APPLIER))
else:
    ok("the paste names an applier that is in the tree: " + APPLIER.name)

if TREE_ONLY:
    print("\n  (skipped in --tree-only: this family reads the batch transcripts)")
else:
    # ---------------------------------------------------------------- J. restore / tree state
    print()
    print("=" * 78)
    print("J. the restore claim, and the state of the tree and the artifacts")
    print("=" * 78)
    b = rd(W / "runreal.digest.before").strip()
    a = rd(W / "runreal.digest.after").strip()
    if b != a or not b:
        fail("restore digests differ or empty: %s vs %s" % (b[:16], a[:16]))
    else:
        ok("restore digests equal and non-empty: " + b[:20] + "...")
    if b not in joined:
        fail("the restore digest the paste quotes is not the one on disk")
    else:
        ok("paste quotes the on-disk restore digest")
    if len(rd(W / "restore.diff")) != 0:
        fail("restore.diff is not empty: %d bytes" % len(rd(W / "restore.diff")))
    else:
        ok("restore.diff is 0 bytes")
    f_real = sum(1 for _ in (T3 / "run-real").rglob("*") if _.is_file())
    d_real = 1 + sum(1 for _ in (T3 / "run-real").rglob("*") if _.is_dir())
    top_real = sum(1 for _ in (T3 / "run-real").iterdir())
    m = re.search(r"files in run-real\s+(\d+) in (\d+) directories \((\d+) top-level", joined)
    if not m:
        fail("no run-real count line")
    elif [int(x) for x in m.groups()] != [f_real, d_real, top_real]:
        fail("run-real counts: paste=%s disk=%s" % (m.groups(), (f_real, d_real, top_real)))
    else:
        ok("run-real counts match disk: %d files / %d dirs / %d top-level" % (f_real, d_real, top_real))
    tok = (T3 / "run-real/generator.sha256").read_text().strip()
    if tok != PIN:
        fail("run-real/generator.sha256 is NOT the pristine token: " + tok[:16])
    else:
        ok("run-real/generator.sha256 is pristine: " + tok[:16] + "...")
    if hashlib.sha256(GEN.read_bytes()).hexdigest() != PIN:
        fail("worktree generator is not the frozen cut")
    else:
        ok("worktree generator == frozen cut " + PIN[:12] + "...")
    left = list((ROOT / "context/audit/scripts").glob(".neg-*"))
    if left:
        fail("mutant copies left in the tree: %s" % [str(p) for p in left])
    else:
        ok("no .neg-* residue in context/audit/scripts")
    C9a = line_with(V / "C9.out", "SENT PREDICATE_SHA_SAME_AS_REAL")
    C9b = line_with(outp("C9"), "SENT PREDICATE_SHA_SAME_AS_REAL")
    says = "identical" if C9a and C9a == C9b else "NOT identical"
    if ("**%s**" % says) not in joined:
        fail("C9 note does not say %r" % says)
    else:
        ok("C9 note says %r, and that is what the files say" % says)

# ---------------------------------------------------------------- K
# Added 2026-11-05 after the round-6 reviews. K enforces, mechanically, the convention this paste's own
# header states; L binds the controls file's hand-placed REBUILT_SHA= keys to this paste's
# emitter-computed re-fire table.
#
# Coordinates (`:1448`) are deliberately OUT of K's scope. Measured on the finished file: 10 of 20
# recur, legitimately, because § 7's mechanism table names a site that the bullets below it then
# discuss. A uniqueness rule the honest file violates is not a rule but a corruption generator, which
# is exactly the defect in the header's first draft that hoodie caught (it invited de-duplicating a
# transcript). K therefore covers KEY=value and 64-hex only, outside raw blocks and fences.
print()
print("=" * 78)
print("K. machine-format values outside transcript blocks: one apiece")
print("=" * 78)


def line_kinds(ls):
    """Classify each line. A ``` fence inside a `> ` commentary block counts as a fence."""
    out, inside = [], False
    for l in ls:
        bare = re.sub(r"^(?:>[ \t]+)*", "", l)
        if bare.startswith("```"):
            out.append(("fence", inside))
            inside = not inside
            continue
        if inside:
            out.append(("fencebody", True))
        elif l.startswith("    "):
            out.append(("raw", False))
        elif l.startswith("|"):
            out.append(("table", False))
        elif l.startswith(">"):
            out.append(("commentary", False))
        else:
            out.append(("prose", False))
    return out


KINDS = line_kinds(lines)
SCOPE = [i for i, (k, _) in enumerate(KINDS) if k in ("prose", "commentary", "table")]
# Digit-initial keys are the common case in this corpus (T3_CENSUS_VERDICT, T3_ROOT): a {3,}
# floor on the first segment cannot see them, which is how K stayed green over two asserted
# occurrences of the verdict line. Measured 2026-11-05: widening adds LC_ALL=C and the verdict
# key, and both are now stated once outside transcript blocks.
KV_RE = re.compile(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+=[0-9A-Za-z_][0-9A-Za-z_./+-]*")
HEX_RE = re.compile(r"\b[0-9a-f]{64}\b")

seen = {}
for i in SCOPE:
    for tok in KV_RE.findall(lines[i]) + HEX_RE.findall(lines[i]):
        seen.setdefault(tok, []).append(i + 1)

# A family that classifies nothing has checked nothing: refuse to be vacuously green.
if len(seen) < 15:
    fail("K is near-vacuous: only %d machine-format values found outside transcript blocks, so the "
         "line classifier or the paste's layout broke" % len(seen))
dupes = {t: w for t, w in seen.items() if len(w) > 1}
for t, where in sorted(dupes.items(), key=lambda x: -len(x[1])):
    fail("machine-format value %r stated %d times outside transcript blocks (lines %s) \u2014 the header "
         "convention allows one asserted occurrence; if one of these is a refuted claim, quote it with "
         "typographic marks instead of restating it" % (t, len(where), where))
if not dupes and len(seen) >= 15:
    ok("%d distinct machine-format values outside transcript blocks, none stated twice" % len(seen))
counts = collections.Counter(k for k, _ in KINDS)
ok("line census: " + " ".join("%s=%d" % (k, counts[k]) for k in
   ("prose", "commentary", "table", "raw", "fence", "fencebody") if counts[k]))

# ---------------------------------------------------------------- L
print()
print("=" * 78)
print("L. the controls file's REBUILT_SHA= keys vs this paste's re-fire table")
print("=" * 78)

# These are two independent renderings of one number: column 2 is computed by the emitter from a
# rebuild, the block key was placed by hand and is asserted by the applier (exit 7). Binding them is
# what keeps C1's identity honest \u2014 it exists nowhere else except in a file this file says to
# regenerate whole, and its generator is gone.
L_START = bad
ctrl_src = CTRL.read_text()
blocks, cur, name = {}, [], None
for ln in ctrl_src.split("\n"):
    m = re.match(r"^### (C\d+)\b", ln)
    if m:
        if name:
            blocks[name] = "\n".join(cur)
        name, cur = m.group(1), []
        continue
    if name and (re.match(r"^#{1,3} ", ln) or ln.strip() == "---"):
        blocks[name] = "\n".join(cur)
        name, cur = None, []
        continue
    if name:
        cur.append(ln)
if name:
    blocks[name] = "\n".join(cur)

table = {}
for ln in lines:
    m = re.match(r"^\| (C\d+) \| `([0-9a-f]{64})` \| (.+?) \| `([0-9a-f]{64})` \|$", ln)
    if m:
        table[m.group(1)] = m.group(2)
if len(table) != 10:
    fail("re-fire table has %d rows, expected 10 \u2014 cannot cross-check" % len(table))

missing = [c for c in ORDER if c not in blocks]
if missing:
    fail("controls file has no `### Cn` block for %s \u2014 the applier keys off that heading too" % missing)
short = []
for c in ORDER:
    body = blocks.get(c, "")
    m = re.search(r"^[ \t]*REBUILT_SHA=([0-9a-f]+)", body, re.M)
    if not m:
        fail("%s: block states no REBUILT_SHA= \u2014 the hand-placed half of the pairing is gone" % c)
        continue
    if len(m.group(1)) != 64:
        short.append((c, len(m.group(1))))
        continue
    if c in table and table[c] != m.group(1):
        fail("%s: controls block REBUILT_SHA=%s\u2026 but this paste's table says %s\u2026"
             % (c, m.group(1)[:16], table[c][:16]))
if short:
    fail("REBUILT_SHA= truncated in blocks: %s" % short)
if not missing and not short and len(table) == 10 and bad == L_START:
    ok("all ten blocks carry a 64-hex REBUILT_SHA= equal to this paste's re-fire table")

print()
print("=" * 78)
print("PROBLEMS=%d" % bad)
print("=" * 78)
sys.exit(1 if bad else 0)
