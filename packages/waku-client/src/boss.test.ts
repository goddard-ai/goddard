import { describe, expect, test } from "bun:test";

import {
  bossAttention,
  bossChat,
  bossEmployeeLifecycle,
  bossListFiles,
  bossReadFile,
  bossReadPlanDocument,
  bossRequest,
  bossRoster,
  bossTerminalIntent,
  bossTranscript,
  deliverableTarget,
  deliverableUnread,
  dismissDeliverable,
  loadBossState,
  markBossGoalsViewed,
  markDeliverableViewed,
  pinDeliverable,
  planItemProgress,
  setPlanItemState,
  setPlanOutcome,
  subscribeBossTerminalIntents,
  sweepDeliverable,
  type BossClient,
} from "./boss";
import type {
  BossDeliverable,
  BossEmployee,
  BossPlan,
  BossResult,
  BossState,
  Command,
  DaemonSettings,
  ResponsePayload,
  SequencedEvent,
} from "./generated";

function stubClient(responses: ResponsePayload[]) {
  const commands: Command[] = [];
  const queue = [...responses];
  const subscriptions: Array<{
    sessionId: string;
    runtimeId: string;
    listener: (event: SequencedEvent) => void;
  }> = [];
  const client: BossClient = {
    request: async (command) => {
      commands.push(command);
      const next = queue.shift();
      if (!next) throw new Error("unexpected request");
      return next;
    },
    subscribe: (sessionId, runtimeId, listener) => {
      subscriptions.push({ sessionId, runtimeId, listener });
      return () => {};
    },
  };
  return { client, commands, subscriptions };
}

function bossPayload(result: BossResult): ResponsePayload {
  return { type: "boss", result };
}

const IDENTITY = { id: "boss-1", name: "Boss", avatarSeed: "seed" };

function bossState(overrides: Partial<BossState> = {}): BossState {
  return {
    identity: IDENTITY,
    personaId: "persona-1",
    sessionId: "boss-session",
    personas: [],
    employees: [],
    retiredEmployees: [],
    deliverables: [],
    planning: [],
    goalsViewedAt: null,
    resourcePolicy: { revision: 0, modelLimits: [] },
    nextSequence: 1,
    nextEventId: 1,
    nameCursor: 0,
    revision: 1,
    ...overrides,
  };
}

function employee(overrides: Partial<BossEmployee> = {}): BossEmployee {
  return {
    sessionId: "emp-1",
    supervisorId: "boss-session",
    identity: { id: "emp-1", name: "Owen", avatarSeed: "seed" },
    jobTitle: "U0",
    personaId: "persona-1",
    workGoal: "errand",
    permissions: {
      bucketIds: [],
      integrationIds: [],
      summonEmployees: false,
      computerUse: false,
    },
    pinnedFiles: [],
    expired: false,
    cancelled: false,
    state: "working",
    ...overrides,
  };
}

function deliverable(overrides: Partial<BossDeliverable> = {}): BossDeliverable {
  return {
    id: "del-1",
    name: "report.md",
    path: "/store/report.md",
    directory: false,
    createdAt: 10,
    updatedAt: 10,
    pinnedAt: null,
    dormantAt: null,
    archivedAt: null,
    viewedAt: null,
    ...overrides,
  };
}

function plan(overrides: Partial<BossPlan> = {}): BossPlan {
  return {
    id: "plan-1",
    sessionId: "plan-session",
    planFile: "plans/goddard-xplat-boss-chat.md",
    idea: "Boss chat on mobile",
    items: [],
    ...overrides,
  };
}

describe("bossRequest", () => {
  test("wraps the operation in a boss command and unwraps the result", async () => {
    const { client, commands } = stubClient([
      bossPayload({ type: "roster", roster: "1 employee" }),
    ]);

    await expect(bossRequest(client, { type: "roster" })).resolves.toEqual({
      type: "roster",
      roster: "1 employee",
    });
    expect(commands).toEqual([{ type: "boss", operation: { type: "roster" } }]);
  });

  test("throws when the daemon answers a non-boss payload", async () => {
    const { client } = stubClient([{ type: "ack" }]);
    await expect(bossRequest(client, { type: "view" })).rejects.toThrow(
      "Expected daemon response boss",
    );
  });
});

describe("loadBossState", () => {
  test("returns null without a boss request when the experiment is off", async () => {
    const { client, commands } = stubClient([
      {
        type: "settings",
        settings: { boss_experiment_enabled: false } as DaemonSettings,
      },
    ]);
    await expect(loadBossState(client)).resolves.toBeNull();
    expect(commands).toHaveLength(1);
  });

  test("fetches the boss document when the experiment is on", async () => {
    const state = bossState();
    const { client, commands } = stubClient([
      {
        type: "settings",
        settings: { boss_experiment_enabled: true } as DaemonSettings,
      },
      bossPayload({ type: "state", state }),
    ]);
    await expect(loadBossState(client)).resolves.toEqual(state);
    expect(commands).toEqual([
      { type: "getSettings" },
      { type: "boss", operation: { type: "view" } },
    ]);
  });
});

