import { parse } from 'comark'
import { defineComarkPlugin } from 'comark/parse'
import { visit } from 'comark/utils'

const wordCount = defineComarkPlugin(() => ({
  name: 'word-count',
  post(state) {
    let count = 0

    visit(
      state.tree,
      (node) => typeof node === 'string',
      (node) => {
        count += String(node).trim().split(/\s+/).filter(Boolean).length
      },
    )

    state.tree.meta.wordCount = count
  },
}))

const tree = await parse('# Hello\n\nCount these words.', {
  plugins: [wordCount()],
})

console.log(tree.meta.wordCount)
