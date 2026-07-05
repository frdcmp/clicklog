import { useState } from 'react'
import { Button } from './Button'

export function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <Button
      type="button"
      variant="secondary"
      size="sm"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text)
          setCopied(true)
          setTimeout(() => setCopied(false), 1500)
        } catch {
          /* clipboard blocked — ignore */
        }
      }}
    >
      {copied ? 'Copied ✓' : label}
    </Button>
  )
}
