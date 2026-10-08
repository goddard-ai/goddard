import type { ComposerDraftAnnotation } from "./generated";

// Span ranges are UTF-8 byte offsets into `text`; a JS string indexes UTF-16
// code units instead. The ranges snap to character boundaries upstream, so
// each offset lands on a code point's first byte.
function utf8Width(codePoint: number): number {
  if (codePoint < 0x80) return 1;
  if (codePoint < 0x800) return 2;
  if (codePoint < 0x10000) return 3;
  return 4;
}

function sliceSpanText(text: string, start: number, end: number): string {
  let from = -1;
  let to = text.length;
  let bytes = 0;
  let index = 0;
  for (const char of text) {
    if (from === -1 && bytes >= start) from = index;
    if (bytes >= end) {
      to = index;
      break;
    }
    bytes += utf8Width(char.codePointAt(0) ?? 0);
    index += char.length;
  }
  return text.slice(from === -1 ? text.length : from, to);
}

// The annotated text: span texts joined in document order the way copy joins
// them — one newline between spans, two across a block break.
function annotationQuotedText(annotation: ComposerDraftAnnotation): string {
  let text = "";
  let hasSpan = false;
  for (const span of annotation.spans) {
    if (hasSpan) {
      text += "\n";
      if (span.block_break) text += "\n";
    }
    text += sliceSpanText(span.text, span.start, span.end);
    hasSpan = true;
  }
  return text;
}

// The quoted passage as the prompt and the sent bubble carry it: a file
// annotation leads with `@path` and its line-span marker above a fenced block
// of the selected code; a transcript annotation is just its text.
function annotationPromptPassage(annotation: ComposerDraftAnnotation): string {
  const file = annotation.file;
  if (!file) return annotationQuotedText(annotation);
  const marker = file.start_line === file.end_line
    ? `[Selected line ${file.start_line}]`
    : `[Selected lines ${file.start_line}-${file.end_line}]`;
  return `@${file.path}\n${marker}\n\`\`\`\n${annotationQuotedText(annotation).trimEnd()}\n\`\`\``;
}

// The "Source:" line an earlier-boss-chat annotation adds to the prompt:
// the session and message the passage actually came from, plus the read
// that pulls the original turn — `goddard-agent read` resolves archived
// boss sessions by task id, and `turn` narrows to the message's turn.
function annotationSourceLine(
  annotation: ComposerDraftAnnotation,
): string | null {
  const history = annotation.history;
  if (!history) return null;
  const read =
    history.turn != null
      ? `goddard-agent read '{"task_id":"${history.session_id}","turn":${history.turn}}'`
      : `goddard-agent read '{"task_id":"${history.session_id}"}'`;
  return `Source: an earlier Boss chat — session ${history.session_id}, message ${annotation.message_id}. To pull the original turn, run \`${read}\`.`;
}

/**
 * The prompt block prepended to a submission carrying annotations. Each
 * passage is quoted and labelled so the agent can cite the comment's target;
 * the trailing instruction is what makes the labels usable. A passage
 * pinned on an earlier boss chat's rows also names its source session and
 * the read that reaches the original turn.
 */
export function annotationPromptPrefix(
  annotations: ComposerDraftAnnotation[],
): string {
  if (!annotations.length) return "";
  let out = "";
  annotations.forEach((annotation, index) => {
    out += `Annotation ${index + 1}:\n`;
    for (const line of annotationPromptPassage(annotation).split("\n")) {
      out += `> ${line}\n`;
    }
    out += `\nComment: ${(annotation.comment ?? "").trim()}\n`;
    const source = annotationSourceLine(annotation);
    if (source) out += `${source}\n`;
    out += "\n";
  });
  out +=
    'When responding, refer to the annotations above by their label (e.g. "Annotation 1") when appropriate.\n\n';
  return out;
}

/**
 * What the sent user bubble shows: each annotated passage as a quote block
 * with its comment, then the typed text — the transport's annotation context
 * without the `Annotation N` labels.
 */
export function annotationBubbleContent(
  annotations: ComposerDraftAnnotation[],
  typed: string,
): string {
  let out = "";
  for (const annotation of annotations) {
    for (const line of annotationPromptPassage(annotation).split("\n")) {
      out += `> ${line}\n`;
    }
    const comment = (annotation.comment ?? "").trim();
    if (comment) out += `\n${comment}\n`;
    out += "\n";
  }
  return out + typed;
}
