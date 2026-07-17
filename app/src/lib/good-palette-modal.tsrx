import { type Signal, useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useEffect, useId } from "preact/hooks"

import { Modal, useSearchNavigation } from "@goddard-ai/ui-primitives"
import { isEmptyQuery } from "./search-query.ts"
import styles from "./good-palette-modal.style.ts"

export type GoodPaletteModalItem = {
  id: string;
  icon?: preact.ComponentChildren;
  keywords?: readonly string[];
  label: string;
  suffix?: preact.ComponentChildren;
}

function prepareGoodPaletteModalItems(items: readonly GoodPaletteModalItem[]) {
  return items.map(
    (item) => ({
      item,
      preparedKeywords: item.keywords ? fuzzysort.prepare(item.keywords.join("\n")) : null,
      preparedLabel: fuzzysort.prepare(item.label),
    }),
  )
}

export function GoodPaletteModal(props: {
  emptyText: string;
  items: readonly GoodPaletteModalItem[];
  listLabel: string;
  open: Signal<boolean>;
  placeholder: string;
  title: string;
  onSelect: (item: GoodPaletteModalItem) => void;
}) @{
  const titleId = useId()
  const listboxId = useId()
  const inputId = useId()
  const search = useSignal("")
  const preparedItems = prepareGoodPaletteModalItems(props.items)
  const visibleItems = (() => {
    if (isEmptyQuery(search.value)) {
      return props.items
    }

    return fuzzysort.searchFields(
      search.value,
      preparedItems,
      [
        { key: "label", extract: (entry) => entry.preparedLabel },
        { key: "keywords", extract: (entry) => entry.preparedKeywords },
      ],
      { threshold: 0 },
    ).items.map((entry) => entry.value.item)
  })()
  const visibleItemIds = visibleItems.map((item) => item.id).join("\n")

  const navigation = useSearchNavigation({
    activeAttribute: "aria-selected",
    count: () => visibleItems.length,
    onActivate(index) {
      const item = visibleItems[index]

      if (item) {
        props.onSelect(item)
      }
    },
    onQueryChange(query) {
      search.value = query
    },
  })

  useEffect(() => {
    if (!props.open.value && search.value.length > 0) {
      search.value = ""
    }

    navigation.resetActiveIndex()
  }, [props.open.value, search.value, visibleItemIds])

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>{props.title}</h2>

    <div
      class={styles.root}
      role="combobox"
      aria-controls={listboxId}
      aria-expanded={props.open.value}
      aria-haspopup="listbox"
      aria-labelledby={titleId}
      aria-owns={listboxId}
    >
      <div class={styles.searchRow}>
        <Search aria-hidden={true} class={styles.searchIcon} size={16} />
        <input
          ref={navigation.inputRef}
          id={inputId}
          autoFocus={true}
          class={styles.input}
          placeholder={props.placeholder}
          role="searchbox"
          type="text"
          value={search.value}
          aria-autocomplete="list"
          aria-controls={listboxId}
        />
      </div>
      <div id={listboxId} class={styles.list} role="listbox" aria-label={props.listLabel}>
        @if (visibleItems.length === 0) {
          <div class={styles.empty}>{props.emptyText}</div>
        } @else {
          @for (const item of visibleItems; index index) {
            const itemId = `${listboxId}-${item.id}`

            <div
              id={itemId}
              key={item.id}
              ref={navigation.itemRef(index)}
              class={styles.item}
              role="option"
              aria-selected="false"
              style={{
                gridTemplateColumns: item.icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
              }}
              onMouseDown={(event) => {
                event.preventDefault()
              }}
              onClick={() => {
                props.onSelect(item)
              }}
            >
              @if (item.icon) {
                <span class={styles.itemIcon}>{item.icon}</span>
              }

              <span class={styles.itemLabel}>{item.label}</span>

              @if (item.suffix) {
                <>
                  {item.suffix}
                </>
              }
            </div>
          }
        }
      </div>
    </div>
  </Modal>
}
