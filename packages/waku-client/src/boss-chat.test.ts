import { expect, test } from "bun:test";

import type {
  ActivityItem,
  AgentSession,
  Message,
  ReportTrigger,
} from "./generated";
import {
  bossChatPredecessor,
  bossSummonCard,
  bossTriggerBurstLabel,
  bossTriggerEntryLabel,
  bossTriggerGroups,
  bossTriggerGroupTone,
  bossTriggerTone,
  findBossSessionMentions,
} from "./boss-chat";

const TURN_A = "11111111-1111-4111-8111-111111111111";
const TURN_B = "22222222-2222-4222-8222-222222222222";
const OLD_CHAT = "33333333-3333-4333-8333-333333333333";
const NEW_CHAT = "44444444-4444-4444-8444-444444444444";
const EMPLOYEE = "55555555-5555-4555-8555-555555555555";

function session(overrides: Partial<AgentSession>): AgentSession {
  return {
    id: NEW_CHAT,
    messages: [],
    transcript_blocks: [],
    turns: [],
    ...overrides,
  } as AgentSession;
}

function message(overrides: Partial<Message>): Message {
  return {
    id: Math.random().toString(36).slice(2),
    role: "user",
    content: "",
    created_at: 0,
    streaming: false,
    turn_id: null,
    ...overrides,
  } as Message;
}

function trigger(overrides: Partial<ReportTrigger>): ReportTrigger {
  return {
    eventId: Math.random().toString(36).slice(2),
    employee: EMPLOYEE,
    employeeName: "Rhea",
    jobTitle: "Fix the bug",
    kind: "finished",
    boundary: "opening",
    ...overrides,
  };
}

test("boss_summon markers parse the JSON card and the legacy bare id", () => {
  const card = bossSummonCard({
    tool_name: "boss_summon",
    arguments: JSON.stringify({
      sessionId: EMPLOYEE,
      name: "Rhea",
      avatarSeed: "seed-1",
      jobTitle: "Review the diff",
      icon: "eye",
    }),
  } as ActivityItem);
  expect(card).toEqual({
    sessionId: EMPLOYEE,
    name: "Rhea",
    avatarSeed: "seed-1",
    jobTitle: "Review the diff",
    icon: "eye",
  });

  expect(
    bossSummonCard({ tool_name: "boss_summon", arguments: EMPLOYEE } as ActivityItem),
  ).toEqual({ sessionId: EMPLOYEE, name: "", avatarSeed: EMPLOYEE, jobTitle: "", icon: null });

  expect(bossSummonCard({ tool_name: "read", arguments: EMPLOYEE } as ActivityItem)).toBeNull();
  expect(
    bossSummonCard({ tool_name: "boss_summon", arguments: "not a marker" } as ActivityItem),
  ).toBeNull();
});

test("a turn's opening reports collapse into one burst anchored at the first", () => {
  const first = message({
    hidden: true,
    turn_id: TURN_A,
    report_trigger: trigger({ employeeName: "Rhea" }),
  });
  const second = message({
    hidden: true,
    turn_id: TURN_A,
    report_trigger: trigger({ employee: "66666666-6666-4666-8666-666666666666", employeeName: "Pablo", kind: "blocker" }),
  });
  const other = message({
    hidden: true,
    turn_id: TURN_B,
    report_trigger: trigger({ kind: "planFinalized", employeeName: "" }),
  });

  const groups = bossTriggerGroups(session({ messages: [first, second, other] }));
  expect(groups).toHaveLength(2);
  expect(groups[0]!.anchorId).toBe(first.id);
  expect(groups[0]!.triggers.map((item) => item.employeeName)).toEqual(["Rhea", "Pablo"]);
  expect(groups[1]!.anchorId).toBe(other.id);
});

test("steer runs close over hidden prompts but split on visible output", () => {
  const one = message({
    hidden: true,
    report_trigger: trigger({ boundary: "steer", employeeName: "Rhea" }),
  });
  const two = message({
    hidden: true,
    report_trigger: trigger({ boundary: "steer", employeeName: "Pablo" }),
  });
  const answer = message({ role: "assistant", content: "ok" });
  const three = message({
    hidden: true,
    report_trigger: trigger({ boundary: "steer", employeeName: "Vex" }),
  });

  const groups = bossTriggerGroups(session({ messages: [one, two, answer, three] }));
  expect(groups).toHaveLength(2);
  expect(groups[0]!.anchorId).toBe(one.id);
  expect(groups[0]!.triggers.map((item) => item.employeeName)).toEqual(["Rhea", "Pablo"]);
  expect(groups[1]!.anchorId).toBe(three.id);
});

test("a transcript block between steer prompts separates their runs", () => {
  const one = message({
    hidden: true,
    report_trigger: trigger({ boundary: "steer" }),
  });
  const two = message({
    hidden: true,
    report_trigger: trigger({ boundary: "steer" }),
  });

  const withBlock = bossTriggerGroups(session({
    messages: [one, two],
    transcript_blocks: [{ after_message: 1, turn_id: null, content: {} } as never],
  }));
  expect(withBlock).toHaveLength(2);

  const withoutBlock = bossTriggerGroups(session({ messages: [one, two] }));
  expect(withoutBlock).toHaveLength(1);
});

test("the same report arriving twice only lands one marker entry", () => {
  const shared = "event-1";
  const one = message({
    hidden: true,
    turn_id: TURN_A,
    report_trigger: trigger({ eventId: shared }),
  });
  const two = message({
    hidden: true,
    turn_id: TURN_A,
    report_trigger: trigger({ eventId: shared }),
  });

  const groups = bossTriggerGroups(session({ messages: [one, two] }));
  expect(groups).toHaveLength(1);
  expect(groups[0]!.triggers).toHaveLength(1);
});

