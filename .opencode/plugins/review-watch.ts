import type { Plugin, PluginInput } from "@opencode-ai/plugin"
import type { Event, Session, SessionStatus } from "@opencode-ai/sdk"
import { mkdirSync, readFileSync, readdirSync, watch } from "node:fs"
import { join } from "node:path"

type Client = PluginInput["client"]

const debounceMs = 1500

function reviews(dir: string): string[] {
  try {
    return readdirSync(dir).filter((name) => /^\d+\.md$/.test(name))
  } catch {
    return []
  }
}

function contents(path: string): string | undefined {
  try {
    return readFileSync(path, "utf8")
  } catch {
    return undefined
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
  mkdirSync(reviewDir, { recursive: true })

  // Compare contents, never mtimes: a checkout or touch that leaves the review
  // unchanged must not wake the agent. The baseline is taken in memory at
  // startup, so no state file (and nothing inside the repo) is involved.
  const seen = new Map<string, string>()
  const pending = new Map<string, string>()
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  const children = new Set<string>()
  let active: string | undefined
  let flushing = false

  for (const name of reviews(reviewDir)) {
    const text = contents(join(reviewDir, name))
    if (text !== undefined) seen.set(name.slice(0, -3), text)
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
    try {
      const id = await idleSession()
      if (!id) return
      const result = await client.session.prompt({
        path: { id },
        query: { directory },
        body: {
          parts: [
            {
              type: "text",
              text: "Wake up: check docs/reviews for new review activity, then continue.",
            },
          ],
        },
      })
      if (result.error) throw result.error
      for (const [review, text] of pending) {
        seen.set(review, text)
        pending.delete(review)
      }
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
        const text = contents(join(reviewDir, name))
        if (text === undefined) return
        if (text === seen.get(id) || pending.get(id) === text) return
        pending.set(id, text)
        void flush()
      }, debounceMs),
    )
  }

  function scan() {
    for (const name of reviews(reviewDir)) {
      const id = name.slice(0, -3)
      const text = contents(join(reviewDir, name))
      if (text === undefined) continue
      if (text === seen.get(id) || pending.get(id) === text) continue
      schedule(name)
    }
  }

  const watcher = watch(reviewDir, { persistent: true }, () => scan())
  const poll = setInterval(scan, 2000)

  // Calling the client while this factory runs deadlocks startup: opencode
  // bootstraps plugins from inside the instance bootstrap, so an in-process
  // request would wait for the bootstrap that is waiting for this factory.
  // Defer every client call until after the factory has returned.
  const startup = setTimeout(() => {
    void (async () => {
      for (const session of await sessions(client, directory)) {
        if (session.parentID) children.add(session.id)
      }
      if (pending.size > 0) await flush()
    })()
  }, 0)

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
      clearTimeout(startup)
      for (const timer of timers.values()) clearTimeout(timer)
    },
  }
}
