import { t } from "@lingui/core/macro"
import { ChevronRight } from "lucide-react"

import type {
  SessionTranscriptToolContent,
  SessionTranscriptWorkDrawer,
} from "~/sessions/models.ts"
import { buildToolDiffPreview } from "./layout.ts"
import styles from "./transcript.style.ts"

function WorkDrawerToolContent(props: {
  content: SessionTranscriptToolContent;
}) @{
  if (props.content.type === "content") {
    const contentText = props.content.text?.trim()

    if (contentText) {
      return <div class={styles.workDrawerDetailText}>{contentText}</div>
    }

    return null
  } else if (props.content.type === "diff") {
    return <div class={styles.workDrawerDetailList}>
      @if (props.content.path) {
        <div class={styles.workDrawerDetailMono}>{props.content.path}</div>
      }

      <div class={styles.workDrawerDetailMono}>{buildToolDiffPreview(props.content)}</div>
    </div>
  }

  return <div class={styles.workDrawerDetailMono}>
    Terminal session
    {props.content.terminalId}
  </div>
}

export function WorkDrawerTranscriptRow(props: {
  message: SessionTranscriptWorkDrawer;
}) @{
  <article class={styles.row}>
    <div class={styles.rowInner} style={{ justifyContent: "flex-start" }}>
      <div class={styles.rowColumn} style={{ width: "100%" }}>
        <details class={styles.workDrawer} open={props.message.expandedByDefault}>
          <summary class={styles.workDrawerSummary}>
            <ChevronRight
              class={styles.workDrawerChevron}
              size={15}
              strokeWidth={2.2}
              aria-hidden="true"
            />
            <span class={styles.workDrawerTitle}>{props.message.title}</span>
          </summary>

          <ol class={styles.workDrawerItemList}>
            @for (const item of props.message.items) {
              <li class={styles.workDrawerItem} key={`${props.message.id}:${item.id}`}>
                @if (item.kind === "thought") {
                  <article class={styles.workDrawerThought}>
                    <div class={styles.workDrawerItemHeading}>
                      {t`Thought`}
                    </div>
                    <p class={styles.workDrawerDetailText}>{item.text}</p>
                  </article>
                } @else {
                  <article class={styles.workDrawerTool}>
                    <div class={styles.workDrawerItemHeading}>{item.title}</div>

                    @for (const content of item.content; index contentIndex) {
                      <WorkDrawerToolContent key={`${item.id}:content:${contentIndex}`} {content} />
                    }
                  </article>
                }
              </li>
            }
          </ol>
        </details>
      </div>
    </div>
  </article>
}
