import type { SessionTranscriptToolCall, SessionTranscriptToolContent } from "~/sessions/models.ts"
import { buildToolDiffPreview } from "./layout.ts"
import styles from "./transcript.style.ts"

const TOOL_KIND_SUMMARIES: Record<SessionTranscriptToolCall["toolKind"], readonly [verb: string, noun: string]> = {
  read: ["Read", "file"],
  edit: ["Edited", "file"],
  delete: ["Deleted", "file"],
  move: ["Moved", "file"],
  search: ["Searched", "time"],
  execute: ["Ran", "command"],
  think: ["Thought", "time"],
  fetch: ["Fetched", "resource"],
  switch_mode: ["Switched", "mode"],
  other: ["Used", "tool"],
}

function formatCount(noun: string, count: number) {
  return `${count} ${count === 1 ? noun : `${noun}s`}`
}

function findToolCallPath(toolCall: SessionTranscriptToolCall) {
  const locationPath = toolCall.locations[0]?.path
  if (locationPath) {
    return locationPath
  }

  for (const content of toolCall.content) {
    if (content.type === "diff" && content.path) {
      return content.path
    }
  }

  return findRawString(toolCall.rawInput, ["path", "file", "filePath"])
}

function getPathBasename(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path
}

function isRecord(value: unknown) {
  return typeof value === "object" && value !== null
}

function findRawString(value: unknown, keys: readonly string[]) {
  if (typeof value === "string") {
    const text = value.trim()
    return text.length > 0 ? text : null
  }

  if (!isRecord(value)) {
    return null
  }

  const record = value as Record<string, unknown>

  for (const key of keys) {
    const rawValue = record[key]
    if (typeof rawValue === "string" && rawValue.trim().length > 0) {
      return rawValue.trim()
    }
  }

  return null
}

function findRawCommand(value: unknown) {
  if (typeof value === "string") {
    const command = value.trim()
    return command.length > 0 ? command : null
  }

  if (!isRecord(value)) {
    return null
  }

  const record = value as Record<string, unknown>
  const command =
    typeof record.command === "string" ? record.command.trim() : ""
  if (command.length === 0) {
    return findRawString(record, ["cmd", "script"])
  }

  const args =
    Array.isArray(record.args)
      ? record.args.flatMap((arg) => (typeof arg === "string" ? [arg] : []))
      : []

  return [command, ...args].join(" ")
}

function removeTitlePrefix(title: string, prefixes: readonly string[]) {
  const trimmedTitle = title.trim()

  for (const prefix of prefixes) {
    if (trimmedTitle.toLowerCase().startsWith(prefix.toLowerCase())) {
      return trimmedTitle.slice(prefix.length).trim() || trimmedTitle
    }
  }

  return trimmedTitle
}

function countToolCallTargets(toolCall: SessionTranscriptToolCall) {
  const paths = new Set<string>()

  for (const location of toolCall.locations) {
    paths.add(location.path)
  }

  for (const content of toolCall.content) {
    if (content.type === "diff" && content.path) {
      paths.add(content.path)
    }
  }

  return paths.size || 1
}

function buildToolCallDescription(toolCall: SessionTranscriptToolCall) {
  const [verb, noun] = TOOL_KIND_SUMMARIES[toolCall.toolKind]

  return `${verb} ${formatCount(noun, countToolCallTargets(toolCall))}`
}

