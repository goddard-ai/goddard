# Select Popover Positioning Repro

This repro isolates the Ark `Popover` setup behind the session-input selector bug.

The test keeps the popover closed on mount and then opens it through controlled state, which matches the session-input selector flow.

It confirms that when the `Popover.Positioner` is created lazily, the floating node is missing during Zag's first placement pass and the positioner never receives `--x`, `--y`, or `--z-index`. The kept-mounted variant in the same file receives non-empty positioning variables under the same JSDOM geometry stubs.

Run it with:

```sh
bun --cwd app run test:bug:select-popover-positioning
```

Or run all bug repros through the shared harness:

```sh
bun --cwd app run test:bugs
```
