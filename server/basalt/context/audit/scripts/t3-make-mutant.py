#!/usr/bin/env python3
"""Cut one negative-control mutant of the frozen census generator.

    usage: t3-make-mutant.py <controls.txt> <base.sh> <out.sh> <Cn> [--expect-sha=HEX64]

Reads control `Cn`'s `mutate:` block out of `t3-census-controls.txt` and applies it to <base.sh> as an
exact-string replacement, requiring the old side to occur EXACTLY ONCE. A control that silently mutated
nothing still runs and still prints confident numbers, which is the failure this check exists for;
`wave0/t3-census-controls.txt` § *Method* rule 2.

Why this file is in the tree rather than in scratch: `wave0/t3-negctl-rechecks.txt` § *Re-fire* claims a
later reader can rebuild all ten control programs from the controls file alone. That claim is not
executable without the applier. Re-firing the ten and comparing each rebuild against the identity its run
printed is the strongest provenance check the audit has.

INTERFACE — the controls file's heading grammar is an API surface, and nothing else will tell you when
you break it. Control bodies are keyed off `^### (C\\d+)` and end at the next heading of any level
(`^#{1,3} `) or a `---` rule. The FIXTURE is the body's indented lines; `> ` commentary is not scanned.
The arrow between the two sides of `mutate:` is U+2192 (→), not `->`. An ellipsis U+2026 in either side
aborts: it means the fixture is not stated completely, and completing one is a decision about a fixture,
so it belongs in the controls file where it can be dated and reviewed. C9's was completed on 2026-11-05
and proved by its rebuild hashing to the value that run printed.

BLOCK KEYS
    GENERATOR_SHA=   the 64-hex identity the block's own run printed. A transcription of a print; do not
                     "correct" it. One exception, gated on BOTH the control and an affirmative marker
                     (see the three-branch refusal below): C1's mutation replaces the generator's identity
                     print with `deadbeef`, so that block records the poison rather than a hash. It is not
                     exempt by *value* — a control whose block lacks `REBUILT_SHA=` and carries `deadbeef`
                     is refused.
     REBUILT_SHA=     the 64-hex sha256 of the program this script builds from the pair above, recorded
                      in the block and REQUIRED of every control: absent or malformed is exit 3, stated
                      twice is exit 4, and a build that disagrees is exit 7. That assertion is what makes
                      \u00a7 3b mechanical instead of ten rows a human eyeballs, and what puts C1's program
                      identity in a file that is not itself an output of the command \u00a7 3b asks you to
                      run. Place it ABOVE `mutate:`: the capture runs from `mutate:` to `GENERATOR_SHA=`,
                      so a key inside that span silently becomes part of the fixture.

EXIT CODES — distinct, because a driver must be able to branch without grepping prose:
    0 ok          2 usage             3 VACUOUS (the recipe does not describe this generator)
    4 AMBIGUOUS   5 incomplete fixture (ellipsis)   6 BASE-MISMATCH (--expect-sha)
    7 REBUILT-SHA does not equal the block's REBUILT_SHA
"""
import hashlib
import os
import pathlib
import re
import sys

ARROW = "\u2192"
ELL = "\u2026"
E_OK, E_USAGE, E_VACUOUS, E_AMBIGUOUS, E_INCOMPLETE, E_BASE, E_REBUILT = 0, 2, 3, 4, 5, 6, 7

# A short token is only ever legitimate as the identity print of THIS control, and only when the block
# separately states the program it is supposed to stand for. Keyed on the control, never on the value:
# value-keyed, "someone copies C1's block for a new control and leaves the poison token" walks straight
# through a WARN and exit 0, which is a fail-open in the one file whose job is refusing.
POISON_CONTROL, POISON_TOKEN = "C1", "deadbeef"


def die(code, msg):
    print(msg)
    sys.exit(code)


ARGS = sys.argv[1:]
if (len(ARGS) < 4 or len(ARGS) > 5
        or (len(ARGS) == 5 and not ARGS[4].startswith("--expect-sha="))):
    print(__doc__)
    sys.exit(E_USAGE)
md_path, base_path, out_path, ctl = ARGS[:4]
EXPECT = ARGS[4].split("=", 1)[1] if len(ARGS) == 5 else None

