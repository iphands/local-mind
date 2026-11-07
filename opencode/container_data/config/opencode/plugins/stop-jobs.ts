// stop-jobs: one OpenCode command, /stop-all-jobs, that stops every running session and
// background subagent this OpenCode process is running, so nothing is left to resume.
//
//   /stop-all-jobs        stop everything (except the session you type it in)
//   /stop-all-jobs list   dry run: show what would be stopped
//
// Why: a prompt that fans out to many background subagents (9 at once on 2026-11-06) can swamp
// basalt, and restarting OpenCode does not help. Running sessions keep session_v2.time_suspended
// set and background subagents keep job rows with status 'running', and `opencode serve` resumes
// both at startup.
//
// How:
// - Candidates are read READ-ONLY from opencode.db: sessions with time_suspended set (what
//   SessionStore.claim marks while a session runs; what startup resumes) and the parent and child
//   sessions of every job with status 'running'. The command never writes to the database.
// - Each is stopped with ctx.session.interrupt (the server's own "interrupt active execution
//   owned by this process"): subagents first, then parents. An interrupted subagent's job
//   completes and notifies its parent, which can wake it, so this repeats until a pass finds
//   nothing new (at most PASSES times).
// - The result goes into the invoking session as a synthetic message with resume: false, so it
//   shows in the TUI without starting a model turn.
// - Anything still marked but not running in this process (a stale flag left by a crash) is
//   reported, not touched: clearing it needs OpenCode stopped (see plugins/README.md).
//
// Env: STOP_JOBS_DB overrides the database path (default $XDG_DATA_HOME/opencode/opencode.db).

import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"

const PASSES = 5
const PASS_GAP_MS = 1_500
const SHOW_TITLES = 12

const log = (msg: string) => console.warn(`[stop-jobs] ${msg}`)

const dbPath = () =>
  process.env.STOP_JOBS_DB || join(process.env.XDG_DATA_HOME || join(homedir(), ".local", "share"), "opencode", "opencode.db")

type Row = Record<string, any>
type Db = { all: (sql: string) => Row[]; close: () => void }

// bun:sqlite inside OpenCode (Bun); node:sqlite in the Node test harness
async function openReadOnly(path: string): Promise<Db> {
  try {
    const mod: any = await import("bun:sqlite")
    const db = new mod.Database(path, { readonly: true })
    return { all: (sql) => db.query(sql).all(), close: () => db.close() }
  } catch (bunErr) {
    const mod: any = await import("node:sqlite")
    const db = new mod.DatabaseSync(path, { readOnly: true })
    return { all: (sql) => db.prepare(sql).all(), close: () => db.close() }
  }
}

type Candidate = { id: string; parent: string | null; title: string; directory: string; viaJob: boolean }

