import { defineGlobalStyles } from "@pandacss/dev"

export const globalCss = defineGlobalStyles({
  ":where(main)": {
    minInlineSize: "0",
    padding: "0.75rem",
  },

  ":where(header, footer, nav, section, article, aside, form, fieldset)": {
    minInlineSize: "0",
  },

  ":where(section, article, aside, nav, form, fieldset) + :where(section, article, aside, nav, form, fieldset)":
    {
      marginBlockStart: "0.75rem",
    },

  ":where(h1, h2, h3, h4, h5, h6)": {
    lineHeight: "1.2",
  },

  ":where(h1, h2, h3, h4, h5, h6) + :where(p, ul, ol, dl, table, form)": {
    marginBlockStart: "0.25rem",
  },

  ":where(p, ul, ol, dl, table, fieldset, pre, hr) + :where(p, ul, ol, dl, table, fieldset, pre, hr)":
    {
      marginBlockStart: "0.5rem",
    },

  ":where(ul)": {
    listStyle: "disc",
    paddingInlineStart: "1.125rem",
  },

  ":where(ol)": {
    listStyle: "decimal",
    paddingInlineStart: "1.125rem",
  },

  ":where(li + li)": {
    marginBlockStart: "0.125rem",
  },

  ":where(dl)": {
    display: "grid",
    gridTemplateColumns: "max-content minmax(0, 1fr)",
    columnGap: "0.5rem",
    rowGap: "0.125rem",
  },

  ":where(dt)": {
    color: "fg.muted",
  },

  ":where(dd)": {
    minInlineSize: "0",
  },

  ":where(fieldset)": {
    padding: "0.5rem 0.625rem 0.625rem",
    border: "1px solid {colors.border}",
    borderRadius: "0.375rem",
  },

  ":where(legend)": {
    paddingInline: "0.25rem",
    color: "fg.muted",
  },

  ":where(label)": {
    display: "block",
    marginBlockEnd: "0.25rem",
  },

  ":where(input:not([type='checkbox'], [type='radio'], [type='range'], [type='color'], [type='file']), select, textarea)":
    {
      inlineSize: "100%",
      minInlineSize: "0",
      minBlockSize: "2rem",
      padding: "0.375rem 0.5rem",
      border: "1px solid {colors.border}",
      borderRadius: "0.375rem",
      background: "bg.canvas",
    },

  ":where(button, input[type='button'], input[type='submit'], input[type='reset'])": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "0.375rem",
    minBlockSize: "2rem",
    paddingInline: "0.625rem",
    paddingBlock: "0.375rem",
    border: "1px solid {colors.border}",
    borderRadius: "0.375rem",
    background: "bg.surface",
    whiteSpace: "nowrap",
    cursor: "pointer",
  },

  ":where(th, td)": {
    padding: "0.375rem 0.5rem",
    textAlign: "left",
    verticalAlign: "top",
    borderBlockEnd: "1px solid {colors.border}",
  },

  ":where(th)": {
    color: "fg.muted",
    fontWeight: "600",
  },

  ":where(pre)": {
    overflow: "auto",
    padding: "0.5rem",
    border: "1px solid {colors.border}",
    borderRadius: "0.375rem",
    background: "bg.surface",
  },

  ":where(hr)": {
    marginBlock: "0.75rem",
    borderTopColor: "border",
  },

  ":where(a, button, input, select, textarea, summary, [tabindex], [data-scope][data-part]):focus-visible":
    {
      outline: "2px solid {colors.accent}",
      outlineOffset: "2px",
    },

  ":where([data-scope='select'][data-part='trigger'], [data-scope='combobox'][data-part='input'], [data-scope='number-input'][data-part='input'], [data-scope='editable'][data-part='input'], [data-scope='pin-input'][data-part='input'])":
    {
      inlineSize: "100%",
      minInlineSize: "0",
      minBlockSize: "2rem",
      padding: "0.375rem 0.5rem",
      border: "1px solid {colors.border}",
      borderRadius: "0.375rem",
      background: "bg.canvas",
    },

  ":where([data-scope='select'][data-part='trigger'])": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "0.5rem",
  },

  ":where([data-scope='dialog'][data-part='content'], [data-scope='popover'][data-part='content'], [data-scope='hover-card'][data-part='content'])":
    {
      minInlineSize: "16rem",
      maxInlineSize: "min(40rem, calc(100vw - 1rem))",
      padding: "0.625rem",
      border: "1px solid {colors.border}",
      borderRadius: "0.5rem",
      background: "bg.panel",
    },

  ":where([data-scope='menu'][data-part='content'], [data-scope='select'][data-part='content'], [data-scope='combobox'][data-part='content'], [data-scope='listbox'][data-part='content'])":
    {
      minInlineSize: "12rem",
      maxInlineSize: "min(32rem, calc(100vw - 1rem))",
      maxBlockSize: "min(20rem, calc(100dvh - 2rem))",
      overflow: "auto",
      padding: "0.25rem",
      border: "1px solid {colors.border}",
      borderRadius: "0.5rem",
      background: "bg.panel",
    },

  ":where([data-scope='menu'][data-part='item'], [data-scope='select'][data-part='item'], [data-scope='combobox'][data-part='item'], [data-scope='listbox'][data-part='item'])":
    {
      display: "flex",
      alignItems: "center",
      gap: "0.5rem",
      minBlockSize: "2rem",
      padding: "0.375rem 0.5rem",
      borderRadius: "0.375rem",
      cursor: "pointer",
    },

  ":where([data-scope='menu'][data-part='item'][data-highlighted], [data-scope='select'][data-part='item'][data-highlighted], [data-scope='combobox'][data-part='item'][data-highlighted], [data-scope='listbox'][data-part='item'][data-highlighted])":
    {
      background: "bg.surface",
    },

  ":where([data-scope='menu'][data-part='item'][data-state='checked'], [data-scope='select'][data-part='item'][data-state='checked'], [data-scope='combobox'][data-part='item'][data-state='checked'], [data-scope='listbox'][data-part='item'][data-selected])":
    {
      background: "bg.surface",
    },

  ":where([data-scope='tabs'][data-part='list'], [data-scope='segment-group'][data-part='root'])": {
    position: "relative",
    display: "inline-flex",
    flexWrap: "wrap",
    gap: "0.125rem",
    padding: "0.125rem",
    border: "1px solid {colors.border}",
    borderRadius: "0.5rem",
    background: "bg.surface",
  },

  ":where([data-scope='tabs'][data-part='trigger'], [data-scope='segment-group'][data-part='item'])":
    {
      position: "relative",
      zIndex: "1",
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      minBlockSize: "1.875rem",
      paddingInline: "0.5rem",
      borderRadius: "0.375rem",
      whiteSpace: "nowrap",
    },

  ":where([data-scope='tabs'][data-part='indicator'], [data-scope='segment-group'][data-part='indicator'])":
    {
      border: "1px solid {colors.border}",
      borderRadius: "0.375rem",
      background: "bg.canvas",
    },

  ":where([data-scope='accordion'][data-part='item'])": {
    borderBlockEnd: "1px solid {colors.border}",
  },

  ":where([data-scope='accordion'][data-part='trigger'], [data-scope='collapsible'][data-part='trigger'])":
    {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: "0.5rem",
      inlineSize: "100%",
      paddingBlock: "0.5rem",
      textAlign: "left",
    },

  ":where([data-scope='accordion'][data-part='content'], [data-scope='collapsible'][data-part='content'])":
    {
      paddingBlockEnd: "0.5rem",
    },

  ":where([data-scope='collapsible'][data-part='indicator'])": {
    transition: "transform 160ms ease",
  },

  ":where([data-scope='collapsible'][data-part='indicator'][data-state='open'])": {
    transform: "rotate(180deg)",
  },
})
