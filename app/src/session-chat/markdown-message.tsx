import { cx } from "@goddard-ai/styled-system/css";
import { createSerializedParse, type ComarkTree } from "comark";
import highlight from "comark/plugins/highlight";
import { ComarkRenderer } from "preact-comark";
import { useEffect, useRef, useState } from "preact/hooks";
import githubDarkDefault from "shiki/dist/themes/github-dark-default.mjs";
import githubLightDefault from "shiki/dist/themes/github-light-default.mjs";

import styles from "./markdown-message.style.ts";

function createMessageParser() {
  return createSerializedParse({
    html: false,
    plugins: [
      highlight({
        registerDefaultThemes: false,
        themes: {
          light: githubLightDefault,
          dark: githubDarkDefault,
        },
        preStyles: true,
      }),
    ],
  });
}

export function MarkdownMessage(props: {
  markdown: string;
  role: "assistant" | "user";
  streaming?: boolean;
}) {
  const parserRef = useRef<ReturnType<typeof createMessageParser> | null>(null);

  if (!parserRef.current) {
    parserRef.current = createMessageParser();
  }

  const [tree, setTree] = useState<ComarkTree | null>(null);
  const renderClass =
    props.role === "user"
      ? cx(styles.content, styles.user)
      : cx(styles.content, styles.assistant);

  useEffect(() => {
    let cancelled = false;

    void parserRef.current!(props.markdown, {
      streaming: props.streaming,
    })
      .then((nextTree) => {
        if (cancelled) {
          return;
        }

        setTree(nextTree);
      })
      .catch((error) => {
        console.error("Failed to parse session-chat markdown.", error);

        if (cancelled) {
          return;
        }

        setTree({
          frontmatter: {},
          meta: {},
          nodes: [props.markdown],
        });
      });

    return () => {
      cancelled = true;
    };
  }, [props.markdown, props.streaming]);

  if (!tree) {
    return <div class={cx(renderClass, styles.fallback)}>{props.markdown}</div>;
  }

  return (
    <ComarkRenderer
      caret={props.streaming ? { class: styles.caret } : false}
      className={renderClass}
      streaming={props.streaming}
      tree={tree}
    />
  );
}
