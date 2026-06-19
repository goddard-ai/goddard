import { i18n } from "@lingui/core"

export const defaultLocale = "en"

export type Locale = typeof defaultLocale

export { i18n }

export async function activateLocale(locale: Locale) {
  const { messages } = await import(`../../locales/${locale}/messages.po`)

  i18n.load(locale, messages)
  i18n.activate(locale)
}