if EXPECT is not None and not re.fullmatch(r"[0-9a-f]{64}", EXPECT):
    die(E_USAGE, f"USAGE: --expect-sha is {len(EXPECT)} hex chars, not 64 — refusing to run a check "
                 "whose own reference is malformed")

MD = pathlib.Path(md_path).read_text()
BASE = pathlib.Path(base_path).read_text()

# The base is the frozen cut or the whole exercise is rebasing a table without moving it. Checked before
# any work, so a mismatched base can never produce a plausible-looking program.
base_sha = hashlib.sha256(BASE.encode()).hexdigest()
if EXPECT is not None and base_sha != EXPECT:
    die(E_BASE, f"BASE-MISMATCH: {base_path} is {base_sha[:16]}…, expected {EXPECT[:16]}… — the control "
                f"recipes are pinned to one cut of the generator; rebuilding from another one silently "
                f"moves every recorded hash")

# Blocks end at the next heading or rule, at any level: an unbounded scan makes C10 swallow § 2 and the
# rest of the file, which is harmless today only because the first `mutate:` happens to win.
blocks, cur, name = {}, [], None
for line in MD.split("\n"):
    m = re.match(r"^### (C\d+)\b", line)
    if m:
        if name:
            blocks[name] = "\n".join(cur)
        name, cur = m.group(1), []
        continue
    if name and (re.match(r"^#{1,3} ", line) or line.strip() == "---"):
        blocks[name] = "\n".join(cur)
        name, cur = None, []
        continue
    if name:
        cur.append(line)
if name:
    blocks[name] = "\n".join(cur)

if ctl not in blocks:
    die(E_VACUOUS, f"VACUOUS: no `### {ctl}` block in {md_path} — control bodies are keyed off that "
                   "heading, so renaming one is a breaking change to this script")

body = blocks[ctl]
# The FIXTURE is the block's indented lines only. Commentary is `> `-prefixed, and C9's note quotes
# `REBUILT_SHA=ef9c34e2…` inside a sentence: a search over the whole body finds the real key today only
# because of that prefix, and one re-wrapped sentence would hand it a truncated hex to die on — or,
# worse, a second disagreeing key would be silently ignored behind the first. Scope the scan, and
# refuse to guess when the fixture states a key twice.
fixture = "\n".join(l for l in body.split("\n") if re.match(r"^[ \t]", l))
if not fixture.strip():
    die(E_VACUOUS, f"VACUOUS parse {ctl}: the block has no indented fixture lines")
seg = re.search(r"^[ \t]*mutate:[ \t]*(.*?)^[ \t]*GENERATOR_SHA=([0-9a-f]+)", fixture, re.S | re.M)
if not seg:
    die(E_VACUOUS, f"VACUOUS parse {ctl}: no mutate:/GENERATOR_SHA= pair in the fixture")

recorded = seg.group(2)
# REBUILT_SHA is the affirmative marker that licenses the poison exception, and an independent identity
# for the recipe whether or not the print was poisoned. Parsed before the print check so C1 cannot be
# exempt from having a stated program identity.
reb = re.findall(r"^[ \t]*REBUILT_SHA=([0-9a-f]+)", fixture, re.M)
if len(reb) > 1:
    die(E_AMBIGUOUS, f"AMBIGUOUS {ctl}: the fixture states REBUILT_SHA= {len(reb)} times "
                     f"({', '.join(x[:12] + '…' for x in reb)}) — one key per block, or the applier is "
                     "guessing which identity to assert")
rebuilt_recorded = None
if reb:
    if not re.fullmatch(r"[0-9a-f]{64}", reb[0]):
        die(E_VACUOUS, f"VACUOUS {ctl}: REBUILT_SHA= is {len(reb[0])} hex chars, not 64 — a "
                       "reconstruction identity has been truncated or mistyped")
    rebuilt_recorded = reb[0]

