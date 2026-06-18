import { expect, test } from "bun:test"

import {
  createWatchEventQueue,
  shouldIgnoreWatchEvent,
  waitForWatchQuietPeriod,
  type WatchEventDetail,
} from "../src/watch-events.ts"
import { createDeferred, sleep } from "./support.ts"

test("watch event queue wakes waiters and drains event details once", async () => {
  const events = createWatchEventQueue(undefined)
  const waiting = events.waitForEvent()
  const detail = createWatchEventDetail("worktree", "change", "README.md")

  events.notify(detail)

  expect(await waiting).toBe(true)
  expect(events.drainEvents()).toEqual([detail])
  expect(events.drainEvents()).toEqual([])
})

test("watch event queue returns pending notifications before waiting", async () => {
  const events = createWatchEventQueue(undefined)

  events.notify()

  expect(await events.waitForEventOrTimeout(1)).toBe(true)
  expect(await events.waitForEventOrTimeout(1)).toBe(false)
})

test("watch event queue wakes waiters on abort", async () => {
  const controller = new AbortController()
  const events = createWatchEventQueue(controller.signal)
  const waiting = events.waitForEvent()

  controller.abort()

  expect(await waiting).toBe(false)
  expect(await events.waitForEvent()).toBe(false)
})

test("watch event queue rejects future waiters after watcher failure", async () => {
  const events = createWatchEventQueue(undefined)
  const error = new Error("watch failed")

  events.fail(error)

  expect(() => events.waitForEvent()).toThrow("watch failed")
})

test("watch quiet period waits until events stop arriving", async () => {
  const events = createWatchEventQueue(undefined)
  const quiet = waitForWatchQuietPeriod(events, undefined, 5)

  events.notify(createWatchEventDetail("worktree", "change", "a.txt"))
  await sleep(1)
  events.notify(createWatchEventDetail("git", "rename", "HEAD"))

  expect(await quiet).toBe(true)
  expect(events.drainEvents()).toHaveLength(2)
})

test("watch quiet period stops when aborted", async () => {
  const controller = new AbortController()
  const events = createWatchEventQueue(controller.signal)
  const waiting = createDeferred<boolean>()

  waitForWatchQuietPeriod(events, controller.signal, 10).then(waiting.resolve, waiting.reject)
  controller.abort()

  expect(await waiting.promise).toBe(false)
})

test("watch event filtering ignores only Git metadata reported by worktree watchers", () => {
  expect(shouldIgnoreWatchEvent("worktree", ".git")).toBe(true)
  expect(shouldIgnoreWatchEvent("worktree", ".git/config")).toBe(true)
  expect(shouldIgnoreWatchEvent("worktree", Buffer.from(".git\\HEAD"))).toBe(true)
  expect(shouldIgnoreWatchEvent("worktree", "src/index.ts")).toBe(false)
  expect(shouldIgnoreWatchEvent("worktree", null)).toBe(false)
  expect(shouldIgnoreWatchEvent("git", ".git/config")).toBe(false)
})

function createWatchEventDetail(
  source: WatchEventDetail["source"],
  eventType: string,
  filename: string,
): WatchEventDetail {
  return {
    source,
    path: source === "git" ? "/repo/.git" : "/repo",
    recursive: source === "worktree",
    eventType,
    filename,
  }
}
