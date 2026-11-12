import type { PluginInput } from "@opencode-ai/plugin"
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

function makeWatcher(directory: string, wake: () => void) {
  const reviewDir = join(directory, "docs", "reviews")
  mkdirSync(reviewDir, { recursive: true })

  // Compare contents, never mtimes: a checkout or touch that leaves the review
  // unchanged must not wake the agent. The baseline is taken in memory at
  // startup, so no state file (and nothing inside the repo) is involved.
  const seen = new Map<string, string>()
  const pending = new Map<string, string>()
  const timers = new Map<string, ReturnType<typeof setTimeout>>()

  for (const name of reviews(reviewDir)) {
    const text = contents(join(reviewDir, name))
    if (text !== undefined) seen.set(name.slice(0, -3), text)
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
        wake()
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

  return {
    pending,
    seen,
    dispose() {
      watcher.close()
      clearInterval(poll)
      for (const timer of timers.values()) clearTimeout(timer)
    },
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

// V1 server path: older hosts call server() and use the returned hooks.
async function server({ client, directory }: PluginInput) {
  const watcher = makeWatcher(directory, () => void flush())
  const children = new Set<string>()
  let active: string | undefined
  let flushing = false

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
    if (flushing || watcher.pending.size === 0) return
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
      for (const [review, text] of watcher.pending) {
        watcher.seen.set(review, text)
        watcher.pending.delete(review)
      }
      success = true
    } catch (error) {
      console.error("review-watch: unable to prompt", error)
    } finally {
      flushing = false
    }
    if (success && watcher.pending.size > 0) void flush()
  }

  if (watcher.pending.size > 0) void flush()

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
    event: async ({ event }: { event: Event }) => {
      await onEvent(event)
    },
    dispose: async () => {
      watcher.dispose()
    },
  }
}

// V2 setup path: current hosts read the default export, call setup(ctx), and
// ignore server(). The V2 context exposes the event stream and session.prompt;
// active/idle sessions are learned from that stream instead of listed.
async function setup(ctx: any) {
  const directory =
    ctx.location?.directory ?? ctx.project?.directory ?? process.cwd()
  const watcher = makeWatcher(directory, () => void flush())
  const children = new Set<string>()
  const idle = new Set<string>()
  const controller = new AbortController()
  let active: string | undefined
  let flushing = false

  async function flush() {
    if (flushing || watcher.pending.size === 0) return
    if (!active || !idle.has(active)) return
    flushing = true
    let success = false
    try {
      await ctx.session.prompt({
        sessionID: active,
        text: "Wake up: check docs/reviews for new review activity, then continue.",
      })
      for (const [review, text] of watcher.pending) {
        watcher.seen.set(review, text)
        watcher.pending.delete(review)
      }
      success = true
    } catch (error) {
      console.error("review-watch: unable to prompt", error)
    } finally {
      flushing = false
    }
    if (success && watcher.pending.size > 0) void flush()
  }

  function info(event: any) {
    return event.properties ?? event
  }

  void (async () => {
    for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
      const props = info(event)
      const session = props.info ?? props.session
      const id = props.sessionID ?? session?.id
      if (event.type === "session.status" && id) {
        if (children.has(id)) continue
        active = id
        if (props.status?.type === "idle") {
          idle.add(id)
          void flush()
        } else {
          idle.delete(id)
        }
      } else if (event.type === "session.idle" && id) {
        if (children.has(id)) continue
        active = id
        idle.add(id)
        void flush()
      } else if (
        event.type === "session.created" ||
        event.type === "session.updated"
      ) {
        if (!id) continue
        if (session?.parentID) {
          children.add(id)
        } else {
          active = id
        }
      } else if (event.type === "session.deleted" && id) {
        children.delete(id)
        idle.delete(id)
        if (active === id) active = undefined
      }
    }
  })().catch((error) => {
    if (!controller.signal.aborted) {
      console.error("review-watch: event stream", error)
    }
  })

  if (watcher.pending.size > 0) void flush()

  return () => {
    controller.abort()
    watcher.dispose()
  }
}

// V1 hosts call server(); V2 hosts call setup(). Export only the definition.
export default {
  id: "review-watch",
  server,
  setup,
}
