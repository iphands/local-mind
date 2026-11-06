// jev: hands cheap classification work to the H2O Lightning 4B decision model
// (server/llamacpp/run-jev-4060, POST /v1/systemone). It never generates text: it prefills a
// `state` and returns probabilities over the options we give it.
//
//   F1  subagent (neckbeard/hoodie) execute.before: append a pre-screen block of likely
//       anti-patterns, and set review depth from a risk rating (quick consult / full review).
//       Never skips a reviewer.
//   F2  shell execute.after: on exit-0 runs, append a pass/fail/warnings verdict hint.
//   F3  permission evaluate (shell): escalate allow -> ask for destructive, network or system
//       commands. Only ever escalates.
//   F4  ask_jev tool: the agent asks one question about files (or a saved tool output) and gets
//       one answer per file without reading them. Runs under the `read` permission, checks each
//       file like the read tool does. Its requests go one at a time across all ask_jev calls (a
//       global queue), so the server's other slots stay free for the hooks.
//
// Fail-open: any error, timeout or bad answer leaves the call exactly as it was. After one
// failure every hook call is skipped for COOLDOWN_MS. Hooks never throw: a throw from
// execute.before blocks the tool call, and a rejection from the other hooks is a defect, not a
// tool error. The ask_jev tool is the one place that may fail: it errors with a readable reason
// and the agent reads the files instead. Its timeouts never start the cooldown (see `query`).
//
// API notes (probed against llama.cpp b12169):
//   - extra keys such as `question` are ignored, so the question has to live in `state`
//   - each question re-prefills the whole state (~4k tok/s on the 4060): pack a checklist into
//     one `choice` rather than one question per item
//   - `choice` criteria descriptions are tokenized and carry the meaning of each option
//
// Env: JEV_URL (default http://cosmo.lan:8701), JEV_DISABLE=1 turns every feature off.
//
// Trace view (off by default): while the file $XDG_CONFIG_HOME/opencode/jev-trace exists, or
// JEV_TRACE=1 is set, each request (state preview + questions) and the raw response go into the
// TUI as formatted JSON, attached to the tool result: F2 and F3 to the shell call, F1 to the
// reviewer's result (never to its prompt). The file is checked on every call, so `touch` and
// `rm` toggle it without a restart. Traces are part of the tool result, so the agent sees them too.

import { execFile } from "node:child_process"
import { existsSync } from "node:fs"
import { readFile, stat } from "node:fs/promises"
import { homedir } from "node:os"
import { isAbsolute, join, parse, relative, resolve, sep } from "node:path"

const JEV_URL = (process.env.JEV_URL ?? "http://cosmo.lan:8701").replace(/\/+$/, "")
const DISABLED = process.env.JEV_DISABLE === "1"
const TRACE_FILE = join(process.env.XDG_CONFIG_HOME || join(homedir(), ".config"), "opencode", "jev-trace")

function tracing(): boolean {
  if (process.env.JEV_TRACE === "1") return true
  try {
    return existsSync(TRACE_FILE)
  } catch {
    return false
  }
}

// Tunables: set from the calibration run, see the comment above each
const TIMEOUT_MS = 8_000 // shell verdict and shell risk
const REVIEW_TIMEOUT_MS = 15_000 // 3 questions over up to ~6k tokens is ~4.5 s on an idle 4060
const COOLDOWN_MS = 60_000 // after a failure, skip all calls this long
const STATE_MAX_CHARS = 24_000 // ~6k tokens; head + tail are kept
const OUTPUT_TAIL_CHARS = 16_000 // ~4k tokens of command output
const MIN_OUTPUT_CHARS = 200 // shorter shell output is not worth a verdict
const FLAG_MIN = 0.15 // list a reviewer flag at or above this probability
const TRIVIAL_MIN = 0.8 // quick consult only when "trivial" is at least this sure
const RISKY_MIN = 0.6 // ask for a full review at or above this
const VERDICT_MIN = 0.5 // below this the verdict line says "unsure"; a fail hidden by exit 0 scored 0.55
const RUNNER_UP_MIN = 0.2 // also show the second label when it is at least this likely
const ESCALATE_MIN = 0.7 // shell allow -> ask at or above this
const IRREVERSIBLE_MIN = 0.85 // shell allow -> ask on an irreversible answer alone
const CACHE_MAX = 32
const TRACE_STATE_CHARS = 800 // state shown in a trace: head + tail
const TRACE_KEEP = 16 // traces waiting for their tool result
const TRACE_MAX_AGE_MS = 10 * 60_000 // a waiting trace older than this is dropped

type Criteria = Record<string, string>
type Questions = Record<string, { type: "choice"; criteria: Criteria }>
type Choice = { choice: string; probabilities: Record<string, number> }
type Answers = Record<string, Choice>

const log = (msg: string) => console.warn(`[jev] ${msg}`)

// ---------------------------------------------------------------- decision-model client

let downUntil = 0

// What was sent and what came back, for the trace view. `response` is the raw body.
type Trace = { request: { state: string; questions: Questions }; response?: unknown; error?: string; ms: number }
type Result = { answers: Answers | null; trace: Trace }

// An answer the server sent but we can't use (bad status, bad shape): the server is up
class ServerAnswerError extends Error {}

// Lanes: hooks ("hook") start the cooldown on any failure, as they always have. The ask_jev tool
// ("tool") respects a running cooldown but only starts one when the server can't be reached at
// all, so a slow scan that times out never switches off F3's gate. An abort through `signal`
// (the user interrupted the tool) is not a failure of the server: no cooldown, no log line.
type QueryOpts = { signal?: AbortSignal; lane?: "hook" | "tool" }

