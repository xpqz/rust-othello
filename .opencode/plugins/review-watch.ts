import type { Plugin, PluginInput } from "@opencode-ai/plugin"
import type { Event, Session, SessionStatus } from "@opencode-ai/sdk"
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  watch,
  writeFileSync,
} from "node:fs"
import { join } from "node:path"

type Client = PluginInput["client"]
type State = Record<string, number>

const debounceMs = 1500

function loadState(path: string): State {
  try {
    const value: unknown = JSON.parse(readFileSync(path, "utf8"))
    if (!value || typeof value !== "object" || Array.isArray(value)) return {}
    const state: State = {}
    for (const [key, entry] of Object.entries(value)) {
      if (typeof entry === "number") state[key] = entry
    }
    return state
  } catch {
    return {}
  }
}

function saveState(path: string, state: State) {
  writeFileSync(path, JSON.stringify(state, null, 2) + "\n")
}

function mtime(path: string): number | undefined {
  try {
    return statSync(path).mtimeMs
  } catch {
    return undefined
  }
}

function reviews(dir: string): string[] {
  try {
    return readdirSync(dir).filter((name) => /^\d+\.md$/.test(name))
  } catch {
    return []
  }
}

async function sessions(client: Client, directory: string): Promise<Session[]> {
  try {
    const result = await client.session.list({ query: { directory } })
    return result.data ?? []
  } catch {
    return []
  }
}

async function statuses(
  client: Client,
  directory: string,
): Promise<Record<string, SessionStatus>> {
  try {
    const result = await client.session.status({ query: { directory } })
    return result.data ?? {}
  } catch {
    return {}
  }
}

export const ReviewWatch: Plugin = async ({ client, directory }) => {
  const reviewDir = join(directory, "docs", "reviews")
  const statePath = join(directory, ".opencode", "review-watch-state.json")
  mkdirSync(reviewDir, { recursive: true })
  mkdirSync(join(directory, ".opencode"), { recursive: true })

  const state = loadState(statePath)
  const seen = new Map<string, number>()
  const pending = new Map<string, number>()
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  const children = new Set<string>()
  let active: string | undefined
  let flushing = false

  for (const name of reviews(reviewDir)) {
    const id = name.slice(0, -3)
    const current = mtime(join(reviewDir, name))
    if (current === undefined) continue
    const prior = state[id]
    if (prior === undefined) {
      state[id] = current
      seen.set(id, current)
    } else {
      seen.set(id, prior)
      if (current > prior) pending.set(id, current)
    }
  }
  saveState(statePath, state)

  for (const session of await sessions(client, directory)) {
    if (session.parentID) children.add(session.id)
  }

  async function idleSession(): Promise<string | undefined> {
    const list = (await sessions(client, directory)).filter((s) => !s.parentID)
    const known = await statuses(client, directory)
    const current = active ? list.find((s) => s.id === active) : undefined
    if (active && !current) active = undefined
    if (current) {
      const status = known[current.id]
      if (status && status.type !== "idle") return undefined
      return current.id
    }
    const idle = list
      .filter((s) => {
        const status = known[s.id]
        return !status || status.type === "idle"
      })
      .sort((a, b) => b.time.updated - a.time.updated)
    return idle[0]?.id
  }

  async function flush() {
    if (flushing || pending.size === 0) return
    flushing = true
    let success = false
    const batch = [...pending.entries()]
    try {
      const id = await idleSession()
      if (!id) return
      const files = batch
        .map(([review]) => `docs/reviews/${review}.md`)
        .join(", ")
      const result = await client.session.prompt({
        path: { id },
        query: { directory },
        body: {
          parts: [
            {
              type: "text",
              text: `Review(s) ready: ${files}. Read them, address the findings, and continue.`,
            },
          ],
        },
      })
      if (result.error) throw result.error
      for (const [review, stamp] of batch) {
        if (mtime(join(reviewDir, `${review}.md`)) !== stamp) continue
        pending.delete(review)
        seen.set(review, stamp)
        state[review] = stamp
      }
      saveState(statePath, state)
      success = true
    } catch (error) {
      console.error("review-watch: unable to prompt", error)
    } finally {
      flushing = false
    }
    if (success && pending.size > 0) void flush()
  }

  function schedule(name: string) {
    const old = timers.get(name)
    if (old) clearTimeout(old)
    timers.set(
      name,
      setTimeout(() => {
        timers.delete(name)
        const id = name.slice(0, -3)
        const current = mtime(join(reviewDir, name))
        if (current === undefined) return
        if (current === seen.get(id) || pending.get(id) === current) return
        pending.set(id, current)
        void flush()
      }, debounceMs),
    )
  }

  function scan() {
    for (const name of reviews(reviewDir)) {
      const id = name.slice(0, -3)
      const current = mtime(join(reviewDir, name))
      if (current === undefined) continue
      if (current === seen.get(id) || pending.get(id) === current) continue
      schedule(name)
    }
  }

  const watcher = watch(reviewDir, { persistent: true }, () => scan())
  const poll = setInterval(scan, 2000)

  if (pending.size > 0) void flush()

  async function onEvent(event: Event) {
    if (event.type === "session.status") {
      if (children.has(event.properties.sessionID)) return
      active = event.properties.sessionID
      if (event.properties.status.type === "idle") await flush()
    } else if (event.type === "session.idle") {
      if (children.has(event.properties.sessionID)) return
      active = event.properties.sessionID
      await flush()
    } else if (event.type === "session.created") {
      if (event.properties.info.parentID) {
        children.add(event.properties.info.id)
      } else {
        active = event.properties.info.id
      }
    } else if (event.type === "session.updated") {
      if (event.properties.info.parentID) {
        children.add(event.properties.info.id)
      } else {
        active = event.properties.info.id
      }
    } else if (event.type === "session.deleted") {
      children.delete(event.properties.info.id)
      if (active === event.properties.info.id) active = undefined
    }
  }

  return {
    event: async ({ event }) => {
      await onEvent(event)
    },
    dispose: async () => {
      watcher.close()
      clearInterval(poll)
      for (const timer of timers.values()) clearTimeout(timer)
    },
  }
}
