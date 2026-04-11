# Tree Collection

Manage hierarchical data such as file systems, navigation menus, or organization charts with `createTreeCollection`.

## Contents

- Create a tree
- Navigate nodes
- Read relationships
- Work with index paths
- Query the tree
- Mutate immutably
- Flatten and inspect values

## Create a Tree

```ts
import { createTreeCollection } from '@ark-ui/react/collection'

const treeData = {
  value: 'root',
  label: 'Root',
  children: [
    {
      value: 'folder1',
      label: 'Folder 1',
      children: [
        { value: 'file1', label: 'File 1.txt' },
        { value: 'file2', label: 'File 2.txt' },
      ],
    },
    {
      value: 'folder2',
      label: 'Folder 2',
      children: [
        {
          value: 'subfolder1',
          label: 'Subfolder 1',
          children: [{ value: 'file3', label: 'File 3.txt' }],
        },
      ],
    },
  ],
}

const tree = createTreeCollection({ rootNode: treeData })
```

## Navigation Methods

### Get First and Last Nodes

```ts
const firstNode = tree.getFirstNode()
console.log(firstNode?.value) // "folder1"

const lastNode = tree.getLastNode()
console.log(lastNode?.value) // "folder2"
```

### Navigate Sequentially

```ts
const nextNode = tree.getNextNode('file1')
console.log(nextNode?.value) // "file2"

const previousNode = tree.getPreviousNode('file2')
console.log(previousNode?.value) // "file1"
```

## Hierarchical Relationships

### Read Parents and Children

```ts
const parentNode = tree.getParentNode('file1')
console.log(parentNode?.value) // "folder1"

const ancestors = tree.getParentNodes('file3')
console.log(ancestors.map((n) => n.value)) // ["folder2", "subfolder1"]

const descendants = tree.getDescendantNodes('folder1')
console.log(descendants.map((n) => n.value)) // ["file1", "file2"]

const descendantValues = tree.getDescendantValues('folder2')
console.log(descendantValues) // ["subfolder1", "file3"]
```

### Navigate Siblings

```ts
const indexPath = tree.getIndexPath('file1') // [0, 0]

const nextSibling = tree.getNextSibling(indexPath)
console.log(nextSibling?.value) // "file2"

const previousSibling = tree.getPreviousSibling(indexPath)
console.log(previousSibling) // undefined

const siblings = tree.getSiblingNodes(indexPath)
console.log(siblings.map((n) => n.value)) // ["file1", "file2"]
```

## Index Path Operations

```ts
const indexPath = tree.getIndexPath('file3')
console.log(indexPath) // [1, 0, 0]

const value = tree.getValue([1, 0, 0])
console.log(value) // "file3"

const valuePath = tree.getValuePath([1, 0, 0])
console.log(valuePath) // ["folder2", "subfolder1", "file3"]

const node = tree.at([1, 0])
console.log(node?.value) // "subfolder1"
```

## Tree Queries

### Check Branches

```ts
const folder1Node = tree.findNode('folder1')
const isBranch = tree.isBranchNode(folder1Node!)
console.log(isBranch) // true

const branchValues = tree.getBranchValues()
console.log(branchValues) // ["folder1", "folder2", "subfolder1"]
```

### Traverse with Custom Logic

```ts
tree.visit({
  onEnter: (node, indexPath) => {
    console.log(`Visiting: ${node.value} at depth ${indexPath.length}`)

    if (node.value === 'folder2') {
      return 'skip'
    }
  },
})
```

### Filter Nodes

```ts
const filteredTree = tree.filter((node) => {
  return node.value.includes('file')
})

console.log(filteredTree.getValues()) // ["file1", "file2", "file3"]
```

## Tree Manipulation

### Add Nodes

```ts
const newFile = { value: 'newfile', label: 'New File.txt' }

const indexPath = tree.getIndexPath('file1')
const updatedTree = tree.insertAfter(indexPath!, [newFile])

const updatedTree2 = tree.insertBefore(indexPath!, [newFile])
```

### Remove Nodes

```ts
const indexPath = tree.getIndexPath('file2')
const updatedTree = tree.remove([indexPath!])

console.log(updatedTree.getValues()) // file2 is removed
```

### Move Nodes

```ts
const fromIndexPaths = [tree.getIndexPath('file1')!]
const toIndexPath = tree.getIndexPath('folder2')!

const updatedTree = tree.move(fromIndexPaths, toIndexPath)
```

### Replace Nodes

```ts
const indexPath = tree.getIndexPath('file1')!
const newNode = { value: 'replacedfile', label: 'Replaced File.txt' }

const updatedTree = tree.replace(indexPath, newNode)
```

## Utility Methods

### Flatten the Tree

```ts
const flatNodes = tree.flatten()
console.log(flatNodes.map((n) => ({ value: n.value, depth: n._indexPath.length })))
// [{ value: "folder1", depth: 1 }, { value: "file1", depth: 2 }, ...]
```

### Get All Values

```ts
const allValues = tree.getValues()
console.log(allValues) // ["folder1", "file1", "file2", "folder2", "subfolder1", "file3"]
```

### Calculate Depth

```ts
const depth = tree.getDepth('file3')
console.log(depth) // 3 (root -> folder2 -> subfolder1 -> file3)
```

## Custom Node Types

```ts
interface CustomNode {
  id: string
  name: string
  items?: CustomNode[]
  isDisabled?: boolean
}

const customTree = createTreeCollection<CustomNode>({
  rootNode: {
    id: 'root',
    name: 'Root',
    items: [
      { id: '1', name: 'Item 1', isDisabled: false },
      { id: '2', name: 'Item 2', isDisabled: true },
    ],
  },
  nodeToValue: (node) => node.id,
  nodeToString: (node) => node.name,
  nodeToChildren: (node) => node.items,
  isNodeDisabled: (node) => node.isDisabled ?? false,
})
```

## Create a File Tree from Paths

```ts
import { createFileTreeCollection } from '@ark-ui/react/collection'

const paths = ['src/components/Button.tsx', 'src/components/Input.tsx', 'src/utils/helpers.ts', 'docs/README.md']

const fileTree = createFileTreeCollection(paths)
console.log(fileTree.getBranchValues()) // ["src", "components", "utils", "docs"]
```

Tree collections are immutable. Use the new collection returned by each mutation method instead of expecting in-place updates.
