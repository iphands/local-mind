Target dc047a7 / tree 3f7b69ae — if that is not the tree you are looking at, you are in the wrong audit.

# `findings.md` — capability-ranked findings

Findings are ranked by **what the code could do**, not by anyone's intent. Nothing here is
evidence of deliberately planted malice; four of them are ordinary bugs that happen to sit
behind a trust boundary. Every claim is **executed**, not reasoned: the harness is
[`scripts/t3-model-file-poc.sh`](scripts/t3-model-file-poc.sh) and its output, with three
negative controls, is
[`wave0/t3-model-file-poc.txt`](wave0/t3-model-file-poc.txt). `[LIVE 2026-11-05]` throughout.

Severity key: 🔴 can leave the machine · 🟠 needs a second mistake · 🟡 local-only · ⚪ hygiene.

## Read this first — the threat model these findings sit in

A `.basalt` pack is **data the operator chooses to boot**, and the server parses it in
`main()` **before the HTTP listener binds** (`serve/server.py:125`). Nothing below is
reachable from the network, and no API request can trigger any of it: `/load` and `/unload`
(`serve/handler.py:317-335`) take **no path** and re-use the config built at startup. The
trigger for all four is one operator act — pointing `--model` at a pack they did not build.
That is a narrow trigger, so state these as *"an untrusted model file"*, never as a remote
exploit, or the report gets dismissed.

What makes the narrow trigger worth writing down at all: `.basalt` packs are exactly the
artefact this project downloads and shares, `cache_dir()` is keyed on data the file itself
declares, and the parser **never** calls the `verify()` that the format's own digests exist
to support.

## F-1 🔴 — a model file writes any file the server's user can write

**What it does.** `tools/model_file.py:163` stores a section name taken straight from the
file header, unsanitised. `serve/engine/modelfile.py:95` builds an output path from it:

```python
(tmp / n.split("/", 1)[1]).write_bytes(mf.read(n))
```

A section named `tokenizer//abs/path` yields `split("/", 1)[1] == "/abs/path"`, and
`Path(x) / "/abs/path"` **discards the left operand entirely** — the cache directory is not
escaped component by component, it is thrown away. `POC F1_ABSOLUTE_JOIN_WRITE=wrote`, with
no `..` in the name at all. The `../` form also works (`F1_TRAVERSAL_WRITE=wrote`). The
payload `tokenizer/../../../../.ssh/authorized_keys` **was written for real**, into the
sandbox `$HOME` (`F1_AUTHORIZED_KEYS_WRITE=wrote`).

Root cause, demonstrated rather than asserted: `ModelWriter._start` rejects a 64-byte
section name; `ModelFile.__init__` **accepts the same name** (`F1_WRITER_REJECTS_64=ValueError`
against `F1_READER_ACCEPTS_64=yes`). The reader trusts invariants enforced only by a writer
it never meets, and the format's 64-byte `ENTRY` name field is the only real length limit.

**The reachability rule is wider than "the parent must exist."** `write_bytes` is
`open(p, "wb")`, which cannot `mkdir` — so a **parent directory is required** — but it does not
require `p` itself to exist. So the class of writable targets is *every existing file* plus **any
new file in any existing directory**, and the new-file half is the quieter one: nothing is
overwritten, so nothing looks damaged.

Measured against this host by the harness (`POC_REACH` lines; three columns because byte count,
reachability and visibility are three different facts):

| target | name bytes | parent | state | git |
|---|---|---|---|---|
| `$HOME/.gitconfig` | 34/64 | ok | symlink → `~/dotfiles/.gitconfig` | tracked, remote `github.com/iphands/dotfiles.git` |
| `$HOME/.bashrc` | 31/64 | ok | symlink → `~/dotfiles/.bashrc` | tracked |
| `$HOME/.profile` | 32/64 | ok | file, 21 B, mode 644 | — (outside a repo) |
| `$HOME/.bash_aliases` | 37/64 | ok | **absent → new file**, auto-sourced by most `.bashrc` | — |
| `…/local-mind/.claude/settings.json` | **64/64** | ok | **absent → new file** | **IGNORED** by `~/.gitignore:16 /.claude` |
| `$HOME/.claude/settings.json` | 45/64 | ok | absent → new file | — |
| `…/server/basalt/run` | 60/64 | ok | file, 7,048 B, **mode 755**, `#!/usr/bin/env bash` | tracked |
| `…/server/basalt/build` | 62/64 | ok | file, 4,634 B, mode 755, bash | tracked |
| `…/local-mind/bin/opencode` | 55/64 | ok | file, 2,104 B, mode 755, `#!/bin/bash` | tracked |
| `…/server/basalt/AGENTS.md` | 66/64 | ok | over budget | — |
| `…/server/basalt/README.md` | 66/64 | ok | over budget | — |
| `$HOME/.ssh/authorized_keys` | 44/64 | **no parent** | `~/.ssh` does not exist | — |
| `$HOME/.ssh/config` | 35/64 | no parent | — | — |
| `$HOME/.config/systemd/user/x.service` | 54/64 | no parent | — | — |
| `$HOME/.config/autostart/x.desktop` | 51/64 | no parent | — | — |

