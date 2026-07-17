import { SigmaTarget } from "preact-sigma"

import type { WorkbenchTab } from "./workbench-tab-set.ts"

type WorkbenchTabLifecycleEvents = {
  closed: { tab: WorkbenchTab }
}

/** App-wide lifecycle events emitted when workbench tabs are removed from the tab set. */
export const workbenchTabLifecycle = new SigmaTarget<WorkbenchTabLifecycleEvents>()
