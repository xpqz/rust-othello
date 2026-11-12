import { Plugin } from "@opencode/plugin/tui"
import { mkdirSync, readFileSync, readdirSync, watch } from "node:fs"
import { join } from "node:path"

const debounceMs = 1500
const wakeText = "Wake up: check docs/reviews for new review activity, then continue."

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
  // unchanged must not wake the agent. The baseline is in memory at startup.
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

export default Plugin.define({
  id: "review-watch",
  setup(context) {
    const directory =
      context.location?.directory ??
      context.data.location.default()?.directory ??
      process.cwd()
    const watcher = makeWatcher(directory, () => void flush())
    let flushing = false

    async function flush() {
      if (flushing || watcher.pending.size === 0) return
      const route = context.ui.router.current()
      if (route.type !== "session") return
      if (context.data.session.status(route.sessionID) !== "idle") return

      flushing = true
      let success = false
      try {
        const client = context.client as any
        let result: any
        try {
          result = await client.session.prompt({
            path: { id: route.sessionID },
            body: { parts: [{ type: "text", text: wakeText }] },
          })
        } catch {
          result = await client.session.prompt({
            sessionID: route.sessionID,
            text: wakeText,
          })
        }
        if (result?.error) throw result.error
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

    const stops = [
      context.data.on("session.idle", () => void flush()),
      context.data.on("session.status", () => void flush()),
    ]
    // A review can change while the TUI is on Home; retry while a session is
    // open so entering it does not need another session event.
    const retry = setInterval(() => void flush(), 2000)
    if (watcher.pending.size > 0) void flush()

    return () => {
      for (const stop of stops) stop()
      clearInterval(retry)
      watcher.dispose()
    }
  },
})
