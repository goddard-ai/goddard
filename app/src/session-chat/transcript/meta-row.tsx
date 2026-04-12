import { metaAuthorClass, metaRowClass, metaTimestampClass } from "./styles.ts"

/** Renders the shared metadata row used above transcript bubbles and tool cards. */
export function TranscriptMetaRow(props: {
  authorName: string
  timestampLabel: string
  alignmentStyle: { justifyContent: string }
}) {
  return (
    <div class={metaRowClass} style={props.alignmentStyle}>
      <span class={metaAuthorClass}>{props.authorName}</span>
      <span class={metaTimestampClass}>{props.timestampLabel}</span>
    </div>
  )
}
