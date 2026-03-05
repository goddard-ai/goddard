---
id: app-screen-pages
status: ACTIVE
links:
  - type: Extends
    target: spec/app/index.md
  - type: Relates-To
    target: spec/product.md
---

# Pages Screen

## Goal
Provide repository-scoped access to project pages and page-level navigation in the desktop app.

## Hypothesis
We believe that fast access to repository pages from the same workspace improves execution continuity and documentation consumption.

## Actors
- Developer reviewing project pages
- Maintainer moving between specs, pages, and execution views

## Conceptual State Machine
`Repo Selection Required -> Repo Selected -> Page List Ready -> Page Opened`

## Behavioral Contract
- Entering Pages domain opens full-screen repository selection modal.
- Modal includes default frequent repositories and searchable selection.
- After repository selection:
  - list all available pages for that repository
  - selecting an item opens the page in tab content
- Individual page presentation follows Foxtrot layout principles.

## Data Requirements
- Repository selector data
  - repository id/name
  - frequent repository ranking
- Page index data
  - page id/path
  - title/label
  - repository linkage
- Page view data
  - rendered content
  - update metadata

## Constraints
- Must keep page navigation explicitly repository-bound.
- Must support a large number of pages without collapsing discoverability.

## Non-Goals
- Replacing external CMS/editor workflows for page authoring depth.
- Defining Foxtrot implementation mechanics here.