test("trigger labels name the outcome; steer entries note the mid-turn catch", () => {
  expect(bossTriggerEntryLabel(trigger({ kind: "finished" }), false)).toBe("Rhea finished");
  expect(bossTriggerEntryLabel(trigger({ kind: "blocker" }), false)).toBe(
    "Rhea reported a blocker",
  );
  expect(bossTriggerEntryLabel(trigger({ kind: "finishedWithBlocker" }), false)).toBe(
    "Rhea finished with a blocker",
  );
  expect(bossTriggerEntryLabel(trigger({ kind: "interrupted" }), true)).toBe(
    "Rhea was interrupted during this turn",
  );
  expect(bossTriggerEntryLabel(trigger({ kind: "failed" }), true)).toBe(
    "Rhea failed during this turn",
  );
  expect(bossTriggerEntryLabel(trigger({ kind: "planFinalized", employeeName: "" }), false)).toBe(
    "Plan finalized its design",
  );
  expect(bossTriggerEntryLabel(trigger({ kind: "failed", employeeName: "" }), false)).toBe(
    "Employee failed",
  );
});

test("burst labels name uniform outcomes and summarize mixed ones", () => {
  const other = "66666666-6666-4666-8666-666666666666";
  expect(
    bossTriggerBurstLabel([
      trigger({ kind: "finished" }),
      trigger({ employee: other, kind: "finished" }),
    ]),
  ).toBe("2 employees finished");
  expect(
    bossTriggerBurstLabel([
      trigger({ kind: "failed" }),
      trigger({ employee: other, kind: "failed" }),
    ]),
  ).toBe("2 employees failed");
  expect(
    bossTriggerBurstLabel([
      trigger({ kind: "planFinalized" }),
      trigger({ employee: other, kind: "planFinalized" }),
    ]),
  ).toBe("2 plans finalized");
  expect(
    bossTriggerBurstLabel([
      trigger({ kind: "finished" }),
      trigger({ employee: other, kind: "blocker" }),
    ]),
  ).toBe("2 employee updates · 1 blocker");
  expect(
    bossTriggerBurstLabel([
      trigger({ kind: "failed" }),
      trigger({ employee: other, kind: "interrupted" }),
      trigger({ employee: "77777777-7777-4777-8777-777777777777", kind: "blocker" }),
    ]),
  ).toBe("3 employee updates · 1 failed · 2 blockers");
});

test("the worst outcome in a burst decides its tone", () => {
  expect(bossTriggerTone("finished")).toBe("tertiary");
  expect(bossTriggerTone("blocker")).toBe("warning");
  expect(bossTriggerTone("failed")).toBe("danger");
  expect(bossTriggerGroupTone([trigger({ kind: "finished" }), trigger({ kind: "failed" })])).toBe(
    "danger",
  );
  expect(bossTriggerGroupTone([trigger({ kind: "finished" }), trigger({ kind: "interrupted" })])).toBe(
    "warning",
  );
  expect(bossTriggerGroupTone([trigger({ kind: "finished" }), trigger({ kind: "planFinalized" })])).toBe(
    "tertiary",
  );
});

test("the destination's hidden rotation link names the predecessor", () => {
  const current = session({
    messages: [
      message({ role: "system", hidden: true, content: `<boss-rotation-link:${OLD_CHAT}>` }),
      message({ role: "assistant", content: "hi" }),
    ],
  });
  expect(bossChatPredecessor(current)).toBe(OLD_CHAT);

  // The source's outgoing marker must be ignored — its link names itself.
  const source = session({
    id: OLD_CHAT,
    messages: [
      message({ role: "system", hidden: true, content: `<boss-rotation-link:${OLD_CHAT}>` }),
    ],
  });
  expect(bossChatPredecessor(source)).toBeNull();
  expect(bossChatPredecessor(session({}))).toBeNull();
});

test("legacy visible rotation text resolves only on the destination", () => {
  const text = `Boss chat rotated. Old session: ${OLD_CHAT}. New session: ${NEW_CHAT}.`;
  expect(
    bossChatPredecessor(session({ messages: [message({ role: "system", content: text })] })),
  ).toBe(OLD_CHAT);
  expect(
    bossChatPredecessor(
      session({ id: OLD_CHAT, messages: [message({ role: "system", content: text })] }),
    ),
  ).toBeNull();
});

test("bare roster names match word-bounded and exact", () => {
  const mentions = [
    { name: "Rhea", session: "s1" },
    { name: "Pablo Vega", session: "s2" },
    { name: "Pablo", session: "s3" },
  ];
  expect(findBossSessionMentions("Rhea shipped it", mentions, false)).toEqual([
    { start: 0, end: 4, session: "s1" },
  ]);
  // Word characters on either side disqualify a hit.
  expect(findBossSessionMentions("Rheanne shipped it", mentions, false)).toEqual([]);
  expect(findBossSessionMentions("xRhea", mentions, false)).toEqual([]);
  expect(findBossSessionMentions("Rhea", mentions, true)).toEqual([]);
  // Case-sensitive — the roster name's own casing only.
  expect(findBossSessionMentions("rhea shipped it", mentions, false)).toEqual([]);
  // Longest hit wins an overlap; hits never cross.
  expect(findBossSessionMentions("Pablo Vega fixed it", mentions, false)).toEqual([
    { start: 0, end: 10, session: "s2" },
  ]);
  expect(findBossSessionMentions("Rhea and Pablo Vega paired", mentions, false)).toEqual([
    { start: 0, end: 4, session: "s1" },
    { start: 9, end: 19, session: "s2" },
  ]);
});
