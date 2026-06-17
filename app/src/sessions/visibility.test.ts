import { createFixtureSession } from "@goddard-ai/fixtures"
import { expect, test } from "bun:test"

import { filterPrimarySessions } from "./visibility.ts"

test("filterPrimarySessions keeps only app-visible non-hidden sessions", () => {
  const visible = createFixtureSession({ id: "ses_visible" })
  const hidden = createFixtureSession({
    id: "ses_hidden",
    origin: "pipeline",
    visibility: "hidden",
  })
  const completedHidden = createFixtureSession({
    id: "ses_completed_hidden",
    completedHidden: true,
  })

  expect(filterPrimarySessions([hidden, visible, completedHidden])).toEqual([visible])
})
