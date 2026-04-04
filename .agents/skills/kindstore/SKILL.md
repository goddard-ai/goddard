---
name: kindstore
description: Model, query, and migrate kindstore-based document stores with example-first guidance. Use when Codex needs to design kinds, choose tags and indexes, write collection queries, use findPage keyset pagination, store metadata, batch writes, or evolve schemas with payload or structural migrations while staying inside kindstore's intentionally narrow typed query model.
---

# Kindstore

Inspect the repository before editing. Match the existing kind declarations,
indexes, query shapes, and migration style instead of assuming they should look
exactly like these examples.

kindstore is a registry-driven document store. It is not a general ORM.

When opening an existing store, treat `UnrecoverableStoreOpenError` as the
signal that the persisted internal metadata or store format cannot be opened
safely by the current code. Downstream code that accepts data loss may catch
that error and wipe the store before retrying; ordinary schema declaration,
structural migration, or payload migration errors should still be handled as
normal application failures.

## Start from real kinds

```ts
import { z } from "zod";
import { kind, kindstore } from "kindstore";

const Task = z.object({
  title: z.string(),
  status: z.enum(["todo", "doing", "done"]),
  assigneeId: z.string().optional(),
});

const db = kindstore({
  filename: ":memory:",
  schema: {
    tasks: kind("tsk", Task)
      .createdAt()
      .updatedAt()
      .index("status")
      .index("assigneeId")
      .index("updatedAt")
      .multi("status_updatedAt", {
        status: "asc",
        updatedAt: "desc",
      }),
  },
});
```

When you need the inferred input or output shape for a declared kind, you can
derive it directly from the builder instance:

```ts
import type { KindInput, KindOutput } from "kindstore";

const schema = {
  tasks: kind("tsk", Task).index("status"),
};

type TaskInput = KindInput<typeof schema.tasks>;
type TaskOutput = KindOutput<typeof schema.tasks>;
```

When you already have a store instance, the same builders are also available at
`db.schema.<kindKey>`, so `KindInput<typeof db.schema.tasks>` and
`KindOutput<typeof db.schema.tasks>` work without keeping a separate schema bag
in scope.

Use this shape when you need:

- stable tagged IDs like `tsk_...`
- typed payload validation with Zod
- typed filtering on declared query fields only
- deterministic ordering on declared query fields

`.multi(...)` may reference any top-level payload field, and it may also
include the store-managed `id`, even if those fields do not also have their own
`.index(...)`. kindstore derives any needed generated columns for payload
fields automatically, while `id` reuses the existing row ID column. Add a
standalone `.index(...)` too when you want a dedicated single-field SQLite
index or need an explicit SQLite type hint.

When a kind uses `.createdAt()` or `.updatedAt()`, kindstore adds integer
timestamp fields to the schema if they are missing. If the field already exists
in the Zod object, keep it as an integer timestamp field.

## Model for query shape, not payload size

```ts
const Message = z.object({
  threadId: z.string(),
  authorId: z.string(),
  body: z.string(),
  sentAt: z.number().int(),
  deliveredAt: z.number().int().nullable(),
});

const Messages = kind("msg", Message)
  .index("threadId")
  .index("authorId")
  .index("sentAt")
  .multi("thread_sentAt", {
    threadId: "asc",
    sentAt: "desc",
  });
```

That declaration is justified by queries like:

```ts
db.messages.findMany({
  where: { threadId: "thr_42" },
  orderBy: { sentAt: "desc" },
  limit: 50,
});
```

Do not index fields just because they exist in the payload. Index fields because
callers really filter or sort on them.

## Use the collection API directly

```ts
const created = db.tasks.create({
  title: "Ship docs",
  status: "todo",
});

const id = db.tasks.newId();

db.tasks.put(id, {
  title: "Ship docs",
  status: "todo",
});

const before = db.tasks.get(id);

db.tasks.update(id, {
  status: "doing",
});

db.tasks.update(id, (current) => ({
  ...current,
  title: current.title.trim(),
  status: "done",
}));

const after = db.tasks.get(id);
const removed = db.tasks.delete(id);
```

Read the API shape literally:

- `create(value)` allocates a fresh tagged ID and inserts one new document
- `newId()` returns a tagged ID for that kind
- `put(id, value)` is a replacement write for a known ID, not a merge
- `update(id, patch)` is a shallow partial update
- `update(id, fn)` is for computed next values
- `get(id)` and `update(id, ...)` return `undefined` when no row exists
- `delete(id)` returns `boolean`

Prefer `create()` when the store should allocate the ID as part of the write.
Reach for `newId()` plus `put()` when the caller needs the ID before writing,
such as optimistic state, batching related writes, or cross-record references.

## Reach for the right read method

```ts
const nextTask = db.tasks.first({
  where: { status: "todo" },
  orderBy: { updatedAt: "desc" },
});

const openTasks = db.tasks.findMany({
  where: {
    status: { in: ["todo", "doing"] },
    updatedAt: { gte: startOfDay },
  },
  orderBy: { updatedAt: "desc" },
  limit: 20,
});

for (const task of db.tasks.iterate({
  where: { assigneeId: "usr_1" },
  orderBy: { updatedAt: "desc" },
})) {
  console.log(task.title);
}
```

The typed query model supports:

```ts
where: {
  field: "exact",
  otherField: null,
  numericField: { gt: 10, lte: 20 },
  enumField: { in: ["a", "b"] },
}
```

