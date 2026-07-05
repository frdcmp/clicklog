import { useState, type FormEvent } from 'react'
import { PageHeader } from '../components/layout/PageHeader'
import { Card } from '../components/ui/Card'
import { Button } from '../components/ui/Button'
import { Field, Input } from '../components/ui/Field'
import { Badge } from '../components/ui/Badge'
import { Modal } from '../components/ui/Modal'
import { CopyButton } from '../components/ui/CopyButton'
import { EmptyState, ErrorNote, Spinner } from '../components/ui/Feedback'
import { useToast } from '../components/ui/Toast'
import { useKeys, useMintKey, useRevokeKey } from '../query/keys'
import { useTenants } from '../query/tenants'
import type { MintResult } from '../api/keys'
import type { ApiKey } from '../types'
import { fmtTs } from '../lib/time'

export function KeysPage() {
  const keys = useKeys()
  const tenants = useTenants()
  const [mintOpen, setMintOpen] = useState(false)
  const [minted, setMinted] = useState<MintResult | null>(null)
  const [revoking, setRevoking] = useState<ApiKey | null>(null)

  return (
    <div>
      <PageHeader
        title="API Keys"
        description="One key maps to one tenant (its own ClickHouse database). Keys are shown once at mint time."
        actions={<Button onClick={() => setMintOpen(true)}>+ Mint key</Button>}
      />

      {keys.isLoading ? (
        <div className="flex justify-center py-16">
          <Spinner />
        </div>
      ) : keys.error ? (
        <ErrorNote>Failed to load keys.</ErrorNote>
      ) : (keys.data?.length ?? 0) === 0 ? (
        <EmptyState title="No API keys yet" hint="Mint one to onboard a service for logging." />
      ) : (
        <KeyList keys={keys.data!} onRevoke={setRevoking} />
      )}

      <MintModal
        open={mintOpen}
        onClose={() => setMintOpen(false)}
        onMinted={(m) => {
          setMintOpen(false)
          setMinted(m)
        }}
        knownTenants={tenants.data?.map((t) => t.tenant) ?? []}
      />

      <MintedModal minted={minted} onClose={() => setMinted(null)} />

      <RevokeModal apiKey={revoking} onClose={() => setRevoking(null)} />
    </div>
  )
}

