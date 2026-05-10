'use client'

import { useState } from 'react'
import { Comark } from '@comark/react'

export function AiChatTranscript() {
  const [content, setContent] = useState('')
  const [isStreaming, setIsStreaming] = useState(false)

  async function askAI(prompt: string) {
    setContent('')
    setIsStreaming(true)

    const response = await fetch('/api/chat', {
      method: 'POST',
      body: JSON.stringify({ prompt }),
    })

    const reader = response.body?.getReader()
    if (!reader) {
      setIsStreaming(false)
      return
    }

    const decoder = new TextDecoder()
    let accumulated = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      accumulated += decoder.decode(value, { stream: true })
      setContent(accumulated)
    }

    setIsStreaming(false)
  }

  return (
    <>
      <button onClick={() => askAI('Summarize the release notes.')}>
        Ask AI
      </button>
      <Comark streaming={isStreaming} caret>
        {content}
      </Comark>
    </>
  )
}
