import type { Signal } from "@preact/signals"

import { flattenConfigOptionValues, type SelectSessionConfigOption } from "./config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "./select-menu.tsrx"

export component SessionModeSelector(props: {
  disabled?: boolean;
  modeOption: SelectSessionConfigOption | null;
  open: Signal<boolean>;
  onValueChange: (value: string) => void;
}) {
  const modeItems: SessionInputSelectItem[] =
    props.modeOption
      ? flattenConfigOptionValues(props.modeOption).map(
          (option) => ({
            value: option.value,
            label: option.name,
            detail: option.description ?? null,
            searchText: option.description ?? null,
          }),
        )
      : []

  <SessionInputSelect
    disabled={props.disabled || modeItems.length === 0}
    filterable={false}
    items={modeItems}
    label={props.modeOption?.name ?? "Approval preset"}
    open={props.open}
    placeholder="Use the adapter default"
    value={props.modeOption?.currentValue ?? null}
    onValueChange={props.onValueChange}
  />
}