// Tool requests go one at a time across ALL ask_jev calls, even ones the agent runs in parallel.
// The benchmark (server/llamacpp/run-jev-4060-benchmark) showed chunk throughput is flat from one
// client to eight, so more in flight only takes slots from the hooks. Hooks never queue here.
let toolQueue: Promise<void> = Promise.resolve()

// Resolves with a release function once it is this request's turn; rejects if `signal` aborts
// first. An aborted waiter still hands its turn on, so the queue can't stall.
async function toolTurn(signal?: AbortSignal): Promise<() => void> {
  let release!: () => void
  const mine = new Promise<void>((done) => (release = done))
  const prev = toolQueue
  toolQueue = mine
  if (!signal) {
    await prev
    return release
  }
  try {
    await new Promise<void>((done, fail) => {
      if (signal.aborted) return fail(new Error("aborted"))
      signal.addEventListener("abort", () => fail(new Error("aborted")), { once: true })
      prev.then(() => done())
    })
  } catch (err) {
    prev.then(release)
    throw err
  }
  return release
}

// Never throws: every failure is { answers: null } with the reason in trace.error
async function query(state: string, questions: Questions, timeoutMs = TIMEOUT_MS, opts: QueryOpts = {}): Promise<Result> {
  if (opts.lane !== "tool") return send(state, questions, timeoutMs, opts)
  let release: () => void
  try {
    release = await toolTurn(opts.signal)
  } catch {
    return { answers: null, trace: { request: { state, questions }, ms: 0, error: "aborted" } }
  }
  try {
    return await send(state, questions, timeoutMs, opts) // the timeout starts once it is our turn
  } finally {
    release()
  }
}

async function send(state: string, questions: Questions, timeoutMs: number, opts: QueryOpts): Promise<Result> {
  const trace: Trace = { request: { state, questions }, ms: 0 }
  if (DISABLED) return { answers: null, trace: { ...trace, error: "JEV_DISABLE=1" } }
  const wait = downUntil - Date.now()
  if (wait > 0)
    return { answers: null, trace: { ...trace, error: `skipped: cooling down after a failure (${Math.ceil(wait / 1000)}s left)` } }
  const t0 = Date.now()
  const timeout = AbortSignal.timeout(timeoutMs)
  try {
    const res = await fetch(`${JEV_URL}/v1/systemone`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ state, questions }),
      signal: opts.signal ? AbortSignal.any([timeout, opts.signal]) : timeout,
    })
    if (!res.ok) throw new ServerAnswerError(`HTTP ${res.status}: ${(await res.text()).slice(0, 200)}`)
    const body = (await res.json().catch(() => {
      throw new ServerAnswerError("response is not JSON")
    })) as { answers?: Record<string, Partial<Choice>> }
    trace.response = body
    const answers: Answers = {}
    for (const id of Object.keys(questions)) {
      const a = body.answers?.[id]
      if (!a || typeof a.choice !== "string" || typeof a.probabilities !== "object" || !a.probabilities)
        throw new ServerAnswerError(`malformed answer for ${id}`)
      answers[id] = { choice: a.choice, probabilities: a.probabilities }
    }
    trace.ms = Date.now() - t0
    return { answers, trace }
  } catch (err) {
    trace.ms = Date.now() - t0
    if (opts.signal?.aborted) {
      trace.error = "aborted"
      return { answers: null, trace }
    }
    const timedOut = timeout.aborted
    trace.error = timedOut ? `timed out after ${timeoutMs / 1000}s` : err instanceof Error ? err.message : String(err)
    const unreachable = !timedOut && !(err instanceof ServerAnswerError)
    if ((opts.lane ?? "hook") === "hook" || unreachable) {
      downUntil = Date.now() + COOLDOWN_MS
      log(`unavailable (${trace.error}); skipping for ${COOLDOWN_MS / 1000}s`)
    } else log(`ask_jev request failed (${trace.error}); hooks unaffected`)
    return { answers: null, trace }
  }
}

async function ask(state: string, questions: Questions, timeoutMs = TIMEOUT_MS): Promise<Answers | null> {
  return (await query(state, questions, timeoutMs)).answers
}

// Keep the head and tail of long text; the middle of a diff or log matters least
function cap(text: string, max = STATE_MAX_CHARS): string {
  if (text.length <= max) return text
  const half = Math.floor((max - 40) / 2)
  return `${text.slice(0, half)}\n\n[... ${text.length - 2 * half} chars cut ...]\n\n${text.slice(-half)}`
}

const tail = (text: string, max: number) => (text.length <= max ? text : `[... cut ...]\n${text.slice(-max)}`)

function hash(s: string): string {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193)
  return (h >>> 0).toString(16)
}

// Caches the promise, not the value, so the two reviewer launches sent in one message share a
// single request. Failed results are dropped so the next call can retry after cooldown.
const cache = new Map<string, Promise<Result>>()
function cached(key: string, run: () => Promise<Result>): Promise<Result> {
  const hit = cache.get(key)
  if (hit) return hit
  const p = run().then((v) => {
    if (v.answers === null) cache.delete(key)
    return v
  })
  cache.set(key, p)
  while (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value as string)
  return p
}

const pct = (p: number) => p.toFixed(2)

// ---------------------------------------------------------------- trace view

// Formatted JSON for the TUI; the state is cut to its head and tail
function traceBlock(label: string, t: Trace): string {
  const view = {
    request: { state: cap(t.request.state, TRACE_STATE_CHARS), questions: t.request.questions },
    ...(t.response === undefined ? {} : { response: t.response }),
    ...(t.error === undefined ? {} : { error: t.error }),
    ms: t.ms,
  }
  return `[jev trace] ${label}\n\`\`\`json\n${JSON.stringify(view, null, 2)}\n\`\`\``
}

