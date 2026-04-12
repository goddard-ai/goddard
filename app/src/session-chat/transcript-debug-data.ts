import type { TranscriptMessage } from "./transcript.tsx"

const assistantMessageBodies = [
  "I mapped the shell layout and the current state modules. The next clean seam is the transcript itself, because it can be pinned down visually before any live session transport exists.",
  "I'd keep the chrome quiet and let the bubble width carry the hierarchy. The assistant replies can stay slightly narrower than the full column so long paragraphs still feel readable when the window gets wide.",
  "I ran through the likely message shapes we need to survive: short acknowledgements, long planning paragraphs, code-adjacent prose, and occasional hard line breaks when the model is outlining next steps.\n\nThat means the renderer needs stable line breaking and predictable row heights before we wire any real data.",
  "For virtualization, the important part is avoiding hidden DOM probes on every resize. Precomputing line wraps in JavaScript lets the transcript stay fast even when the fixture has hundreds of messages.",
  "This fixture is intentionally repetitive in structure but varied in length. It is here to expose spacing problems, not to simulate a perfect model transcript.",
  "One thing to watch in the layout pass is how single-line replies sit next to denser blocks. If the short bubbles look too lightweight, we can tighten the corner radius or slightly raise the bubble contrast.",
]

const userMessageBodies = [
  "Keep going on the transcript surface.",
  "Make the user bubbles feel a little more deliberate, not just a mirrored version of the assistant.",
  "We don't need live streaming yet. I just want the spacing, rhythm, and virtualization model locked down so future state work has a stable target.",
  "Can we bias the debug fixture toward medium and long messages instead of lots of tiny back-and-forth turns?",
  "Make sure I can pass a fake `messages` prop directly so the component stays dumb while we experiment.",
]

const systemMessageBodies = [
  "Debug preset: transcript-only surface with synthetic messages.",
  "Viewport resized. Pretext line layout recomputed for the current width.",
]

/** Formats one deterministic clock label for the transcript fixture. */
function formatFixtureTimestamp(totalMinutes: number): string {
  const dayMinutes = ((totalMinutes % (24 * 60)) + 24 * 60) % (24 * 60)
  const hours = Math.floor(dayMinutes / 60)
  const minutes = dayMinutes % 60
  const suffix = hours >= 12 ? "PM" : "AM"
  const hour12 = hours % 12 || 12
  return `${hour12}:${String(minutes).padStart(2, "0")} ${suffix}`
}

/** Builds a long deterministic transcript fixture that makes virtualization visible during debugging. */
function buildDebugMessages(): TranscriptMessage[] {
  const messages: TranscriptMessage[] = [
    {
      kind: "message",
      id: "system:opening",
      role: "system",
      authorName: "Debug Surface",
      timestampLabel: "9:10 AM",
      text: systemMessageBodies[0],
    },
    {
      kind: "toolCall",
      id: "tool:opening",
      toolCallId: "tool:opening",
      authorName: "Goddard",
      timestampLabel: "9:11 AM",
      title: "Read transcript root",
      toolKind: "read",
      status: "completed",
      content: [
        {
          type: "content",
          text: "Loaded the debug transcript fixture and verified that the current layout path supports long-form tool output.",
        },
        {
          type: "diff",
          path: "/repo/app/src/session-chat/transcript.tsx",
          oldText: "const transcriptViewportClass = css({",
          newText:
            'const transcriptViewportClass = css({\n  position: "relative",\n  minHeight: "100%",',
        },
      ],
      locations: [
        {
          path: "/repo/app/src/session-chat/transcript.tsx",
          line: 1,
        },
      ],
    },
  ]

  let minuteCursor = 9 * 60 + 12

  for (let index = 0; index < 96; index += 1) {
    const role = index % 11 === 0 ? "system" : index % 2 === 0 ? "assistant" : "user"
    const authorName = role === "assistant" ? "Goddard" : role === "user" ? "Alec" : "Runtime"
    const body =
      role === "assistant"
        ? assistantMessageBodies[index % assistantMessageBodies.length]
        : role === "user"
          ? userMessageBodies[index % userMessageBodies.length]
          : systemMessageBodies[index % systemMessageBodies.length]

    messages.push({
      kind: "message",
      id: `${role}:${index}`,
      role,
      authorName,
      timestampLabel: formatFixtureTimestamp(minuteCursor),
      text:
        role === "assistant" && index % 5 === 2
          ? `${body}\n\nWorking note ${index + 1}: preserve readable gutters, keep row heights predictable, and let the transcript stay dumb until real session wiring arrives.`
          : body,
    })

    if (role === "assistant" && index % 24 === 10) {
      messages.push({
        kind: "toolCall",
        id: `tool:${index}`,
        toolCallId: `tool:${index}`,
        authorName: "Goddard",
        timestampLabel: formatFixtureTimestamp(minuteCursor + 1),
        title: index % 48 === 10 ? "Search layout references" : "Execute transcript probe",
        toolKind: index % 48 === 10 ? "search" : "execute",
        status: index % 48 === 10 ? "in_progress" : "failed",
        content:
          index % 48 === 10
            ? [
                {
                  type: "content",
                  text: "Scanning nearby transcript modules for spacing, width-estimation, and virtualization assumptions.",
                },
              ]
            : [
                {
                  type: "terminal",
                  terminalId: `term-${index}`,
                },
              ],
        locations: [
          {
            path: `/repo/app/src/session-chat/example-${index}.tsx`,
            line: 18 + index,
          },
        ],
      })
      minuteCursor += 1
    }

    minuteCursor += role === "assistant" ? 4 : role === "user" ? 2 : 1
  }

  return messages
}

/** Stable fake transcript data used by the debug transcript tab. */
export const SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES = buildDebugMessages()
