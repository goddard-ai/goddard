# Keep Boss instructions through chat refreshes

Use instruction pins for obligations that must remain visible until a condition
is met: a paused workflow, a temporary restriction, or a decision awaiting your
confirmation. Enable the Boss experiment, then open **Boss → Pinned instructions**.

Pins complement personas and memory buckets. Personas supply standing methods;
memory stores durable knowledge. Pins carry active obligations with a release
condition. Boss receives active pins and a compact current work brief on each
new prompt, including after a chat refresh or provider switch. Detailed past
conversations remain available through targeted history search and transcript
reads.

## Review an instruction

1. Open **Pinned instructions**. Boss may propose a candidate from a visible message
   you sent. Candidates are suggestions and do not impose instructions.
2. Choose **Review and activate** to inspect the saved source. Edit the instruction, choose
   its project scope, and enter the full condition for releasing it.
3. Choose **Save and activate**. The instruction now appears as active. Use
   **Add instruction** to enter one directly, without a source message.

For example:

| Field | Value |
| --- | --- |
| Instruction | Do not resume the paused summon chains. |
| Scope | Goddard project |
| Retire when | The fix has landed and I have confirmed a daemon restart. |
| Release method | Manual evidence |

Detection can miss an instruction or propose an irrelevant candidate. You or
Boss can dismiss a candidate, edit its scope, or add a missing instruction.
Without a configured Jev backend, detection uses conservative wording checks.

## Release an instruction

Boss can retire an instruction with evidence that its condition was met, or
mark it superseded by an active replacement when you change the instruction.
Uncertain conditions remain active. **Release instruction** is your explicit override.
Released instructions remain in the **Retired** view with their evidence.

You can opt into automatic release when a particular outcome completes with
evidence. Use this only when that completion is the entire release condition.
Cancellation, a failed assignment, and an employee finishing are insufficient.
For the example above, completing the repair alone would not prove your restart
confirmation; keep the release method manual. Editing the condition resets an
automatic release link to manual evidence.

Pins survive daemon restarts, Boss chat refreshes, provider switches, and
clearing Boss chat. Clearing chat removes transcripts; the pin retains its
saved source excerpt. Use **Release instruction** to remove an active obligation.

## Manage pins from the Boss CLI

The main Boss chat and a human can use these commands. Planning chats and
employees cannot read or manage the pin catalog, including employees granted
Boss coordination authority. Boss should carry applicable constraints when
delegating work.

```sh
goddard-agent boss instructions list
goddard-agent boss instructions list --all
```

The list contains IDs, revisions, states, and short source previews. Replace
`PIN_ID` and `REVISION` below with values from that list:

```sh
goddard-agent boss instructions show PIN_ID
goddard-agent boss instructions edit PIN_ID --revision REVISION \
  --text 'Do not resume the paused summon chains.' \
  --retire-when 'The fix has landed and the user confirms a daemon restart.'
goddard-agent boss instructions retire PIN_ID --revision REVISION \
  --evidence 'The user confirmed the restart after the fix landed.'
```

Use the new revision returned by each edit. A stale revision refuses the change
so another client cannot accidentally restore a released instruction. Boss
must cite a visible human message when creating a pin; `--help` documents the
source fields and optional outcome link.

Active instructions share a bounded context budget. A change exceeding it is
refused with a request to shorten or release another pin; an active instruction
is never silently dropped to make room. Source excerpts may be shortened, and
`show` identifies that with `quoteTruncated`.

See [Boss concepts](boss-concepts.md) for outcomes, personas, and memory.