// Traces made before a tool runs (F1, F3) wait here until its execute.after event arrives
type Waiting = { match: (event: any) => boolean; label: string; trace: Trace; at: number }
const waiting: Waiting[] = []
function park(w: Omit<Waiting, "at">) {
  waiting.push({ ...w, at: Date.now() })
  while (waiting.length > TRACE_KEEP) waiting.shift()
}
function collect(event: any): string[] {
  const now = Date.now()
  const out: string[] = []
  for (let i = 0; i < waiting.length; ) {
    const w = waiting[i]
    if (now - w.at > TRACE_MAX_AGE_MS) waiting.splice(i, 1)
    else if (w.match(event)) {
      out.push(traceBlock(w.label, w.trace))
      waiting.splice(i, 1)
    } else i++
  }
  return out
}

// Adds text to a tool result as the model and the TUI see it, whatever shape `content` has
function withText(result: any, text: string): any {
  const content = result.content
  return {
    ...result,
    content:
      typeof content === "string"
        ? `${content}\n${text}`
        : [...(Array.isArray(content) ? content : []), { type: "text", text }],
  }
}

// Labels at or above `min` other than `skip`, most likely first
function flagged(c: Choice, min: number, skip: string): [string, number][] {
  return Object.entries(c.probabilities)
    .filter(([label, p]) => label !== skip && p >= min)
    .sort((a, b) => b[1] - a[1])
}

// ---------------------------------------------------------------- F1: reviewer pre-screen

const REVIEWERS = new Set(["neckbeard", "hoodie"])

const REVIEW_QUESTIONS: Questions = {
  neckbeard_flag: {
    type: "choice",
    criteria: {
      none: "no correctness, safety or security problem in this code",
      type_suppression: "suppresses the type checker: as any, @ts-ignore, type: ignore, unchecked casts",
      empty_catch: "silently swallows an error: empty catch or except, ignored error result, || true",
      weakened_test: "deletes, skips or weakens a test or an assertion",
      hardcoded_secret: "hardcodes a password, token, API key or other credential",
      injection: "builds a shell command, SQL query or file path from untrusted input without escaping",
      null_unhandled: "uses a value that may be null, undefined, None or missing without checking it",
      logic_bug: "has a logic error, an off-by-one, or an unhandled edge case",
    },
  },
  hoodie_flag: {
    type: "choice",
    criteria: {
      none: "no performance or usability problem in this code",
      quadratic: "nested loops or repeated scans where one pass or a lookup table would do",
      io_in_loop: "makes a network, database, file or subprocess call inside a loop",
      unbounded_load: "loads a whole file, table or dataset into memory where it could stream or paginate",
      missing_cache: "recomputes or refetches an expensive result that could be cached",
      blocking_sync: "blocks the main thread or event loop with synchronous I/O or sleeps",
      poor_error_message: "error messages that do not say what failed or how to fix it",
    },
  },
  risk: {
    type: "choice",
    criteria: {
      trivial: "only comments, docs, formatting, renames or log text change; behaviour is unchanged",
      standard: "an ordinary code or configuration change",
      risky: "touches security, auth, data deletion, concurrency, migrations or production config",
    },
  },
}

// The reviewed material: fenced blocks (code and diffs) when there are any, else the whole prompt
function reviewContent(prompt: string): string {
  const blocks = [...prompt.matchAll(/```[^\n]*\n([\s\S]*?)```/g)].map((m) => m[1].trimEnd())
  const body = blocks.join("\n\n").trim()
  return body.length >= 40 ? body : prompt
}

function reviewState(prompt: string): string {
  return (
    "A coding agent wrote the change below and is sending it to code review. " +
    "Judge the change itself.\n\n" +
    cap(reviewContent(prompt))
  )
}

function preScreen(agent: string, a: Answers): string {
  const own = a[agent === "neckbeard" ? "neckbeard_flag" : "hoodie_flag"]
  const flags = flagged(own, FLAG_MIN, "none")
  const risk = a.risk
  const lines = [
    "",
    "## Decision-model pre-screen (a 4B model's hints, not findings — confirm or refute each flag first)",
    `- flags: ${flags.length ? flags.map(([l, p]) => `${l} p=${pct(p)}`).join(", ") : "none above threshold"}`,
    `- risk: ${risk.choice} p=${pct(risk.probabilities[risk.choice] ?? 0)}`,
  ]
  const pTrivial = risk.probabilities.trivial ?? 0
  const pRisky = risk.probabilities.risky ?? 0
  const anyFlag =
    flagged(a.neckbeard_flag, FLAG_MIN, "none").length + flagged(a.hoodie_flag, FLAG_MIN, "none").length > 0
  if (pTrivial >= TRIVIAL_MIN && !anyFlag)
    lines.push("- depth: treat this as a quick consultation (your Response Format's 2-3 sentence form).")
  else if (pRisky >= RISKY_MIN) lines.push("- depth: risk flagged — do a full review.")
  return lines.join("\n")
}

// ---------------------------------------------------------------- F2: shell output verdict

const VERDICT_QUESTIONS: Questions = {
  verdict: {
    type: "choice",
    criteria: {
      pass: "the command succeeded and its output shows no errors or failures",
      fail: "the command failed: errors, failing tests, crashes or panics",
      warnings: "the command succeeded but printed warnings worth reading",
      inconclusive: "the output does not show whether the command succeeded",
    },
  },
}

