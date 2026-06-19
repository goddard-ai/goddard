# Automation

> Some Goddard automation starts from named definitions or multi-agent coordination instead of a user typing into one live session. This section explains those daemon-owned automation surfaces and how clients observe or control them.

## Purpose

- This folder explains daemon-owned automation surfaces that create or coordinate agent work beyond direct interactive sessions.

## Named automation

- [Actions](./actions.md)
  - Reusable one-shot execution definitions that create daemon-managed sessions.
- [Loops](./loops.md)
  - Reusable daemon-owned runtimes that can be started, inspected, listed, and shut down.

## Multi-agent automation

- [Workforce](./workforce.md)
  - Daemon-owned multi-agent orchestration for one repository workspace.
- [Workforce requests](./workforce-requests.md)
  - Request, update, cancel, truncate, respond, and suspend behavior.
- [Workforce suspension and recovery](./workforce-suspension-and-recovery.md)
  - Suspended work, validation failures, restart recovery, and safe shutdown.