The `authorized_keys` row is the one both reviewers leaned hardest on and **it does not apply to
this machine** — no `~/.ssh`, so no parent. It is demonstrated only against a sandbox `$HOME` that
has one. A developer box that does have one is the normal case, so the leg is real in general and
unavailable here. Say it out loud: it is the reason the severity argument below is *not* built on
`authorized_keys`.

**`open(p,"wb")` truncates the existing inode** — it neither replaces it nor changes its mode
(`F1_TRUNCATE_PRESERVES_MODE_AND_INODE=yes`, same inode, still `0755`, still `os.access(X_OK)`).
That single fact is what turns a file write into code execution on `run`, `build` and
`bin/opencode`: the overwrite leaves them executable and interpreted, so the next invocation runs
the attacker's script. Contrast the refuted `os.replace` leg below, which *does* replace the
inode.

**Symlinks are followed at the final component *and* through path components.** Both measured.
A dotfile here that is symlinked into `~/dotfiles` has its **target** written, so a hostile
`.gitconfig` lands in `~/dotfiles` — which is a git repo with remote
`github.com/iphands/dotfiles.git`, and `.gitconfig`/`.bashrc` are tracked in it (pasted in
[`wave0/t3-model-file-poc.txt`](wave0/t3-model-file-poc.txt)). And because `open()` resolves
components, a **symlinked directory** is an exit: `server/basalt/models` is a symlink to
`/mnt/noir/scratch/ai/llm/models`, and the 59-byte relative name
`tokenizer/../../../../../proj/server/basalt/models/evil.pth` wrote **into that shared store**
(`F1_SYMLINKED_COMPONENT_WRITE=wrote`). `models/` is on the project's own sensitive-file surface
(RULES' `PATTERNS`), which says the project already agrees it matters.

**Why it is 🔴 and not 🟠.** The trigger is one operator mistake, which fits 🟠. It sits above that
line on three legs, all measured live on this host, none needing a second mistake:

- **`run` / `build` / `bin/opencode` overwritten** — all bash, all mode 755, all surviving the
  truncate executable → arbitrary code as the user on the next invocation. `run` is the documented
  sole entrypoint for booting every pack.
- **`.bashrc` / `.profile` / a new `.bash_aliases`** — attacker-authored shell init, executed by
  the next interactive or login shell. `.bash_aliases` needs no overwrite at all.
- **`.claude/settings.json`, 64/64 bytes, as a brand-new file** — hook configuration an agent
  runner reads, and `git status` reports **nothing** for it, because the operator's *global*
  `~/.gitignore:16 /.claude` ignores the whole directory. This is the only row in the table with
  no diff to catch it, and it is the one that needs no overwrite. Note the caveat: that ignore
  rule is a **host** property, not a repo property.

State the trade-off rather than pretending the scale settles it: neckbeard argued 🟠 while the
trigger is the operator's own `--model` argument, and that argument survives the re-rating.

**Why it isn't scarier than that.** `write_bytes` cannot `mkdir` — which is what the `parent`
column is for. Every *overwrite* leg is **loud**: replacing a 7,048-byte launcher with a one-line
script, or a whole `.gitconfig` (identity, aliases, credential helper) with one section, is the
biggest diff in whichever repo tracks it, and this project's contract forces a commit and a
mechanical gate per task. `run` and `build` are tracked, so the exposure window is write → next
`git status`. Only the *persistence* is quiet; the *overwrite* is not. Do not write "silently
redirects a `git push`" — that sentence was wrong in the loud direction, and the delivery story
for the dotfiles leg is weaker than it looks: the hostile file has to survive the operator's own
`git status` in the most-diffed repo a person owns, and `insteadOf` only fires when they next push
a matching URL. Beyond that: `cache_dir` is a `.hex()` string, so it is hex by construction — no
path or argv injection through it. `cfg["args"]` is built with `os.path.abspath` and the engine is
spawned as a **list with no `shell=True`** (`serve/engine/process.py:265`) — no shell anywhere on
this path. And a pack **cannot name a binary**: `engine_bin` is a CLI/config parameter
(`modelfile.py:137`), never read from the model file, so loading a hostile pack is not "run this
executable".

**The one action.** Validate section names in `ModelFile.__init__` — one allowlist
(`^[A-Za-z0-9][A-Za-z0-9._-]*(/[A-Za-z0-9._-]+)*$`) covers every present and future consumer,
in Python and anything else that parses this header, and it defeats *all four* legs above because
all four are carried in the name. A `Path.resolve().is_relative_to()` check at the sink is a second
belt, not the fix: it must run *before* the `mkdir`s at `modelfile.py:91`, it fixes one sink rather
than the format, and on its own it is **bypassable by a symlinked path component** — which is
measured above, not hypothetical. The belt that actually holds at the sink is `os.open` with
`O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW` on a dirfd-relative path, which is the idiom
`serve/engine/image_source.py::_open_under` already uses correctly in this same tree.

## F-2 🟠 — the model cache is keyed on data the file declares, so one pack can poison another

**What it does.** `cache_dir()` is `Path.home()/".cache"/"basalt"/ mf.sections["meta"][2].hex()[:16]`
— the meta digest **as written in the header**, never recomputed from the bytes, and
`ModelFile.verify()` is called from **no production path** (only `tools/convert/` and
`tools/migrations/` tests). So the cache identity is attacker-chosen, and the lookup is
existence-gated and content-blind: `if (out / "vocab.json").exists(): return out`.

Two ways in, both executed:

- **Declared-digest collision.** Copy a legitimate pack's 32 declared digest bytes into a
  tiny hostile pack (no preimage needed) → identical cache dir. Boot the hostile one first
  and the **real pack's** `extract_tokenizer()` returns the attacker's `vocab.json`
  (`F2_EVIL_BOOTED_FIRST=poisoned`), and the hostile file can then be deleted. Boot the real
  one first and it does not take (`F2_REAL_BOOTED_FIRST=no`) — **first writer wins**; say so,
  it is a real limit on the attack.
- **Sibling poisoning**, which needs no collision at all: the name
  `tokenizer/../../<16-hex>/tokenizer/vocab.json` is 53 bytes and writes into *another*
  model's cache (`F2_SIBLING_CACHE_POISON=poisoned`). The 16 hex characters are readable off
  any copy of the target pack, which is a published artefact.

**The consequence is bigger than the tokenizer, and this is the part that matters.** The same
directory is everything the server caches for a model, and `serve/server.py` reads it back as
trusted:

- `serve/server.py:164-165` sets `a.tokenizer = cfg["tokenizer"]` — the **extracted** dir —
  so `:222` renders `ChatTemplate(tpath / "chat_template.jinja")` from **the pack's own
  section** when present, falling back to the bundled template only if absent
  (`F2_PACK_CHAT_TEMPLATE_AT_TPATH=yes`).
- `serve/server.py:170-177` reads `vocab.json`, `merges.txt` and `token_type.json` from that
  same directory (`F2_PACK_VOCAB_AT_TPATH=yes`) — a pack can substitute the tokenizer.
- `cfg["model_name"]` is attacker JSON, surfaced as the advertised model id
  (`F2_MODEL_NAME_FROM_PACK=TplPack`).
- With `--conversation-cache-disk-mib` above zero, two colliding packs share one
  `--conversation-cache-dir` (`F2_SHARED_CONVERSATION_CACHE=yes`) and one `engine.log`
  (`F2_SHARED_ENGINE_LOG=yes`) — cross-model state sharing, not just bad tokenizer files.

**Why it isn't 🔴.** It is an integrity chain, not a primitive: the template is rendered by
`ImmutableSandboxedEnvironment` (`serve/frontend.py:19,37`), so there is no expression-level
code execution, and no demonstrated path sends anything off the machine. What was *not*
tested is whether an injected template changes generated output end to end — `jinja2` is not
installed in the audit shell (`requirements.txt:5` pins `jinja2==3.1.6`), so the harness
proves the file lands where production reads it, and stops there. Do not write "silently
changes model behaviour" beyond that.

**What would make it scary.** That a poisoned `chat_template.jinja` is a *persistent,
attacker-authored wrapper around every prompt on a pack the operator did verify*, with the
attacker no longer present. Reaching a machine exit from there needs a client that obeys
injected text — which is why this is 🟠 and F-1 is 🔴.

**The one action.** Derive the cache key from the bytes (`hashlib.sha256(mf.read("meta"))`)
and call `mf.verify(name)` before extracting each section — the digests already exist and
cost one pass. Note this **contradicts `serve/tests/test_modelfile.py:87-89`**, which asserts
that a present cache directory is not rewritten, so it is a contract change and needs the
maintainer, not a patch.

## F-3 🟡 — declared section sizes are never bounded against the real file

**What it does.** `ModelFile.__init__` checks `getsize(path) < file_bytes` and never checks
`off + size <= file_bytes`. Two executed consequences:

- **Silent truncation.** A 1,114,127-byte file declaring a 1 GiB `tokenizer/vocab.json` makes
  `read()` return the short real bytes **with no exception** (`F3_SHORT_READ_RAISES=none`),
  and `extract_tokenizer` never compares what it got against the declared size. Cached,
  short, never re-checked.
- **Disk amplification.** An input file with **76 KiB allocated** (sparse, 256 MiB apparent)
  and `vision/mmproj` declared at the full length makes `write_section` write a
  **256 MiB fully allocated** file into `~/.cache` — `F3_SPARSE_INPUT_ALLOCATED_KIB=76`,
  `F3_SPARSE_OUTPUT_ALLOCATED_MIB=256`, ~3,400×, needing `--vision`. The bound is the file's
  *apparent* length, and the input is itself sparse, so input cost is metadata.
- A declared size past what the machine has makes `read()` attempt that allocation: a 64 GiB
  declaration raised `MemoryError` under a deliberate `RLIMIT_AS=512MiB` guard rather than
  touching the host. The availability story is weak — real packs are already huge — so this
  row is mostly the first two bullets.

**Why it isn't scarier.** `write_section`'s `if not b: raise IOError` short-read check is a
genuine guard, and its `finally` unlinks the temp file, so the copy leg fails loudly and
self-cleans; `verify()` handles oversize correctly too. This is the one function in the file
that gets it right, along with `vision_config`'s size comparison at `modelfile.py:119`.

**The one action.** In `ModelFile.__init__`, reject `off > file_bytes or size > file_bytes - off`.

## F-4 ⚪ — malformed files crash with a traceback instead of a one-line rejection

**What it does.** `serve/server.py:129` catches `(OSError, ValueError, KeyError)`. Three
inputs escape it, all asserted: an **8-byte file** → `struct.error` (because
`HEADER.unpack_from` runs **before** the magic check); **`n = 512` sections** → `struct.error`
(`n` is never checked against `MAX_SECTIONS = 511`); **`meta` = `[1,2]`** → `AttributeError`.
`F4_STRUCT_ERROR_IN_TUPLE=no`, so these propagate out of `main()` — which is the operator's
actual traceback, no server boot needed to see it.

The honest symptom is not an attack: **a truncated download or a mistyped path gets a
traceback instead of `not a .basalt model file`**. A robustness bug for honest users that
happens to apply to hostile ones too.

**Checked, and correctly handled** — recorded so the rejection surface is stated completely
rather than inferred, and each stays `clean`: non-UTF-8 name bytes → `UnicodeDecodeError` (a
`ValueError`); an embedded NUL → `ValueError`; a name of exactly `tokenizer/` →
`IsADirectoryError`; a file truncated to 3000 bytes → `ValueError`. Two further format
invariants enforced only on the write side: a duplicate name collapses after `rstrip(b"\0")`
and **last wins silently** (`OK_DUPLICATE_LAST_WINS={"SECOND":1}`); and a **zero-length**
`tokenizer/vocab.json` from a perfectly well-formed pack passes both the cache gate and
`server.py:168`'s `vocab.json exists` check forever (`OK_ZERO_LENGTH_VOCAB_PASSES_GATE=yes`).

**The one action.** Raise `ValueError` from the parser: check `len(head) < HEADER_BYTES` and
`n > MAX_SECTIONS` **before** unpacking, and validate `meta` is an object.

## A hypothesis that was wrong — so nobody re-reports it

`write_section` ends in `os.replace(tmp, dest)`, and `os.replace` on POSIX **replaces a
symlink at `dest` rather than following it**. Planting a symlink at
`cache_dir/vision/mmproj.gguf` therefore redirects nothing: the victim file kept its contents
and the symlink was replaced. `REFUTED_OS_REPLACE_FOLLOWS_SYMLINK=not-followed`. There is no
second write primitive here, and fixing names plus sizes is sufficient on the Python side.

## Checked and clean `[LIVE 2026-11-05]`

The negative results, because "is this safe" needs them as much as the findings do.

- **No `eval`, `exec`, `compile`, `pickle`, `marshal` or `__reduce__`** anywhere in the serve
  path. `serve/service.py`'s 56 code-exec-adjacent hits are all defensive
  `getattr(self.engine, "known_attr")` probing.
- **No suspicious remote endpoint.** Hardcoded addresses are README/docs examples,
  `serve/config.py:403-404` defaults, an SSRF table, and tests (`127.0.0.1`, `example.com`,
  `evil.test`, `forged`). The only outbound channels are `tools/convert/mtp_fetch.py:31`
  (`urllib`, hardcoded `huggingface.co/Qwen/...` with a pinned-revision check at `:136`) and
  `serve/server.py`'s `webbrowser`. One `wget` of a CUDA keyring in `.github/workflows/ci.yml:80`.
- **`serve/engine/image_source.py` and `serve/security.py` hold up.** DNS rebinding is
  defeated by resolving first, requiring *every* address be public, then connecting to those
  exact IPs via `_PinnedConnection` while `Host`/SNI/cert stay on the original name; no
  redirect handler is registered. The symlink TOCTOU path uses an fd-relative `O_NOFOLLOW`
  walk (`_open_under`). Non-regular files are refused before `open`, with `O_NONBLOCK`
  against FIFOs, and reads are capped and deadline-bounded.
- **`remote_reason` is a one-way ratchet, which is the right shape**: only the socket peer can
  make a request *local*; forwarding headers, a non-loopback `Host`, any `Origin`, or a
  malformed header can only push it *remote* (`serve/security.py:187-207`). Everything
  alarming in `image_source.py` — arbitrary path read, arbitrary URL fetch — is gated on
  *local*, i.e. on this machine, by design.
- `serve/engine/vision.py`'s whitespace-split `ENC` protocol takes paths built from
  `sha256(...).hexdigest()[:32]`, so it is not attacker-reachable. (`TMPDIR` containing a
  space would break it; environment-controlled, cosmetic.)
- The API key comparison is constant-time and handles the latin-1/UTF-8 header split
  (`serve/security.py:35-45`).

## Not examined — the honest boundary of this page

- **`src/` and `include/` — the C++ engine's own reader of the same 128-byte entry table.**
  Whether it bounds `off + size` against file length, and whether it turns a section name into
  a path, is **the largest untested surface in the audit** and it is neither cheap nor local.
  Nothing on this page constrains it. Any Python-side fix is incomplete until someone reads it.
- The vendored third-party tree — ggml, llama.cpp, gguf-py, and `src/vision/stb_image.h`
  (7,988 lines, `sha256 594c2fe3…`, **2.30**, fed by user-supplied images) — was tiered
  `B-vendored@T5` on **recognised names**, which is a deferral and not a verdict. A file named
  `stb_image.c` that is not stb_image is exactly how this class hides. Exact identity to a
  pristine upstream 2.30 is **unproven**: the banner, the changelog head and the code markers
  (`STBI_MAX_DIMENSIONS :796`, `stbi__pnm_is16 :7624` → floor ≥2.27) say 2.30, but no local oracle
  exists and the hash comparison against upstream's tag is an owner step. The plan's Key Facts
  said **2.21** until 2026-11-05; corrected in place, with the method failure recorded in
  `pitfalls.md`.
- **Unmeasured leads on the decode path, deliberately not ranked here** — they are derived by
  reading, not by running, and this page only ranks executed findings. All of them belong to T5:
  `preprocess.cpp:34` returns `w,h` that may differ from the header dims the `:24-33` guard
  checked, and `:41` sizes `out.rgb.assign(px, px + w*h*3)` from the rewritten values; no
  `STBI_MALLOC` ceiling and no decode timeout, on a box whose design is system RAM holding the
  expert tier; `:20` `std::min<size_t>(len, 0x7fffffff)` **silently truncates** rather than
  refusing; `fit_size` (`:46-64`) has no clamp back to `max_px` and cannot shrink a side below
  `f = patch*merge`, which analytic reading says can overshoot badly at extreme aspect ratios;
  and `stbi_set_flip_vertically_on_load*` has **0 call sites** outside the vendored header, so
  if the encoder expects bottom-up every image is vertically mirrored and neither a fuzzer nor
  the PPL/KLD gate would see it. The measured ceilings are real and are recorded in the plan:
  32 MiB/image, 256 MiB/request, 64 MP (`kMaxImagePixels`), `1<<24` per side.
- Whether an injected `chat_template.jinja` changes generated output end to end (needs `jinja2`).
- `tests/` (163 files / 51,125 lines) is owned by no plan task.

## Where the fixes go

**Upstream, not into `vendor/`.** `vendor/basalt` is pinned, read-only in this project, and
`git diff` against `dc047a7` must stay empty (the freeze check above). These four are bug
reports for the maintainer — F-2 additionally needs them to accept a cache contract change.