if not re.fullmatch(r"[0-9a-f]{64}", recorded):
    if ctl == POISON_CONTROL and recorded == POISON_TOKEN and rebuilt_recorded:
        print(f"WARN {ctl}: GENERATOR_SHA= is the poison constant this control injects, not a program "
              f"hash; identity rests on REBUILT_SHA={rebuilt_recorded[:16]}… stated in the same block")
    elif recorded == POISON_TOKEN and ctl != POISON_CONTROL:
        die(E_VACUOUS, f"VACUOUS {ctl}: GENERATOR_SHA= is the poison constant belonging to "
                       f"{POISON_CONTROL}. Blocks get copied; this is the shape a copy-paste leaves "
                       "behind, and the program would have no recorded identity. State this control's "
                       "own REBUILT_SHA=, or restore the hash its run printed. Do not pad the token to "
                       "64 hex to clear this gate.")
    elif recorded == POISON_TOKEN:
        # i.e. ctl == POISON_CONTROL with no REBUILT_SHA= to license the short token
        die(E_VACUOUS, f"VACUOUS {ctl}: GENERATOR_SHA= is the poison constant this control injects and "
                       "no REBUILT_SHA= is asserted, so this program has no recorded identity. Add the "
                       "rebuild hash to the block; do not pad the poison token to 64 hex to clear "
                       "this gate.")
    else:
        die(E_VACUOUS, f"VACUOUS {ctl}: GENERATOR_SHA= is {len(recorded)} hex chars, not 64 — either a "
                       "recorded identity was truncated in transcription, or this block is not naming a "
                       "64-hex program. Fix it in the controls file with a dated note. Do not pad, prefix "
                       "or otherwise invent hex to get past this check.")

# REBUILT_SHA= is required of every control, not only of C1: without it a build is a program whose
# identity is recorded nowhere, and the § 3b table slides back into ten rows a human eyeballs. Checked
# AFTER the print checks so the sharpest diagnosis wins: a block copied from C1 that kept the poison and
# lost the key is told which of its two problems it has, not merely that a key is missing.
if rebuilt_recorded is None:
    die(E_VACUOUS, f"VACUOUS {ctl}: the fixture states no REBUILT_SHA=, so this build would have no "
                   "recorded identity to assert. State the sha256 of the program the pair above builds "
                   "(above `mutate:`, not inside the capture); do not invent hex to satisfy this.")

# Leading indentation is layout, not fixture: strip it per physical line so a recipe may wrap.
phys = [re.sub(r"^[ \t]+", "", l).rstrip() for l in seg.group(1).strip("\n").split("\n")]
joined = "\n".join(phys)
if ARROW not in joined:
    die(E_VACUOUS, f"VACUOUS {ctl}: no {ARROW} between the two sides of mutate: (this file wants U+2192, "
                   "not `->`)")
old, new = (s.strip() for s in joined.split(ARROW, 1))
if ELL in old or ELL in new:
    die(E_INCOMPLETE, f"INCOMPLETE {ctl}: the recipe contains \u2018{ELL}\u2019, so it does not state a "
                      f"complete fixture.\n  Complete it in {md_path} with a dated note, then re-run; do "
                      f"not substitute here.")

occ = BASE.count(old)
if occ == 0:
    die(E_VACUOUS, f"VACUOUS n=0 for {ctl}: the OLD side does not occur in this base — the recipe no "
                   "longer describes this generator (or the base is the wrong cut)")
if occ > 1:
    die(E_AMBIGUOUS, f"AMBIGUOUS n={occ} for {ctl}: the OLD side occurs {occ} times, so the recipe is not "
                     "naming a single site")

out_text = BASE.replace(old, new, 1)
rebuilt = hashlib.sha256(out_text.encode()).hexdigest()
if rebuilt_recorded and rebuilt != rebuilt_recorded:
    die(E_REBUILT, f"REBUILT-SHA MISMATCH {ctl}: this build is {rebuilt[:16]}…, the block records "
                   f"{rebuilt_recorded[:16]}… — either the recipe or the base has moved since the "
                   "re-fire that recorded it")

pathlib.Path(out_path).write_text(out_text)
# The lesson recorded at wave0/t3-negctl-rechecks.txt: a mutant written 644 while the base is 755 makes
# every run exit 126, and a permission failure reads as a broken control rather than a fired one.
os.chmod(out_path, os.stat(base_path).st_mode)
print(f"OK {ctl} old-occurrences=1 delta-bytes={len(new) - len(old)} rebuilt-sha256={rebuilt} "
      f"mode={os.stat(out_path).st_mode & 0o777:03o} base-sha256={base_sha[:16]}…"
      + (" REBUILT_SHA-verified" if rebuilt_recorded else ""))
