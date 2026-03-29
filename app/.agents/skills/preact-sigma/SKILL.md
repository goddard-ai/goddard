---
name: preact-sigma
description: Expert instructions and guidelines for working with the preact-sigma library.
---

# preact-sigma

Expert instructions and guidelines for developing with `preact-sigma`, a class-builder state API built on top of `@preact/signals` and `immer`.

## When to use

Use this skill when developing, refactoring, or reviewing code that leverages `preact-sigma` for state management, particularly when defining or interacting with `SigmaType` instances, computed signals, actions, and queries.

## Instructions

1. **Understand Core Concepts:**
   - **sigma type:** The builder returned by `new SigmaType<...>()`. It is also the constructor for sigma-state instances after configuration.
   - **sigma state:** An instance created from a configured sigma type.
   - **computed:** Argument-free derived state declared with `.computed({ ... })`.
   - **query:** A tracked method declared with `.queries({ ... })` or created with `query(fn)`.
   - **action:** A method declared with `.actions({ ... })` that reads and writes through one Immer draft for one synchronous call.

2. **Follow Type Inference and Architectural Rules:**
   - Prefer explicit type arguments only on `new SigmaType<TState, TEvents>()`. Let builder methods infer from their inputs (do not pass explicit types to `.defaultState(...)`, `.actions(...)`, etc.).
   - The `SigmaType` builder is additive. Builder methods mutate the same builder and return it.

3. **Respect Draft Boundaries in Actions:**
   - `emit()` is a draft boundary.
   - Any action call is a draft boundary unless it is a same-instance sync nested action call. (Cross-instance or async calls are draft boundaries).
   - `await` inside an async action is a draft boundary.
   - Call `this.commit()` _only_ when the current action has unpublished draft changes and is about to cross a draft boundary. A synchronous action that doesn't cross a boundary doesn't need it.
   - Successful publishes deep-freeze draftable public state (if auto-freezing is enabled). Non-plain objects need `[immerable] = true` to participate.

4. **Refer to Extended API Details:**
   - For full details on API exports, instance shape, events, Preact hooks, advanced utilities, and advanced action rules, see the extended reference: [api-details.md](./api-details.md).

## Example Pattern

```typescript
import { SigmaType } from "preact-sigma";

type Todo = {
  id: string;
  title: string;
  completed: boolean;
};

type TodoListState = {
  draft: string;
  todos: Todo[];
};

type TodoListEvents = {
  added: Todo;
};

const TodoList = new SigmaType<TodoListState, TodoListEvents>()
  .defaultState({
    draft: "",
    todos: [],
  })
  .computed({
    completedCount() {
      return this.todos.filter((todo) => todo.completed).length;
    },
  })
  .queries({
    canAddTodo() {
      return this.draft.trim().length > 0;
    },
  })
  .actions({
    setDraft(draft: string) {
      this.draft = draft;
    },
    addTodo() {
      const todo = {
        id: crypto.randomUUID(),
        title: this.draft,
        completed: false,
      };
      this.todos.push(todo);
      this.draft = "";
      this.commit(); // Needed if emit() is called, as emit() is a draft boundary
      this.emit("added", todo);
    },
  });

const todoList = new TodoList();
```
