import type { SessionTranscriptTurnStop } from "~/sessions/models.ts"
import styles from "./transcript.style.ts"

/** Renders one left-aligned lifecycle note after an abnormal terminal session turn. */
export function TurnStopTranscriptRow(props: {
  row: {
    item: SessionTranscriptTurnStop;
    index: number;
    viewportWidth: number;
  };
}) @{
  const message = props.row.item

  <article class={styles.row}>
    <div class={styles.rowInner}>
      <p class={styles.turnStopMessage} data-status={message.status}>
        <span class={styles.turnStopTitle}>{message.title}</span>

        @if (message.reason) {
          {
            " "
          }
          <span>{message.reason}</span>
        }
      </p>
    </div>
  </article>
}