Keep queries inside this boundary:

- only declared query fields are queryable: top-level payload fields declared in `.index(...)` or `.multi(...)`, plus `id` when it is included in `.multi(...)`
- ordering is only on declared query fields
- the operators are `in`, `gt`, `gte`, `lt`, and `lte`
- there is no arbitrary boolean composition or join support

## Paginate with `findPage()`

```ts
const firstPage = db.tasks.findPage({
  where: { status: "doing" },
  orderBy: { updatedAt: "desc" },
  limit: 20,
});

const secondPage = db.tasks.findPage({
  where: { status: "doing" },
  orderBy: { updatedAt: "desc" },
  limit: 20,
  after: firstPage.next,
});

for (const task of firstPage.items) {
  console.log(task.title);
}
```

Treat these as hard rules:

- `findPage()` requires `orderBy`
- `findPage()` requires a positive `limit`
- reuse the same `orderBy` when continuing with `after`
- prefer non-null ordered boundary fields across page boundaries
- read results from `{ items, next }`

## Add metadata when the data is store-scoped

```ts
const SyncCursor = z.object({
  cursor: z.string(),
  syncedAt: z.number().int(),
});

const db = kindstore({
  filename: ":memory:",
  metadata: {
    syncCursor: SyncCursor,
  },
  schema: {
    tasks: kind("tsk", Task).index("status"),
  },
});

db.metadata.set("syncCursor", {
  cursor: "cursor_123",
  syncedAt: Date.now(),
});

const cursor = db.metadata.get("syncCursor");

db.metadata.update("syncCursor", (current) => ({
  cursor: current?.cursor ?? "cursor_123",
  syncedAt: Date.now(),
}));
```

Here `syncedAt` is just an ordinary metadata field in the payload. kindstore
does not add or maintain metadata timestamps for you.

Use metadata for small typed values like preferences, checkpoints, cursors, or
feature flags. kindstore does not add store-managed metadata timestamps, so if
you need them, include them in the metadata schema yourself. If the value needs
many records, indexing, or per-record IDs, make it a real kind instead.

## Batch related writes together

```ts
db.batch(() => {
  const id = db.tasks.newId();

  db.tasks.put(id, {
    title: "Write release notes",
    status: "todo",
  });

  db.metadata.set("syncCursor", {
    cursor: id,
    syncedAt: Date.now(),
  });
});
```

Use `batch()` when those writes should succeed or fail together.

## Show payload migrations in code

```ts
const TaskV2 = z.object({
  title: z.string(),
  status: z.enum(["todo", "doing", "done"]),
});

const db = kindstore({
  filename: ":memory:",
  schema: {
    tasks: kind("tsk", TaskV2)
      .updatedAt()
      .index("status")
      .index("updatedAt")
      .migrate(2, {
        1: (value) => ({
          ...value,
          status: "todo",
        }),
      }),
  },
});
```

When you write one, remember the shape:

- `.migrate(currentVersion, steps)`
- each step key upgrades from that version to the next
- migration input should be treated as partial old payload
- migration output must satisfy the new schema
- `context.now` is available when the migration needs a store-managed timestamp

Example with `context.now`:

```ts
1: (value, context) => ({
  ...value,
  status: "todo",
  migratedAt: context.now,
})
```

## Show structural migrations in code

```ts
const WorkItem = z.object({
  title: z.string(),
  status: z.enum(["todo", "doing", "done"]),
});

const db = kindstore({
  filename: ":memory:",
  migrate(m) {
    m.rename("tasks", "workItems");
    m.retag("workItems", "tsk");
    m.drop("drafts");
  },
  schema: {
    workItems: kind("wrk", WorkItem).index("status"),
  },
});
```

Reach for store-level `migrate(...)` when the change affects persisted
ownership or identity:

- collection rename: `rename(previousKindKey, nextKindKey)`
- removed kind: `drop(previousKindKey)`
- changed tag prefix: `retag(kindKey, previousTag)`

If a previous kind disappears or a tag changes without explicit migration,
startup should fail. That is intentional.

## Use raw SQL as an escape hatch

```ts
const row = db.raw
  .query(`SELECT count(*) AS count FROM "tasks" WHERE "status" = ?`)
  .get("todo") as { count: number };
```

Prefer the typed APIs for normal reads and writes. Use `db.raw` for operational
inspection or queries that do not fit kindstore's narrow typed model.

## Write code in this style

Prefer snippets like these over generic advice:

```ts
db.posts.findMany({
  where: { authorId: "usr_1", status: "published" },
  orderBy: { updatedAt: "desc" },
  limit: 10,
});
```

```ts
db.posts.update(id, (current) => ({
  ...current,
  title: current.title.trim(),
}));
```

```ts
kind("pst", Post)
  .updatedAt()
  .index("authorId")
  .index("status")
  .index("updatedAt")
  .multi("author_status_updatedAt", {
    authorId: "asc",
    status: "asc",
    updatedAt: "desc",
  });
```

When editing an app that uses kindstore, make the resulting code answer these
questions clearly in code:

- Which fields are queryable?
- Which fields are store-managed timestamps?
- Is this write a replacement or an update?
- Is this read one row, many rows, a page, or an iterator?
- Is this schema change payload-level or structural?

## Validation

Run the relevant tests after changing code that uses kindstore.

Check actual application behavior around indexes, ordering, pagination, and
migrations instead of assuming a generic document-store pattern.
