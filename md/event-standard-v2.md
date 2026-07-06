# Event standard v2 — required fields (DECIDED 2026-07-06)

> Status: decisions made, pending implementation. When implemented, update
> `ingest-api/src/schema.rs`, the READMEs, and the dashboard docs, then delete
> the "pending" wording here.

## Decisions

1. **`server` is hard-required** — yes, even though piumavault must be fixed first.
2. **`severity=error` requires `message`** — `error_code` stays optional.
3. **One global standard** — no per-tenant contracts; the whole point is one shape.
4. **No warn mode.** Violations surface the way they already do: `400` with the
   per-event error list. (Warn-mode surfacing is moot given #5.)
5. **Enforce from implementation, no fallback.** Fix the two known emitters,
   then ship the validation. Anything still off-spec gets rejected — that is
   the contract.

## The standard (v2)

### Hard-required (missing → 400, whole batch rejected)

| field | why |
|---|---|
| `category` | coarse class (existing requirement) |
| `event_type` | specific type (existing requirement) |
| `source` | which process emitted — table stakes for triage |
| `server` | which host — dashboards are meaningless without it |

### Conditionally required (400 when the condition holds)

| condition | then required |
|---|---|
| `category = "http"` | `route`, `http_status`, `duration_ms` |
| `severity = "error"` | `message` |
| `category = "llm"` | `model` |

### Soft-required (docs + nudges, never rejected)

- `route` on **any** request/operation-scoped event (`worker:email_send` for non-http).
- `request_id` on anything request-scoped.
- `user_id` / `user_email` whenever a user context exists (`CurrentUser` middleware pattern).
- `duration_ms` on anything timed (jobs, LLM calls), not just http.

## Day-one impact (measured, last 7 days of live traffic)

| tenant | events/7d | Tier-1 violations | Tier-2 violations |
|---|---:|---:|---:|
| keeperproxy | 5051 | **33** (missing `source`) | 0 |
| bodhistreams | 1749 | 0 | 0 |
| piumavault | 3782 | **3782** (missing `server`) | 0 |
| acp | 124 | 0 | 0 |
| work_andovar | 120 | 0 | 0 |
| assessment_platform | 230 | 0 | 0 |

Tier 2 (conditional) breaks **nothing** — every http event already carries
`route`+`http_status`, every error a `message`, every llm event a `model`.

## Implementation checklist

1. [ ] **piumavault**: set `server` on every event (pass `SERVER_NAME` to the
       backend; default it in the event builder). 100% of its traffic violates
       Tier 1 today.
2. [ ] **keeperproxy**: set `source` on the `email` / `reservation` domain-event
       emitters (~33 events/week; http traffic is clean).
3. [ ] **gateway** (`ingest-api/src/schema.rs`): add the Tier-1 + Tier-2
       validations; update the per-event error strings to name the rule
       (e.g. `"category 'http' requires 'route'"`).
4. [ ] **docs**: README event standard, ingest-api README §3, dashboard
       DocsPage field table + callout, and the LLM integration guide
       (`frontend/public/llms.txt` — the required/conditional field rules).
5. [ ] Deploy gateway to decametro + andolinux **after** 1–2 land.

## Historical coverage (baseline, all-time — for reference)

| tenant | events | source | server | route | message | request_id | user_* |
|---|---:|---:|---:|---:|---:|---:|---:|
| keeperproxy | 6711 | 99 | 100 | 8 | 1 | 0 | 11 |
| bodhistreams | 1763 | 100 | 100 | 14 | 6 | 0 | 94 |
| piumavault | 5058 | 100 | **0** | 97 | 8 | 0 | 2 |
| acp | 124 | 100 | 100 | 44 | 1 | 98 | 81 |
| work_andovar | 118 | 100 | 100 | 77 | 1 | 86 | 14 |
| assessment_platform | 229 | 100 | 100 | 96 | 0 | 96 | 0¹ |

¹ fixed 2026-07-05 (`CurrentUser` middleware) — new events now attributed.
