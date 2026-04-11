import type { SessionMessageEvent } from "@goddard-ai/schema/daemon";
import { expect, test } from "bun:test";
import { createDaemonSubscriptionCoordinator } from "./daemon-subscriptions.ts";

test("daemon subscription coordinator resets once before the first subscription and routes payloads", async () => {
  const calls: string[] = [];
  const coordinator = createDaemonSubscriptionCoordinator({
    webviewId: 7,
    nextSubscriptionId: () => "sub-1",
    resetSubscriptions: async ({ webviewId }) => {
      calls.push(`reset:${webviewId}`);
    },
    subscribe: async ({ subscriptionId, target, webviewId }) => {
      calls.push(`subscribe:${webviewId}:${subscriptionId}:${target.name}`);
    },
    unsubscribe: async ({ subscriptionId }) => {
      calls.push(`unsubscribe:${subscriptionId}`);
    },
  });

  const received: SessionMessageEvent[] = [];
  const unsubscribe = await coordinator.subscribe(
    {
      name: "session.message",
      filter: { id: "ses_1" },
    },
    (payload) => {
      received.push(payload);
    },
  );

  coordinator.dispatchEvent({
    subscriptionId: "sub-1",
    name: "session.message",
    payload: {
      id: "ses_1",
      message: { jsonrpc: "2.0", method: "session/update", params: {} },
    },
  });

  unsubscribe();
  await Promise.resolve();

  expect(calls).toEqual(["reset:7", "subscribe:7:sub-1:session.message", "unsubscribe:sub-1"]);
  expect(received).toEqual([
    {
      id: "ses_1",
      message: { jsonrpc: "2.0", method: "session/update", params: {} },
    },
  ]);
});

test("daemon subscription coordinator removes callbacks when subscribe fails", async () => {
  const coordinator = createDaemonSubscriptionCoordinator({
    webviewId: 3,
    nextSubscriptionId: () => "sub-fail",
    resetSubscriptions: async () => {},
    subscribe: async () => {
      throw new Error("boom");
    },
    unsubscribe: async () => {},
  });

  await expect(
    coordinator.subscribe(
      {
        name: "session.message",
        filter: { id: "ses_1" },
      },
      () => {
        throw new Error("should not be called");
      },
    ),
  ).rejects.toThrow("boom");

  coordinator.dispatchEvent({
    subscriptionId: "sub-fail",
    name: "session.message",
    payload: {
      id: "ses_1",
      message: { jsonrpc: "2.0", method: "session/update", params: {} },
    },
  });
});

test("daemon subscription coordinator retries reset after a reset failure", async () => {
  let resetAttempts = 0;
  const calls: string[] = [];
  const coordinator = createDaemonSubscriptionCoordinator({
    webviewId: 11,
    nextSubscriptionId: () => `sub-${resetAttempts}`,
    resetSubscriptions: async () => {
      resetAttempts += 1;

      if (resetAttempts === 1) {
        throw new Error("reset failed");
      }

      calls.push("reset:ok");
    },
    subscribe: async ({ subscriptionId }) => {
      calls.push(`subscribe:${subscriptionId}`);
    },
    unsubscribe: async () => {},
  });

  await expect(
    coordinator.subscribe(
      {
        name: "session.message",
        filter: { id: "ses_1" },
      },
      () => {},
    ),
  ).rejects.toThrow("reset failed");

  await coordinator.subscribe(
    {
      name: "session.message",
      filter: { id: "ses_1" },
    },
    () => {},
  );

  expect(calls).toEqual(["reset:ok", "subscribe:sub-2"]);
});

test("daemon subscription coordinator returns an idempotent unsubscribe function", async () => {
  const calls: string[] = [];
  const coordinator = createDaemonSubscriptionCoordinator({
    webviewId: 5,
    nextSubscriptionId: () => "sub-2",
    resetSubscriptions: async () => {},
    subscribe: async () => {},
    unsubscribe: async ({ subscriptionId }) => {
      calls.push(subscriptionId);
    },
  });

  const unsubscribe = await coordinator.subscribe(
    {
      name: "workforce.event",
      filter: { rootDir: "/repo" },
    },
    () => {},
  );

  unsubscribe();
  unsubscribe();
  await Promise.resolve();

  expect(calls).toEqual(["sub-2"]);
});