describe("boss session discovery", () => {
  test("bossChat projects identity and chat session off the document", () => {
    expect(bossChat(bossState())).toEqual({
      identity: IDENTITY,
      sessionId: "boss-session",
    });
    expect(bossChat(bossState({ sessionId: null })).sessionId).toBeNull();
  });
});

describe("boss reads", () => {
  test("roster returns the digest text", async () => {
    const { client } = stubClient([bossPayload({ type: "roster", roster: "text" })]);
    await expect(bossRoster(client)).resolves.toBe("text");
  });

  test("transcript passes the bounded turn through", async () => {
    const transcript = {
      taskId: "emp-1",
      title: "U0",
      provider: "codex" as const,
      status: "idle" as const,
      items: [],
    };
    const { client, commands } = stubClient([
      bossPayload({ type: "transcript", transcript }),
    ]);
    await expect(bossTranscript(client, "emp-1", 3)).resolves.toEqual(transcript);
    expect(commands).toEqual([
      { type: "boss", operation: { type: "transcript", sessionId: "emp-1", turn: 3 } },
    ]);
  });

  test("file ops list directories and read plan documents", async () => {
    const { client, commands } = stubClient([
      bossPayload({ type: "files", files: [{ path: "plans/a.md", directory: false }] }),
      bossPayload({ type: "file", path: "plans/a.md", content: "# Plan" }),
      bossPayload({ type: "file", path: "plans/a.md", content: "# Plan" }),
    ]);
    await expect(bossListFiles(client, "plans")).resolves.toEqual([
      { path: "plans/a.md", directory: false },
    ]);
    await expect(bossReadFile(client, "plans/a.md")).resolves.toBe("# Plan");
    await expect(
      bossReadPlanDocument(client, plan({ planFile: "plans/a.md" })),
    ).resolves.toBe("# Plan");
    expect(commands).toEqual([
      { type: "boss", operation: { type: "listFiles", path: "plans" } },
      { type: "boss", operation: { type: "readFile", path: "plans/a.md" } },
      { type: "boss", operation: { type: "readFile", path: "plans/a.md" } },
    ]);
  });
});

describe("user-invokable ops", () => {
  test("deliverable ops send their operation variants", async () => {
    const { client, commands } = stubClient([
      bossPayload({ type: "saved" }),
      bossPayload({ type: "saved" }),
      bossPayload({ type: "saved" }),
      bossPayload({ type: "saved" }),
      bossPayload({ type: "saved" }),
    ]);
    await markDeliverableViewed(client, "del-1");
    await pinDeliverable(client, "del-1", false);
    await sweepDeliverable(client, "del-1");
    await dismissDeliverable(client, "del-1");
    await markBossGoalsViewed(client);
    expect(commands).toEqual([
      { type: "boss", operation: { type: "markDeliverableViewed", id: "del-1" } },
      { type: "boss", operation: { type: "pinDeliverable", id: "del-1", pinned: false } },
      { type: "boss", operation: { type: "sweepDeliverable", id: "del-1", dormant: true } },
      { type: "boss", operation: { type: "dismissDeliverable", id: "del-1" } },
      { type: "boss", operation: { type: "markGoalsViewed" } },
    ]);
  });

  test("plan ops return the fresh document", async () => {
    const next = bossState({ revision: 2 });
    const { client, commands } = stubClient([
      bossPayload({ type: "state", state: next }),
      bossPayload({ type: "state", state: next }),
    ]);
    await expect(
      setPlanItemState(client, "plan-1", "item-1", "done"),
    ).resolves.toEqual(next);
    await expect(
      setPlanOutcome(client, "plan-1", "completed"),
    ).resolves.toEqual(next);
    expect(commands).toEqual([
      {
        type: "boss",
        operation: { type: "setPlanItemState", plan: "plan-1", item: "item-1", state: "done" },
      },
      { type: "boss", operation: { type: "setPlanOutcome", plan: "plan-1", outcome: "completed" } },
    ]);
  });
});

