# State Module: RepositoryContentState
- **Responsibility:** Own repository-scoped specification and page discovery, including tree navigation, file metadata, and document-opening intents.
- **Data Shape:** Managed repository content roots; directory tree nodes keyed by repository and path; spec and page file metadata keyed by document id; selected tree node; visible file list; loading and error state.
- **Mutations/Actions:** `loadRepositoryContentRoots`; `loadTreeNodeChildren`; `selectContentNode`; `refreshRepositoryContent`; `openDocument`; `clearContentError`.
- **Scope & Hoisting:** Hoisted into a shared provider because spec discovery is reused by the specs page, document breadcrumbs, and global search.
- **Side Effects:** Reads repository-local spec and page metadata from managed repositories; resolves document-opening requests into `WorkbenchTabsState` so files open as MDX detail tabs.
