# State Module: ProjectContentState
- **Responsibility:** Own project-scoped specification and page discovery, including tree navigation, file metadata, and document-opening intents.
- **Data Shape:** Managed project content roots; directory tree nodes keyed by project and path; spec and page file metadata keyed by document id; selected tree node; visible file list; loading and error state.
- **Mutations/Actions:** `loadProjectContentRoots`; `loadTreeNodeChildren`; `selectContentNode`; `refreshProjectContent`; `openDocument`; `clearContentError`.
- **Scope & Hoisting:** Hoisted into a shared provider because spec discovery is reused by the specs page, document breadcrumbs, and global search.
- **Side Effects:** Reads project-local spec and page metadata from managed projects; resolves document-opening requests into `WorkbenchTabsState` so files open as markdown detail tabs.
