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
//
// Fail-open: any error, timeout or bad answer leaves the call exactly as it was. After one
// failure every call is skipped for COOLDOWN_MS. Hooks never throw: a throw from execute.before
// blocks the tool call, and a rejection from the other hooks is a defect, not a tool error.
//
// API notes (probed against llama.cpp b12169):
//   - extra keys such as `question` are ignored, so the question has to live in `state`
//   - each question re-prefills the whole state (~4k tok/s on the 4060): pack a checklist into
//     one `choice` rather than one question per item
//   - `choice` criteria descriptions are tokenized and carry the meaning of each option
//
// Env: JEV_URL (default http://cosmo.lan:8701), JEV_DISABLE=1 turns every feature off.

const JEV_URL = (process.env.JEV_URL ?? "http://cosmo.lan:8701").replace(/\/+$/, "")
const DISABLED = process.env.JEV_DISABLE === "1"

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
const CACHE_MAX = 32

type Criteria = Record<string, string>
type Questions = Record<string, { type: "choice"; criteria: Criteria }>
type Choice = { choice: string; probabilities: Record<string, number> }
type Answers = Record<string, Choice>

const log = (msg: string) => console.warn(`[jev] ${msg}`)

// ---------------------------------------------------------------- decision-model client

let downUntil = 0

async function ask(state: string, questions: Questions, timeoutMs = TIMEOUT_MS): Promise<Answers | null> {
  if (DISABLED || Date.now() < downUntil) return null
  try {
    const res = await fetch(`${JEV_URL}/v1/systemone`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ state, questions }),
      signal: AbortSignal.timeout(timeoutMs),
    })
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${(await res.text()).slice(0, 200)}`)
    const body = (await res.json()) as { answers?: Record<string, Partial<Choice>> }
    const answers: Answers = {}
    for (const id of Object.keys(questions)) {
      const a = body.answers?.[id]
      if (!a || typeof a.choice !== "string" || typeof a.probabilities !== "object" || !a.probabilities)
        throw new Error(`malformed answer for ${id}`)
      answers[id] = { choice: a.choice, probabilities: a.probabilities }
    }
    return answers
  } catch (err) {
    downUntil = Date.now() + COOLDOWN_MS
    log(`unavailable (${err instanceof Error ? err.message : String(err)}); skipping for ${COOLDOWN_MS / 1000}s`)
    return null
  }
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
// single request. Failed (null) results are dropped so the next call can retry after cooldown.
const cache = new Map<string, Promise<Answers | null>>()
function cached(key: string, run: () => Promise<Answers | null>): Promise<Answers | null> {
  const hit = cache.get(key)
  if (hit) return hit
  const p = run().then((v) => {
    if (v === null) cache.delete(key)
    return v
  })
  cache.set(key, p)
  while (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value as string)
  return p
}

const pct = (p: number) => p.toFixed(2)

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
        const a = await cached(hash(state), () => ask(state, REVIEW_QUESTIONS, REVIEW_TIMEOUT_MS))
        if (!a) return
        e.input = { ...input, prompt: input.prompt + "\n" + preScreen(input.agent, a) }
      }),
    )

    await ctx.tool.hook(
      "execute.after",
      guard("shell", async (e: any) => {
        if (e.tool !== "shell" || e.status !== "completed") return
        const out = e.result?.output
        if (!out || out.status === "running" || typeof out.output !== "string") return
        if (out.exit !== 0) return // a non-zero exit already says it failed
        if (out.output.length < MIN_OUTPUT_CHARS) return
        const command = typeof e.input?.command === "string" ? e.input.command : ""
        if (READ_ONLY.test(command)) return
        const a = await ask(verdictState(command, out.exit, out.output), VERDICT_QUESTIONS)
        if (!a) return
        const line = verdictLine(a, out.exit)
        const content = e.result.content
        e.result = {
          ...e.result,
          content:
            typeof content === "string"
              ? `${content}\n${line}`
              : [...(Array.isArray(content) ? content : []), { type: "text", text: line }],
        }
      }),
    )

    await ctx.permission.hook(
      "evaluate",
      guard("permission", async (e: any) => {
        if (e.action !== "shell" || e.effect !== "allow" || !e.resources?.length) return
        const judged = e.resources.filter((r: string) => !inProjectRm(r))
        if (!judged.length) return
        const a = await ask(riskState(judged), RISK_QUESTIONS)
        if (!a) return
        const hits = flagged(a.shell_risk, ESCALATE_MIN, "routine")
        if (!hits.length) return
        const [label, p] = hits[0]
        e.effect = "ask"
        e.message = `jev flagged this command as ${label} (p=${pct(p)}); confirm to run it`
        log(`escalated to ask: ${label} p=${pct(p)}: ${e.resources.join(" ; ").slice(0, 200)}`)
      }),
    )
  },
}

// For the calibration script only; the plugin loader reads `default` alone
export const __test = {
  ask,
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
  READ_ONLY,
}