// Everything that would resume at the next start, or is running now
async function readCandidates(path: string): Promise<Map<string, Candidate>> {
  const db = await openReadOnly(path)
  try {
    const found = new Map<string, Candidate>()
    const info = new Map<string, Row>()
    for (const r of db.all("SELECT id, parent_id, title, directory FROM session_v2")) info.set(r.id, r)
    const add = (id: string | null, viaJob: boolean) => {
      if (!id) return
      const r = info.get(id)
      const prev = found.get(id)
      found.set(id, {
        id,
        parent: r?.parent_id ?? null,
        title: r?.title ?? "(unknown session)",
        directory: r?.directory ?? "",
        viaJob: viaJob || prev?.viaJob === true,
      })
    }
    for (const r of db.all("SELECT id FROM session_v2 WHERE time_suspended IS NOT NULL")) add(r.id, false)
    for (const r of db.all("SELECT session_id, child_session_id FROM job WHERE status = 'running'")) {
      add(r.child_session_id, true)
      add(r.session_id, false)
    }
    return found
  } finally {
    db.close()
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

const names = (cs: Candidate[]) =>
  cs.slice(0, SHOW_TITLES).map((c) => `"${c.title}"`).join(", ") + (cs.length > SHOW_TITLES ? `, … (+${cs.length - SHOW_TITLES})` : "")

function describe(cs: Candidate[]): string {
  const subs = cs.filter((c) => c.parent || c.viaJob).length
  return `${cs.length} session${cs.length === 1 ? "" : "s"} (${subs} subagent${subs === 1 ? "" : "s"})`
}

async function stopAll(ctx: any, invoking: string | undefined, dryRun: boolean): Promise<string> {
  const path = dbPath()
  if (!existsSync(path)) return `/stop-all-jobs: no OpenCode database at ${path}; nothing to do.`
  const read = async () => {
    const all = await readCandidates(path)
    if (invoking) all.delete(invoking)
    return all
  }

  const first = await read()
  if (dryRun) {
    if (!first.size) return "/stop-all-jobs list: nothing is running or waiting to resume."
    const lines = [...first.values()]
      .sort((a, b) => Number(!!b.parent) - Number(!!a.parent))
      .map((c) => `- ${c.parent || c.viaJob ? "subagent" : "session "}  ${c.title}  (${c.id}${c.directory ? `, ${c.directory}` : ""})`)
    return [`/stop-all-jobs list: would stop ${describe([...first.values()])}:`, ...lines].join("\n")
  }

  const stopped = new Map<string, Candidate>()
  const notHere = new Map<string, Candidate>() // never running in this process (interrupt said no)
  const errors: string[] = []
  let passes = 0
  let busy = false // the last pass still stopped something
  let current = first
  // Each pass interrupts everything still marked, including sessions stopped in an earlier pass:
  // a parent can be woken again by its subagent's completion notice. It ends when a pass stops
  // nothing (all that's left is idle or stale) or after PASSES passes.
  while (passes < PASSES) {
    const todo = [...current.values()].filter((c) => !notHere.has(c.id))
    if (!todo.length) break
    passes++
    busy = false
    // subagents first, so their parents are stopped after any completion notice wakes them
    todo.sort((a, b) => Number(!!b.parent) - Number(!!a.parent))
    for (const c of todo) {
      try {
        const r = await ctx.session.interrupt({ sessionID: c.id, resume: false })
        if (r?.interrupted === false) {
          if (!stopped.has(c.id)) notHere.set(c.id, c)
        } else {
          stopped.set(c.id, c)
          busy = true
        }
      } catch (err) {
        errors.push(`${c.title}: ${err instanceof Error ? err.message : String(err)}`)
        if (!stopped.has(c.id)) notHere.set(c.id, c)
      }
    }
    await sleep(PASS_GAP_MS)
    current = await read()
    if (!busy) break
  }

  // Whatever is still marked now would resume at the next start
  const stillMarked = [...current.values()]
  const stale = busy ? stillMarked.filter((c) => notHere.has(c.id)) : stillMarked
  const lines: string[] = []
  if (!stopped.size && !stale.length) lines.push("/stop-all-jobs: nothing was running.")
  if (stopped.size)
    lines.push(
      `/stop-all-jobs: stopped ${describe([...stopped.values()])} in ${passes} pass${passes === 1 ? "" : "es"}: ` +
        names([...stopped.values()]),
    )
  if (stale.length)
    lines.push(
      `Still marked but not running in this OpenCode process (${stale.length}): ${names(stale)}. These are stale flags ` +
        "(e.g. left by a crash) and would resume at the next start: stop the opencode container and clear them " +
        "(plugins/README.md, /stop-all-jobs section).",
    )
  const leftover = busy ? stillMarked.filter((c) => !notHere.has(c.id)) : []
  if (leftover.length)
    lines.push(`Still running after ${passes} passes (${leftover.length}): ${names(leftover)}. Run /stop-all-jobs again.`)
  if (errors.length) lines.push(`Errors: ${errors.slice(0, 5).join("; ")}`)
  return lines.join("\n")
}

export default {
  id: "local-mind.stop-jobs",
  setup: async (ctx: any) => {
    if (typeof ctx.command?.transform !== "function" || typeof ctx.session?.interrupt !== "function") {
      log("not registered: this OpenCode has no command.transform or session.interrupt")
      return
    }
    await ctx.command.transform((commands: any) => {
      commands.add({
        name: "stop-all-jobs",
        description: "stop every running session and background subagent (`list` for a dry run)",
        execute: async (invocation: any) => {
          const sessionID: string | undefined = invocation?.sessionID
          const dryRun = /^\s*(list|ls|dry-?run)\b/i.test(invocation?.prompt?.text ?? "")
          let text: string
          try {
            text = await stopAll(ctx, sessionID, dryRun)
          } catch (err) {
            text = `/stop-all-jobs failed: ${err instanceof Error ? err.message : String(err)}`
          }
          log(text.split("\n")[0])
          if (sessionID) {
            try {
              await ctx.session.synthetic({ sessionID, text, resume: false })
            } catch (err) {
              log(`could not report in the session: ${err instanceof Error ? err.message : String(err)}`)
            }
          }
        },
      })
    })
    log("registered /stop-all-jobs")
  },
}

// For the test harness only; the plugin loader reads `default`
export const __test = { readCandidates, stopAll, dbPath }