// Commands that only read; a verdict on them is noise
const READ_ONLY = /^\s*(cat|ls|ll|head|tail|less|grep|rg|ag|find|fd|tree|wc|file|stat|du|df|pwd|which|type|echo|printf|sed\s+-n|awk|jq|diff|git\s+(log|show|diff|status|blame|branch|remote|rev-parse|ls-files))\b/

function verdictState(command: string, exit: number, output: string): string {
  return (
    "A coding agent ran a shell command. Judge from the output whether it really succeeded.\n\n" +
    `$ ${cap(command, 2_000)}\nexit code: ${exit}\n\n${tail(output, OUTPUT_TAIL_CHARS)}`
  )
}

function verdictLine(a: Answers, exit: number): string {
  const [[top, p], second] = flagged(a.verdict, 0, "")
  const label = p >= VERDICT_MIN ? top : `unsure (leans ${top})`
  const runnerUp = second && second[1] >= RUNNER_UP_MIN ? `, ${second[0]} p=${pct(second[1])}` : ""
  return `[jev] verdict: ${label} p=${pct(p)}${runnerUp} (decision-model hint, not a review; exit=${exit})`
}

// ---------------------------------------------------------------- F3: shell risk escalation

const RISK_QUESTIONS: Questions = {
  shell_risk: {
    type: "choice",
    criteria: {
      routine:
        "routine work inside the project: build, test, run, read, search, create, edit, move or delete project " +
        "files, clean build output, install dependencies, curl a localhost or .lan service, " +
        "git add, commit, stash or checkout",
      destructive:
        "irreversible loss outside the project or of history: recursive delete of a home, system or root " +
        "directory, wiping a disk or database, git reset --hard, git clean, force push, deleting branches",
      network: "runs or fetches code from the internet: piping a downloaded script into a shell, downloading model or binary files, git push",
      system: "uses sudo, or changes system configuration, services, users, permissions or disks, or kills processes it did not start",
    },
  },
  // Asked in the same request; it only adds escalations (see the permission hook)
  reversibility: {
    type: "choice",
    criteria: {
      read_only: "only reads or inspects and changes nothing: ls, cat, grep, find without -delete, git status, git log, git diff",
      reversible:
        "changes that git, a rebuild or a reinstall can undo: edits, moves or deletes of project files or build " +
        "output (rm -rf node_modules, target/, dist/), git add, commit, stash, checkout, installing dependencies",
      irreversible:
        "destroys something nothing can restore: deletes in a home, system or root directory or of untracked user " +
        "data, find -delete or rm -rf outside the project, dd, mkfs, truncating files or databases, " +
        "git reset --hard, git clean -fd, git push --force",
    },
  },
}