function KeyList({ keys, onRevoke }: { keys: ApiKey[]; onRevoke: (k: ApiKey) => void }) {
  return (
    <Card className="overflow-hidden">
      {/* Desktop table */}
      <table className="hidden w-full text-sm md:table">
        <thead>
          <tr className="border-b border-zinc-200 text-left text-xs uppercase tracking-wide text-zinc-400">
            <th className="px-4 py-2.5 font-medium">Tenant</th>
            <th className="px-4 py-2.5 font-medium">Label</th>
            <th className="px-4 py-2.5 font-medium">Status</th>
            <th className="px-4 py-2.5 font-medium">Created</th>
            <th className="px-4 py-2.5" />
          </tr>
        </thead>
        <tbody>
          {keys.map((k) => (
            <tr key={k.id} className="border-b border-zinc-100 last:border-0">
              <td className="px-4 py-2.5 font-medium text-zinc-800">{k.tenant}</td>
              <td className="px-4 py-2.5 text-zinc-500">{k.label || '—'}</td>
              <td className="px-4 py-2.5">
                {k.active === 1 ? <Badge tone="green">active</Badge> : <Badge tone="red">revoked</Badge>}
              </td>
              <td className="px-4 py-2.5 text-zinc-500">{fmtTs(k.created_at)}</td>
              <td className="px-4 py-2.5 text-right">
                {k.active === 1 && (
                  <Button variant="ghost" size="sm" className="text-red-600 hover:bg-red-50" onClick={() => onRevoke(k)}>
                    Revoke
                  </Button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* Mobile cards */}
      <ul className="divide-y divide-zinc-100 md:hidden">
        {keys.map((k) => (
          <li key={k.id} className="flex items-center justify-between gap-3 p-4">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="font-medium text-zinc-800">{k.tenant}</span>
                {k.active === 1 ? <Badge tone="green">active</Badge> : <Badge tone="red">revoked</Badge>}
              </div>
              <div className="truncate text-xs text-zinc-500">{k.label || '—'}</div>
              <div className="text-xs text-zinc-400">{fmtTs(k.created_at)}</div>
            </div>
            {k.active === 1 && (
              <Button variant="ghost" size="sm" className="text-red-600" onClick={() => onRevoke(k)}>
                Revoke
              </Button>
            )}
          </li>
        ))}
      </ul>
    </Card>
  )
}

function MintModal({
  open,
  onClose,
  onMinted,
  knownTenants,
}: {
  open: boolean
  onClose: () => void
  onMinted: (m: MintResult) => void
  knownTenants: string[]
}) {
  const mint = useMintKey()
  const toast = useToast()
  const [tenant, setTenant] = useState('')
  const [label, setLabel] = useState('')

  const submit = (e: FormEvent) => {
    e.preventDefault()
    const t = tenant.trim().toLowerCase()
    if (!/^[a-z_][a-z0-9_]*$/.test(t)) {
      toast.error('Tenant must be a slug: letters, digits, underscore (no leading digit).')
      return
    }
    mint.mutate(
      { tenant: t, label: label.trim() },
      {
        onSuccess: (m) => {
          setTenant('')
          setLabel('')
          onMinted(m)
        },
        onError: () => toast.error('Failed to mint key.'),
      },
    )
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Mint API key"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button form="mint-form" type="submit" loading={mint.isPending}>
            Mint key
          </Button>
        </>
      }
    >
      <form id="mint-form" onSubmit={submit} className="space-y-4">
        <Field label="Tenant" htmlFor="tenant" hint="Lowercase slug. Also the ClickHouse DB and (if used) the Valkey key prefix.">
          <Input
            id="tenant"
            list="known-tenants"
            value={tenant}
            onChange={(e) => setTenant(e.target.value)}
            placeholder="app_one"
            autoFocus
          />
          <datalist id="known-tenants">
            {knownTenants.map((t) => (
              <option key={t} value={t} />
            ))}
          </datalist>
        </Field>
        <Field label="Label" htmlFor="label" hint="Free-text note, e.g. environment or owner.">
          <Input id="label" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="prod" />
        </Field>
      </form>
    </Modal>
  )
}

function MintedModal({ minted, onClose }: { minted: MintResult | null; onClose: () => void }) {
  return (
    <Modal
      open={!!minted}
      onClose={onClose}
      title="Key created"
      footer={<Button onClick={onClose}>Done</Button>}
    >
      {minted && (
        <div className="space-y-3">
          <p className="text-sm text-zinc-600">
            Copy this key now for tenant <span className="font-medium text-zinc-900">{minted.tenant}</span>. It is
            hashed server-side and <span className="font-medium">cannot be shown again</span>.
          </p>
          <div className="flex items-center gap-2 rounded-md border border-zinc-200 bg-zinc-50 p-2">
            <code className="min-w-0 flex-1 break-all font-mono text-xs text-zinc-800">{minted.key}</code>
            <CopyButton text={minted.key} />
          </div>
          <div className="rounded-md bg-zinc-50 p-3 text-xs text-zinc-500">
            Use it as{' '}
            <code className="font-mono">Authorization: Bearer {'{key}'}</code> or{' '}
            <code className="font-mono">x-api-key</code> when POSTing to <code className="font-mono">/v1/events</code>.
          </div>
        </div>
      )}
    </Modal>
  )
}

function RevokeModal({ apiKey, onClose }: { apiKey: ApiKey | null; onClose: () => void }) {
  const revoke = useRevokeKey()
  const toast = useToast()
  return (
    <Modal
      open={!!apiKey}
      onClose={onClose}
      title="Revoke key"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="danger"
            loading={revoke.isPending}
            onClick={() =>
              apiKey &&
              revoke.mutate(apiKey.id, {
                onSuccess: () => {
                  toast.success('Key revoked.')
                  onClose()
                },
                onError: () => toast.error('Failed to revoke key.'),
              })
            }
          >
            Revoke
          </Button>
        </>
      }
    >
      {apiKey && (
        <p className="text-sm text-zinc-600">
          Revoke the key for tenant <span className="font-medium text-zinc-900">{apiKey.tenant}</span>
          {apiKey.label ? ` (${apiKey.label})` : ''}? Any service using it will stop being able to send or read events
          within ~60s. This cannot be undone.
        </p>
      )}
    </Modal>
  )
}