function buildToolCallDetailDescription(toolCall: SessionTranscriptToolCall) {
  const path = findToolCallPath(toolCall)

  if (path && ["read", "edit", "delete", "move"].includes(toolCall.toolKind)) {
    const [verb] = TOOL_KIND_SUMMARIES[toolCall.toolKind]

    return {
      text: `${verb} ${getPathBasename(path)}`,
      tooltip: path,
    }
  }

  if (toolCall.toolKind === "execute") {
    const command = findRawCommand(toolCall.rawInput)

    return {
      text: `Ran ${
        command ?? removeTitlePrefix(toolCall.title, ["Run ", "Ran ", "Execute ", "Executed "])
      }`,
      tooltip: null,
    }
  }

  if (toolCall.toolKind === "search") {
    const searchTarget =
      findRawString(toolCall.rawInput, ["query", "pattern", "search"]) ??
        removeTitlePrefix(toolCall.title, ["Search ", "Searched ", "Queue ", "Queued "])

    return {
      text: `Searched ${searchTarget}`,
      tooltip: null,
    }
  }

  if (toolCall.toolKind === "fetch") {
    const resource = findRawString(toolCall.rawInput, ["url", "uri", "href", "resource"])

    return {
      text: `Fetched ${resource ?? removeTitlePrefix(toolCall.title, ["Fetch ", "Fetched "])}`,
      tooltip: null,
    }
  }

  if (toolCall.toolKind === "switch_mode") {
    const mode = findRawString(toolCall.rawInput, ["mode", "name"])

    return {
      text: `Switched ${mode ?? removeTitlePrefix(toolCall.title, ["Switch ", "Switched "])}`,
      tooltip: null,
    }
  }

  return {
    text: toolCall.toolKind === "other" ? toolCall.title : buildToolCallDescription(toolCall),
    tooltip: null,
  }
}

function buildToolGroupDescription(toolCalls: readonly SessionTranscriptToolCall[]) {
  const counts = new Map<SessionTranscriptToolCall["toolKind"], number>()

  for (const toolCall of toolCalls) {
    counts.set(
      toolCall.toolKind,
      (counts.get(toolCall.toolKind) ?? 0) + countToolCallTargets(toolCall),
    )
  }

  return Array.from(counts.entries()).map(([toolKind, count]) => {
    const [verb, noun] = TOOL_KIND_SUMMARIES[toolKind]

    return `${verb} ${formatCount(noun, count).toLowerCase()}`
  }).map((part, index) => {
    return index === 0 ? part : `${part.slice(0, 1).toLowerCase()}${part.slice(1)}`
  }).join(", ")
}

component ToolDetailHeading(props: {
  toolCall: SessionTranscriptToolCall;
}) {
  const description = buildToolCallDetailDescription(props.toolCall)

  <div class={styles.toolDetailHeading} title={description.tooltip ?? undefined}>
    {description.text}
  </div>
}

component ToolContentDetail(props: {
  content: SessionTranscriptToolContent;
}) {
  if (props.content.type === "content") {
    <div class={styles.toolDetailText}>
      {props.content.text?.trim() || "Structured tool output."}
    </div>
  } else if (props.content.type === "diff") {
    <div class={styles.toolDetailList}>
      if (props.content.path) {
        <div class={styles.toolDetailMono}>
          {props.content.path}
        </div>
      }

      <div class={styles.toolDetailMono}>
        {buildToolDiffPreview(props.content)}
      </div>
    </div>
  } else {
    <div class={styles.toolDetailMono}>
      "Terminal session "
      {props.content.terminalId}
    </div>
  }
}

/** Renders one collapsed transcript tool row that expands into plain-text details on demand. */
export component ToolTranscriptRow(props: {
  groupId: string;
  toolCalls: readonly SessionTranscriptToolCall[];
}) {
  <article class={styles.row}>
    <div class={styles.rowInner} style={{ justifyContent: "flex-start" }}>
      <div class={styles.rowColumn} style={{ width: "100%" }}>
        <details class={styles.toolDisclosure}>
          <summary class={styles.toolSummary}>
            <span class={styles.toolSummaryText}>
              {buildToolGroupDescription(props.toolCalls)}
            </span>
          </summary>
          <ol class={styles.toolDetailList}>
            for (const toolCall of props.toolCalls) {
              <li class={styles.toolDetailItem} key={`${props.groupId}:${toolCall.id}`}>
                <article>
                  <ToolDetailHeading {toolCall} />

                  for (const content of toolCall.content; index index) {
                    <ToolContentDetail key={`${toolCall.id}:content:${index}`} {content} />
                  }
                </article>
              </li>
            }
          </ol>
        </details>
      </div>
    </div>
  </article>
}
