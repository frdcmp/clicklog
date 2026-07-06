import { useState } from 'react'
import { createPortal } from 'react-dom'
import { Button } from './Button'

// navigator.clipboard exists only in secure contexts (https / localhost); on a
// plain-http deployment fall back to a hidden textarea + execCommand.
async function copyToClipboard(text: string): Promise<boolean> {
  if (window.isSecureContext && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      /* fall through to the legacy path */
    }
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.setAttribute('readonly', '')
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }
  ta.remove()
  return ok
}

export function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <>
      <Button
        type="button"
        variant="secondary"
        size="sm"
        onClick={async () => {
          if (await copyToClipboard(text)) {
            setCopied(true)
            setTimeout(() => setCopied(false), 1800)
          }
        }}
      >
        {copied ? 'Copied ✓' : label}
      </Button>
      {copied &&
        createPortal(
          <div className="fixed left-1/2 top-4 z-50 -translate-x-1/2 rounded-md bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white shadow-lg">
            Copied to clipboard
          </div>,
          document.body,
        )}
    </>
  )
}
