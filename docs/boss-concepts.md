# Boss concepts

Use these distinctions to direct delegated work and decide what is finished.
Boss is an experimental capability; this guide separates the conceptual model
from the limits of the current repository implementation. It does not promise
that every concept has a dedicated editor in your installed build.

## What each concept owns

| Concept | Meaning | What belongs here |
| --- | --- | --- |
| Employee | An agent carrying out work under a supervisor. | Agent identity and its conversation, execution, and workspace. |
| Assignment | The work brief given to an employee. | Scope, constraints, success criteria, and what to do with the result. |
| Outcome | The desired result, potentially requiring several assignments and employees. | What should be achieved and the criteria for accepting it as complete. |
| Plan | A reviewed design and organized course of work. | The approach, its rationale, and work items linked to assignments. |
| Persona | Reusable instructions for how an agent approaches a kind of work. | Methods, boundaries, and defaults that apply across assignments. |
| Memory | Durable, scoped knowledge available across sessions. | Prior decisions, facts, and lessons worth retrieving again. |

An employee's name identifies the agent. Its job title describes the work it
has been given. Its persona supplies reusable guidance. Changing the assignment
does not conceptually create a new identity, and a persona is neither an
employee nor a particular job.

## From a desired result to assigned work

Start with an outcome and explicit acceptance criteria. Break the work into
assignments that employees can complete and report independently. Use a plan
when the approach needs review or the work needs an organized breakdown.
Assignments can contribute to plan items as well as to the outcome.

These relationships are broader than a one-to-one checklist:

- One outcome can need multiple assignments and employees.
- One persona can guide many employees doing different assignments.
- One plan can organize multiple work items. An item can need several
  assignments, such as implementation and verification.
- An assignment can be standalone; it need not belong to an outcome or plan.

The current implementation records at most one outcome membership, one plan
link, and one item link on an employee. An outcome has at most one attached
plan, which must be approved and open when attached. A plan link can exist
without an item link. The system does not provide a general many-to-many
assignment graph or a separate history of assignment records for each employee.

### Changing an assignment

Conceptually, an assignment is an editable brief: revise its scope or success
criteria when the work changes, while keeping the employee's identity separate.

Today, the supervisor can redirect work through prompts and steering, change
the job title, and adjust supported settings such as workspace, permissions,
model, resources, and plan/item links. The assignment details display the
original summon brief; there is no dedicated editor that rewrites that brief.
Read the subsequent conversation for scope changes.

Outcome membership and completion behavior are captured when the assignment
starts. Resuming keeps those settings. Changing that contract requires stopping
and reassigning the work; editing a title or plan link does not change it.

## Approval and completion are separate

**Employee completion** means the employee's attempt has settled. Check the
result: it may have succeeded, failed, been interrupted, or raised a blocker.
Even a successful assignment may only supply one part of an outcome.

**Plan approval** accepts the proposed design. Finalization freezes the plan
document as the reviewed baseline. The ordered work-item list remains a live
map: it can be revised and its progress updated without rewriting that
baseline. Approval does not start or complete an outcome. The plan also has its
own completion or abandonment state, separate from the outcome's state.

**Outcome completion** accepts the desired result. Ordinary assignment success
leaves a handoff for a decision about the next step. A designated finishing
assignment can complete the outcome on accepted success, provided the closure
conditions still hold: other non-cancelled assignments have finished
successfully and pending handoffs have been resolved. Only one finishing
assignment can be live at a time. The owner can also explicitly complete an
outcome with evidence. No assignments remaining is not automatic completion.

## Personas and memory provide different context

A persona says how to work; memory says what has already been learned.
For example, a verification persona can require independent evidence, while
project memory records a known platform limitation to check.

Every employee composes the shared Employee base persona. On top of it, an
assignment can select one employee role: a shipped specialist — Researcher,
Feature Developer, Bug Investigator, or Verifier — or a custom persona you
have saved. Selecting a role changes only the instructions; memory buckets,
integrations, and other grants are assigned separately.

Memory is supplementary. Keep the original transcripts for what was said and
done, and plans for what was reviewed. A memory summary does not replace their
detail or establish that an outcome's acceptance criteria were met.

Current memory access uses named buckets. Employees can access their assigned
project's shared bucket and additional explicitly granted buckets. Personal
knowledge is not automatically shared with every employee. Pinned documents
and memory grants are separate. An assignment or persona can impose stricter
rules, including reporting findings instead of writing memory.

## Example: reliable offline file transfers

The outcome is: **Sending a file to an offline friend fails clearly and keeps
the file available to retry.** Acceptance criteria cover the visible failure,
the logged cause, and a successful retry after the friend reconnects.

The plan proposes detecting the offline condition, preserving the file, and
verifying recovery. Approval accepts that approach; it does not mean the
transfer behavior has changed.

A first employee uses an investigation persona to identify the failure path.
Its assignment ends with evidence and a recommended fix. A second employee
uses an implementation persona to make the fix. A third uses a verification
persona to reproduce the offline case and test recovery. Their assignments
link to the relevant plan items and serve the same outcome.

If verification finds that retry loses the file, the verifier has finished its
attempt but the outcome remains open. Revise the work and assign the repair.
Once the acceptance evidence is available and outstanding handoffs are
resolved, the designated finishing assignment or owner can close the outcome.
A durable platform caveat can then inform future work through scoped memory;
the transcripts and approved plan remain the original records.

For the surrounding task and workspace workflow, return to the
[user guide](../WIKI.md#core-concepts).