// The model scores any `rm` as destructive (rm -rf target/ 0.94 vs rm -rf ~/x 0.96), so no
// threshold separates them. An rm whose every target is a plain relative path inside the working
// directory (no /, ~, $, .., globs at the top, or bare .) is decided here instead.
function inProjectRm(command: string): boolean {
  const words = command.trim().split(/\s+/)
  if (words[0] !== "rm" || /[;&|<>`]|\$\(/.test(command)) return false
  const paths = words.slice(1).filter((w) => !w.startsWith("-"))
  return (
    paths.length > 0 &&
    paths.every((p) => /^[\w@+.-][\w@+.\/-]*\/?$|^\.\/[\w@+.\/-]+$/.test(p) && !p.split("/").includes("..") && p !== "." && p !== "./")
  )
}

const riskState = (commands: readonly string[]) =>
  "A coding agent wants to run this shell command without asking the user.\n\n$ " + cap(commands.join("\n$ "), 4_000)

// ---------------------------------------------------------------- F4: ask_jev tool

// Tunables, set by the calibration run (README, F4: 32 Rust files, 4 questions, 8k/12k/24k chunks)
const ASK_CHUNK_CHARS = 8_000 // one request per chunk, ~2k tokens, ~0.6 s; best recall (19/20 vs 17/20 at 12k)
const ASK_OVERLAP_LINES = 20 // chunks overlap so a match on a boundary is seen whole
const ASK_MAX_FILES = 50 // per call; extra matches are counted, not checked
const ASK_FILE_MAX_CHARS = 200_000 // a bigger file is checked from the top and marked partial
const ASK_BUDGET_CHARS = 400_000 // per call, ~100k tokens, ~30 s
const ASK_TIMEOUT_MS = 15_000 // per request
const ASK_UNSURE_BELOW = 0.7 // a winning answer below this is reported as unsure; halves false yeses, keeps recall
const ASK_NO_COLLAPSE = 0.9 // confident "no" answers are folded into one line
const ASK_LEAN_MIN = 0.6 // custom options: note a part whose own answer differs at this level
const ASK_MAX_LINES = 60 // lines in the tool result

const ASK_DESCRIPTION =
  "Ask the jev decision model one question about files WITHOUT reading them into your context. " +
  "Each file is judged separately and you get one line per file: answer, probability, and the line range " +
  "that decided it. Use it to scout: which files are relevant, which contain something, which layer a file " +
  "belongs to, or what a long saved command output says (pass the path from a \"full output saved to\" notice). " +
  "One call checks up to 50 files: pass a glob or a directory (\"src/**/*.rs\") instead of listing files, and do not " +
  "split a set into batches or run several ask_jev calls in parallel; calls run one at a time anyway. " +
  "With `options`, phrase the question as a choice (\"Which of these best describes this file?\"), not as yes/no. " +
  "It is a fast 4B classifier, not a reader: it cannot quote, summarise or count, and it can be wrong. " +
  "Read a file before editing it, quoting it, or relying on an answer as proof. " +
  "If jev is unavailable the tool fails; then read the files instead."

const ASK_INPUT_SCHEMA = {
  type: "object",
  additionalProperties: false,
  required: ["paths", "question"],
  properties: {
    paths: {
      type: "array",
      minItems: 1,
      maxItems: 20,
      items: { type: "string", minLength: 1 },
      description:
        'Files, directories or globs ("src/**/*.ts"), relative to the project; one glob can cover the whole call ' +
        '(up to 50 files are checked). A saved tool-output path from a "full output saved to <path>" notice is also accepted.',
    },
    question: {
      type: "string",
      minLength: 3,
      maxLength: 300,
      description: 'One question, answered separately for each file, e.g. "Does this file spawn a subprocess?"',
    },
    options: {
      type: "object",
      minProperties: 2,
      maxProperties: 8,
      additionalProperties: { type: "string", minLength: 1, maxLength: 200 },
      description:
        'Optional answer choices as label -> what it means, e.g. {"http_handler": "defines HTTP routes or handlers", ' +
        '"domain": "business logic", "data": "database or storage access"}. Then ask "Which of these…?". ' +
        "Omit for a yes/no question.",
    },
  },
}

type Loc = { directory: string; project?: { directory?: string } }
type AskInput = { paths: string[]; question: string; options?: Record<string, string> }
type AskCall = { sessionID?: string; agent?: string; messageID?: string; id?: string; signal?: AbortSignal }
type Permission = { assert: (req: Record<string, unknown>) => Promise<unknown> }
type Target = { abs: string; resource: string; shown: string }
type Chunk = { text: string; from: number; to: number }
type Part = { probabilities: Record<string, number>; from: number; to: number; chars: number }
type Verdict = { path: string; label: string; p: number; range?: string; lean?: string; partial?: string }

const toolOutputDir = () =>
  join(process.env.XDG_DATA_HOME || join(homedir(), ".local", "share"), "opencode", "tool-output")

const within = (parent: string, child: string) => {
  const rel = relative(parent, child)
  return rel === "" || (rel !== ".." && !rel.startsWith(".." + sep) && !isAbsolute(rel))
}

// The read tool's rule (FileAccess.resolve): inside the directory or project the permission
// resource is the path relative to the directory, else the absolute path. Outside both, only the
// saved tool-output directory is accepted.
function resolvePath(loc: Loc, p: string): Target | null {
  const expanded = p === "~" ? homedir() : p.startsWith("~/") ? join(homedir(), p.slice(2)) : p
  const abs = resolve(loc.directory, expanded)
  const project = loc.project?.directory ? resolve(loc.project.directory) : undefined
  if (within(loc.directory, abs) || (project && project !== parse(project).root && within(project, abs))) {
    const rel = relative(loc.directory, abs).split(sep).join("/") || "."
    return { abs, resource: rel, shown: rel }
  }
  if (within(toolOutputDir(), abs)) return { abs, resource: abs.split(sep).join("/"), shown: abs }
  return null
}

const isGlob = (p: string) => /[*?[{]/.test(p)

// The same listing OpenCode's glob tool runs: .gitignore respected, hidden files and .git skipped
function rgFiles(cwd: string, args: string[], signal?: AbortSignal): Promise<string[]> {
  const argv = ["--no-config", "--files", ...args, "--glob=!**/.*", "--glob=!**/.git/**"]
  return new Promise((done, fail) => {
    execFile("rg", argv, { cwd, signal, timeout: 10_000, maxBuffer: 16 << 20 }, (err: any, stdout) => {
      // exit 1 with no output means "no files"
      if (err && !(err.code === 1 && !String(stdout).trim())) return fail(new Error(`rg: ${err.message}`))
      done(String(stdout).split("\n").filter(Boolean))
    })
  })
}

async function expandPaths(loc: Loc, paths: string[], signal?: AbortSignal) {
  const seen = new Map<string, Target>()
  const skipped: string[] = []
  for (const p of paths) {
    try {
      if (isGlob(p)) {
        if (isAbsolute(p) || p.startsWith("~")) {
          skipped.push(`${p} (use a glob relative to the project)`)
          continue
        }
        const found = await rgFiles(loc.directory, [`--glob=${p}`], signal)
        if (!found.length) skipped.push(`${p} (no files matched)`)
        for (const f of found) {
          const t = resolvePath(loc, f)
          if (t) seen.set(t.abs, t)
        }
        continue
      }
      const t = resolvePath(loc, p)
      if (!t) {
        skipped.push(`${p} (outside the project)`)
        continue
      }
      const st = await stat(t.abs).catch(() => null)
      if (!st) skipped.push(`${p} (not found)`)
      else if (st.isDirectory()) {
        const found = await rgFiles(t.abs, [], signal)
        if (!found.length) skipped.push(`${p} (no files in it)`)
        for (const f of found) {
          const ft = resolvePath(loc, join(t.abs, f))
          if (ft) seen.set(ft.abs, ft)
        }
      } else seen.set(t.abs, t)
    } catch (err) {
      if (signal?.aborted) throw err
      skipped.push(`${p} (${err instanceof Error ? err.message : String(err)})`)
    }
  }
  const all = [...seen.values()].sort((a, b) => a.shown.localeCompare(b.shown))
  return { files: all.slice(0, ASK_MAX_FILES), more: Math.max(0, all.length - ASK_MAX_FILES), skipped }
}

// Split at line breaks into ~size-char chunks with `overlap` lines shared between neighbours.
// A single line longer than `size` (minified code) is cut into pieces that keep its line number.
function chunkFile(text: string, size = ASK_CHUNK_CHARS, overlap = ASK_OVERLAP_LINES): Chunk[] {
  const lines: { text: string; no: number }[] = []
  text.split("\n").forEach((line, i) => {
    if (line.length <= size) lines.push({ text: line, no: i + 1 })
    else for (let j = 0; j < line.length; j += size) lines.push({ text: line.slice(j, j + size), no: i + 1 })
  })
  const chunks: Chunk[] = []
  let start = 0
  while (start < lines.length) {
    let end = start
    let chars = 0
    while (end < lines.length && (end === start || chars + lines[end].text.length + 1 <= size)) {
      chars += lines[end].text.length + 1
      end++
    }
    chunks.push({ text: lines.slice(start, end).map((l) => l.text).join("\n"), from: lines[start].no, to: lines[end - 1].no })
    if (end >= lines.length) break
    start = Math.max(end - overlap, start + 1)
  }
  return chunks
}

const yesNoCriteria = (): Criteria => ({
  yes: "this part of the file shows the answer to the question is yes",
  no: "the answer is no, or this part of the file has nothing to do with the question",
})

function askState(question: string, shown: string, c: Chunk, lines: number, k: number, m: number): string {
  return (
    "A coding agent asks a question about one file. Answer from this part of the file only.\n\n" +
    `Question: ${question}\n\nFile: ${shown} (lines ${c.from}-${c.to} of ${lines}, part ${k} of ${m})\n\n${c.text}`
  )
}

// yes/no: a "yes" anywhere counts (max over parts). Custom options: length-weighted mean.
function combine(parts: Part[], yesNo: boolean, labels: string[]): Omit<Verdict, "path" | "partial"> {
  if (yesNo) {
    const best = parts.reduce((a, b) => ((b.probabilities.yes ?? 0) > (a.probabilities.yes ?? 0) ? b : a))
    const pYes = best.probabilities.yes ?? 0
    return pYes >= 0.5 ? { label: "yes", p: pYes, range: `lines ${best.from}-${best.to}` } : { label: "no", p: 1 - pYes }
  }
  const total = parts.reduce((s, x) => s + x.chars, 0) || 1
  const mean: Record<string, number> = {}
  for (const l of labels) mean[l] = parts.reduce((s, x) => s + (x.probabilities[l] ?? 0) * x.chars, 0) / total
  const [label, p] = Object.entries(mean).sort((a, b) => b[1] - a[1])[0]
  let lean: string | undefined
  let leanP = ASK_LEAN_MIN
  for (const x of parts) {
    const [own, op] = Object.entries(x.probabilities).sort((a, b) => b[1] - a[1])[0] ?? ["", 0]
    if (own !== label && op >= leanP) {
      leanP = op
      lean = `lines ${x.from}-${x.to} lean ${own} ${pct(op)}`
    }
  }
  return { label, p, lean }
}

type AskSummary = {
  question: string
  labels: string[]
  yesNo: boolean
  verdicts: Verdict[]
  skipped: string[]
  more: number
  notChecked: string[]
  stopped?: string
  files: number
  chars: number
  tokens: number
  ms: number
}

function formatAnswers(s: AskSummary): string {
  const kind = s.yesNo ? "yes/no" : `options: ${s.labels.join(" / ")}`
  const head =
    `ask_jev: "${s.question}" (${kind}) · ${s.files} file${s.files === 1 ? "" : "s"} · ` +
    `${Math.round(s.chars / 1000)}k chars · ${s.tokens.toLocaleString("en-US")} tokens judged in ${(s.ms / 1000).toFixed(1)} s`
  const line = (v: Verdict) => {
    const label = v.p < ASK_UNSURE_BELOW ? `unsure (leans ${v.label})` : v.label
    const extra = [v.range, v.lean, v.partial].filter(Boolean).join(" · ")
    return `${label} ${pct(v.p)}  ${v.path}${extra ? `  (${extra})` : ""}`
  }
  const sure = s.verdicts.filter((v) => v.p >= ASK_UNSURE_BELOW)
  const unsure = s.verdicts.filter((v) => v.p < ASK_UNSURE_BELOW).sort((a, b) => b.p - a.p)
  const body: string[] = []
  if (s.yesNo) {
    body.push(...sure.filter((v) => v.label === "yes").sort((a, b) => b.p - a.p).map(line))
    body.push(...unsure.map(line))
    const no = sure.filter((v) => v.label === "no")
    body.push(...no.filter((v) => v.p < ASK_NO_COLLAPSE || v.partial).sort((a, b) => a.p - b.p).map(line))
    const folded = no.filter((v) => v.p >= ASK_NO_COLLAPSE && !v.partial)
    if (folded.length) {
      const names = folded.slice(0, 10).map((v) => v.path).join(", ")
      body.push(
        `no  ${folded.length} file${folded.length === 1 ? "" : "s"}, all p≥${pct(ASK_NO_COLLAPSE)}: ${names}` +
          (folded.length > 10 ? `, … (+${folded.length - 10})` : ""),
      )
    }
  } else {
    for (const l of s.labels) body.push(...sure.filter((v) => v.label === l).sort((a, b) => b.p - a.p).map(line))
    body.push(...unsure.map(line))
  }
  const budget = ASK_MAX_LINES - 6
  const shown = body.length > budget ? [...body.slice(0, budget), `… ${body.length - budget} more lines`] : body
  const tailLines: string[] = []
  if (s.stopped) tailLines.push(`stopped early: ${s.stopped}`)
  if (s.notChecked.length)
    tailLines.push(
      `not checked (${s.stopped ? "stopped" : "budget; ask again with these paths"}): ${s.notChecked.slice(0, 10).join(", ")}` +
        (s.notChecked.length > 10 ? `, … (+${s.notChecked.length - 10})` : ""),
    )
  if (s.more) tailLines.push(`+${s.more} more files matched; narrow the paths to check them`)
  if (s.skipped.length)
    tailLines.push(`skipped: ${s.skipped.slice(0, 15).join(" · ")}${s.skipped.length > 15 ? ` · … (+${s.skipped.length - 15})` : ""}`)
  tailLines.push("jev answers are a 4B classifier's guesses: read a file before editing it, quoting it, or relying on the answer.")
  return [head, ...shown, ...tailLines].join("\n")
}

// The whole tool call. Throws only when nothing could be answered (the agent then reads instead)
async function askFiles(input: AskInput, call: AskCall, loc: Loc, permission: Permission) {
  const t0 = Date.now()
  const signal = call.signal
  const question = input.question.trim()
  const yesNo = !input.options
  const criteria: Criteria = input.options ?? yesNoCriteria()
  const labels = Object.keys(criteria)
  const questions: Questions = { answer: { type: "choice", criteria } }

  const { files, more, skipped } = await expandPaths(loc, input.paths, signal)
  const verdicts: Verdict[] = []
  const notChecked: string[] = []
  const msPer: number[] = []
  let firstState: string | undefined
  let stopped: string | undefined
  let chars = 0
  let tokens = 0
  let checkedFiles = 0

  for (const f of files) {
    if (stopped) {
      notChecked.push(f.shown)
      continue
    }
    if (signal?.aborted) throw new Error("aborted")
    try {
      const source = call.messageID && call.id ? { type: "tool", messageID: call.messageID, id: call.id } : undefined
      await permission.assert({
        action: "read",
        resources: [f.resource],
        save: ["*"],
        ...(call.sessionID ? { sessionID: call.sessionID } : {}),
        ...(call.agent ? { agent: call.agent } : {}),
        ...(source ? { source } : {}),
      })
    } catch {
      if (signal?.aborted) throw new Error("aborted")
      skipped.push(`${f.shown} (denied by permissions)`)
      continue
    }
    const buf = await readFile(f.abs).catch(() => null)
    if (!buf) {
      skipped.push(`${f.shown} (unreadable)`)
      continue
    }
    if (buf.subarray(0, 8192).includes(0)) {
      skipped.push(`${f.shown} (binary)`)
      continue
    }
    let text = buf.toString("utf8")
    const totalLines = text.split("\n").length
    let partial: string | undefined
    const room = Math.min(ASK_FILE_MAX_CHARS, ASK_BUDGET_CHARS - chars)
    if (room <= 0 || (text.length > room && checkedFiles > 0 && room < ASK_CHUNK_CHARS)) {
      notChecked.push(f.shown)
      continue
    }
    if (text.length > room) {
      text = text.slice(0, text.lastIndexOf("\n", room) > 0 ? text.lastIndexOf("\n", room) : room)
      partial = `partial: lines 1-${text.split("\n").length} of ${totalLines}`
    }
    const chunks = chunkFile(text)
    const parts: Part[] = []
    for (const [k, c] of chunks.entries()) {
      const state = askState(question, f.shown, c, totalLines, k + 1, chunks.length)
      firstState ??= state
      const r = await query(state, questions, ASK_TIMEOUT_MS, { signal, lane: "tool" })
      if (signal?.aborted) throw new Error("aborted")
      if (!r.answers) {
        stopped = `${r.trace.error ?? "no answer"} (at ${f.shown}, lines ${c.from}-${c.to})`
        break
      }
      msPer.push(r.trace.ms)
      const usage = (r.trace.response as { usage?: { input_tokens?: number } })?.usage
      tokens += typeof usage?.input_tokens === "number" ? usage.input_tokens : 0
      chars += c.text.length
      parts.push({ probabilities: r.answers.answer.probabilities, from: c.from, to: c.to, chars: c.text.length })
    }
    if (stopped && parts.length < chunks.length) {
      if (parts.length) partial = `partial: lines 1-${parts[parts.length - 1].to} of ${totalLines}`
      else {
        notChecked.push(f.shown)
        continue
      }
    }
    checkedFiles++
    verdicts.push({ path: f.shown, ...combine(parts, yesNo, labels), partial })
  }

  if (!verdicts.length && stopped) throw new Error(`jev is unavailable (${stopped}); read the files instead`)
  const ms = Date.now() - t0
  const summary: AskSummary = {
    question,
    labels,
    yesNo,
    verdicts,
    skipped,
    more,
    notChecked,
    stopped,
    files: checkedFiles,
    chars,
    tokens,
    ms,
  }
  let content = verdicts.length || files.length ? formatAnswers(summary) : `ask_jev: no files to check.\nskipped: ${skipped.join(" · ")}`
  if (tracing() && firstState) {
    const trace: Trace = {
      request: { state: firstState, questions },
      response: { requests: msPer.length, tokens, ms_per_request: msPer, stopped },
      ms,
    }
    content += "\n" + traceBlock("F4 ask_jev (first request shown)", trace)
  }
  return { content, metadata: { files: checkedFiles, chars, tokens, ms, question } }
}

// ---------------------------------------------------------------- hooks

function guard<E>(name: string, fn: (e: E) => Promise<void>) {
  return async (e: E) => {
    try {
      await fn(e)
    } catch (err) {
      log(`${name} hook error, passing through: ${err instanceof Error ? err.stack ?? err.message : String(err)}`)
    }
  }
}

export default {
  id: "local-mind.jev",
  setup: async (ctx: any) => {
    if (DISABLED) {
      log("JEV_DISABLE=1, not registering hooks")
      return
    }
    log(`decision model at ${JEV_URL}`)

    await ctx.tool.hook(
      "execute.before",
      guard("subagent", async (e: any) => {
        if (e.tool !== "subagent") return
        const input = e.input
        if (!input || typeof input.prompt !== "string" || !REVIEWERS.has(input.agent)) return
        if (input.sessionID) return // a follow-up to a running review: leave it alone
        const state = reviewState(input.prompt)
        const r = await cached(hash(state), () => query(state, REVIEW_QUESTIONS, REVIEW_TIMEOUT_MS))
        const prompt = r.answers ? input.prompt + "\n" + preScreen(input.agent, r.answers) : input.prompt
        if (tracing() && e.id !== undefined) {
          const callID = e.id // execute.before and execute.after carry the same tool call id
          park({ label: `F1 reviewer pre-screen (${input.agent})`, trace: r.trace, match: (ev) => ev?.id === callID })
        }
        if (r.answers) e.input = { ...input, prompt }
      }),
    )

    // F1 traces: shown on the reviewer's result, so the reviewer never reads them
    await ctx.tool.hook(
      "execute.after",
      guard("subagent trace", async (e: any) => {
        if (e.tool !== "subagent" || !e.result || !waiting.length) return
        const blocks = collect(e)
        if (blocks.length) e.result = withText(e.result, blocks.join("\n"))
      }),
    )

    await ctx.tool.hook(
      "execute.after",
      guard("shell", async (e: any) => {
        if (e.tool !== "shell" || e.status !== "completed") return
        const out = e.result?.output
        if (!out || out.status === "running" || typeof out.output !== "string") return
        const command = typeof e.input?.command === "string" ? e.input.command : ""
        const lines: string[] = []
        const traces = waiting.length ? collect(e) : [] // F3, from the permission check
        // a non-zero exit already says it failed; short or read-only output is not worth a verdict
        if (out.exit === 0 && out.output.length >= MIN_OUTPUT_CHARS && !READ_ONLY.test(command)) {
          const r = await query(verdictState(command, out.exit, out.output), VERDICT_QUESTIONS)
          if (r.answers) lines.push(verdictLine(r.answers, out.exit))
          if (tracing()) traces.push(traceBlock("F2 shell verdict", r.trace))
        }
        lines.push(...traces)
        if (lines.length) e.result = withText(e.result, lines.join("\n"))
      }),
    )

    await ctx.permission.hook(
      "evaluate",
      guard("permission", async (e: any) => {
        if (e.action !== "shell" || e.effect !== "allow" || !e.resources?.length) return
        const judged = e.resources.filter((r: string) => !inProjectRm(r))
        if (!judged.length) return
        const r = await query(riskState(judged), RISK_QUESTIONS)
        if (tracing())
          park({
            label: "F3 shell risk",
            trace: r.trace,
            // the permission event has no tool call id; match the shell call by its command text
            match: (ev) =>
              ev?.tool === "shell" &&
              typeof ev.input?.command === "string" &&
              judged.every((c: string) => ev.input.command.includes(c)),
          })
        const a = r.answers
        if (!a) return
        // shell_risk decides first and keeps its labels; irreversible alone can only add an escalation
        const hits = flagged(a.shell_risk, ESCALATE_MIN, "routine")
        const pIrreversible = a.reversibility.probabilities.irreversible ?? 0
        if (!hits.length && pIrreversible >= IRREVERSIBLE_MIN) hits.push(["irreversible", pIrreversible])
        if (!hits.length) return
        const [label, p] = hits[0]
        e.effect = "ask"
        e.message = `jev flagged this command as ${label} (p=${pct(p)}); confirm to run it`
        log(`escalated to ask: ${label} p=${pct(p)}: ${e.resources.join(" ; ").slice(0, 200)}`)
      }),
    )

    // F4. Registered last and fenced off, so a problem here never costs F1-F3. Without a
    // permission check the tool would read past the agents' read rules, so no check, no tool.
    try {
      const loc = ctx.location as Loc | undefined
      if (typeof ctx.tool?.transform !== "function" || typeof ctx.permission?.assert !== "function" || !loc?.directory) {
        log("ask_jev not registered: this OpenCode has no tool.transform, permission.assert or location.directory")
        return
      }
      await ctx.tool.transform((tools: any) => {
        tools.add({
          name: "ask_jev",
          description: ASK_DESCRIPTION,
          input: ASK_INPUT_SCHEMA,
          options: { codemode: false, permission: "read" },
          execute: (input: AskInput, call: AskCall) => askFiles(input, call, loc, ctx.permission),
        })
      })
      log(`ask_jev tool registered for ${loc.directory}`)
    } catch (err) {
      log(`ask_jev not registered: ${err instanceof Error ? err.message : String(err)}`)
    }
  },
}

// For the calibration script only; the plugin loader reads `default` alone
export const __test = {
  ask,
  query,
  askFiles,
  expandPaths,
  resolvePath,
  chunkFile,
  combine,
  formatAnswers,
  yesNoCriteria,
  toolOutputDir,
  downUntil: () => downUntil,
  tracing,
  traceBlock,
  park,
  collect,
  withText,
  TRACE_FILE,
  cap,
  reviewState,
  preScreen,
  verdictState,
  verdictLine,
  riskState,
  inProjectRm,
  flagged,
  REVIEW_QUESTIONS,
  VERDICT_QUESTIONS,
  RISK_QUESTIONS,
  IRREVERSIBLE_MIN,
  ESCALATE_MIN,
  READ_ONLY,
}
