import type { Signal } from "@preact/signals"

import { flattenConfigOptionValues, type SelectSessionConfigOption } from "./config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "./select-menu.tsrx"

export function SessionModeSelector(props: {
  disabled?: boolean;
  compact?: boolean;
  modeOption: SelectSessionConfigOption | null;
  open: Signal<boolean>;
  shortcut?: string | null;
  onValueChange: (value: string) => void;
}) @{
  <SessionConfigOptionSelector
    disabled={props.disabled}
    compact={props.compact}
    option={props.modeOption}
    open={props.open}
    label="Approval preset"
    placeholder="Use the adapter default"
    shortcut={props.shortcut}
    onValueChange={props.onValueChange}
  />
}

export function SessionConfigOptionSelector(props: {
  disabled?: boolean;
  compact?: boolean;
  label?: string;
  option: SelectSessionConfigOption | null;
  open: Signal<boolean>;
  placeholder?: string;
  shortcut?: string | null;
  onValueChange: (value: string) => void;
}) @{
  const optionItems: SessionInputSelectItem[] =
    props.option
      ? flattenConfigOptionValues(props.option).map(
          (option) => ({
            value: option.value,
            label: option.name,
            detail: option.description ?? null,
            searchText: option.description ?? null,
          }),
        )
      : []

  <SessionInputSelect
    compact={props.compact}
    disabled={props.disabled || optionItems.length === 0}
    filterable={false}
    items={optionItems}
    label={props.option?.name ?? props.label ?? "Session option"}
    open={props.open}
    placeholder={props.placeholder ?? "Use the adapter default"}
    shortcut={props.shortcut}
    value={props.option?.currentValue ?? null}
    onValueChange={props.onValueChange}
  />
}