describe("derived state", () => {
  test("expired projection wins over a stale lifecycle", () => {
    expect(bossEmployeeLifecycle(employee({ expired: true, state: "working" }))).toBe("expired");
    expect(bossEmployeeLifecycle(employee())).toBe("working");
  });

  test("planItemProgress derives in-progress from live linked employees", () => {
    const item = { id: "item-1", title: "U0", state: "toDo" as const };
    const target = plan({ items: [item] });
    expect(planItemProgress(target, item, [])).toBe("toDo");
    expect(
      planItemProgress(target, item, [employee({ planId: "plan-1", itemId: "item-1" })]),
    ).toBe("inProgress");
    expect(
      planItemProgress(target, item, [
        employee({ planId: "plan-1", itemId: "item-1", expired: true, state: "expired" }),
      ]),
    ).toBe("toDo");
    expect(planItemProgress(target, { ...item, state: "done" }, [])).toBe("done");
  });

  test("deliverableUnread follows the sidebar's badge rule", () => {
    expect(deliverableUnread(deliverable())).toBe(true);
    expect(deliverableUnread(deliverable({ viewedAt: 10 }))).toBe(false);
    expect(deliverableUnread(deliverable({ viewedAt: 10, updatedAt: 11 }))).toBe(true);
    expect(deliverableUnread(deliverable({ archivedAt: 5 }))).toBe(false);
    expect(deliverableUnread(deliverable({ dormantAt: 5 }))).toBe(false);
    expect(deliverableUnread(deliverable({ dormantAt: 5, pinnedAt: 6 }))).toBe(true);
  });

  test("deliverableTarget roots a directory at itself and a file at its parent", () => {
    expect(deliverableTarget(deliverable({ directory: true, path: "/store/bundle" }))).toEqual({
      root: "/store/bundle",
      relativePath: null,
    });
    expect(deliverableTarget(deliverable({ path: "/store/report.md" }))).toEqual({
      root: "/store",
      relativePath: "report.md",
    });
    expect(deliverableTarget(deliverable({ path: "C:\\store\\report.md" }))).toEqual({
      root: "C:\\store",
      relativePath: "report.md",
    });
    expect(deliverableTarget(deliverable({ path: "/report.md" }))).toEqual({
      root: "/",
      relativePath: "report.md",
    });
  });
});

describe("bossAttention", () => {
  test("reports nothing on the first sync or an identity change", () => {
    const current = bossState({ employees: [employee({ blocker: "stuck" })] });
    expect(bossAttention(undefined, current)).toEqual([]);
    expect(
      bossAttention(bossState({ identity: { ...IDENTITY, id: "other" } }), current),
    ).toEqual([]);
  });

  test("reports a newly raised blocker", () => {
    const previous = bossState({ employees: [employee()] });
    const current = bossState({ employees: [employee({ blocker: "needs a key" })] });
    expect(bossAttention(previous, current)).toEqual([
      { type: "employeeBlocker", employee: current.employees[0]! },
    ]);
    // A blocker that was already raised does not re-report.
    expect(bossAttention(current, current)).toEqual([]);
  });

  test("reports new and republished deliverables", () => {
    const published = deliverable();
    const first = bossAttention(bossState(), bossState({ deliverables: [published] }));
    expect(first).toEqual([{ type: "deliverablePublished", deliverable: published }]);

    const republished = { ...published, updatedAt: 20 };
    const second = bossAttention(
      bossState({ deliverables: [published] }),
      bossState({ deliverables: [republished] }),
    );
    expect(second).toEqual([{ type: "deliverableUpdated", deliverable: republished }]);
  });

  test("reports a plan that crossed into finalized", () => {
    const draft = plan();
    const finalized = plan({ finalizedAt: 30 });
    expect(
      bossAttention(bossState({ planning: [draft] }), bossState({ planning: [finalized] })),
    ).toEqual([{ type: "planFinalized", plan: finalized }]);
    // A plan that appears already finalized reports too — the client missed
    // the transition between polls.
    expect(bossAttention(bossState(), bossState({ planning: [finalized] }))).toEqual([
      { type: "planFinalized", plan: finalized },
    ]);
  });
});

describe("bossTerminalIntent", () => {
  const intentEvent: SequencedEvent = {
    sessionId: "boss-session",
    runtimeId: "runtime",
    epoch: "epoch",
    sequence: 1,
    event: {
      kind: "bossTerminalIntent",
      payload: { title: "Logs", cwd: "/tmp", command: "tail -f x" },
    },
  };

  test("decodes the wire event and ignores other kinds", () => {
    expect(bossTerminalIntent(intentEvent.event)).toEqual({
      title: "Logs",
      cwd: "/tmp",
      command: "tail -f x",
    });
    expect(
      bossTerminalIntent({ kind: "textDelta", payload: "nope" }),
    ).toBeNull();
    expect(
      bossTerminalIntent({ kind: "bossTerminalIntent", payload: { title: 1 } }),
    ).toBeNull();
  });

  test("subscribeBossTerminalIntents filters the session stream", () => {
    const { client, subscriptions } = stubClient([]);
    const received: string[] = [];
    subscribeBossTerminalIntents(client, "boss-session", "runtime", (intent) =>
      received.push(intent.title),
    );
    const subscription = subscriptions[0]!;
    expect([subscription.sessionId, subscription.runtimeId]).toEqual(["boss-session", "runtime"]);
    subscription.listener(intentEvent);
    subscription.listener({ ...intentEvent, event: { kind: "textDelta", payload: "x" } });
    expect(received).toEqual(["Logs"]);
  });
});
