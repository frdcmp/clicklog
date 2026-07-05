# clicklog dashboard

React + Vite + TypeScript admin UI for the ingest gateway: API-key CRUD, advanced
cross-tenant log search, stats, and in-app docs. Auth is a single seeded admin
(JWT via `POST /v1/admin/login`).

- **Data layer:** `src/api/` (raw endpoint fns + fetch client) and `src/query/`
  (TanStack Query hooks). All calls go to `/v1/admin/*`.
- **Styling:** Tailwind v4, single light theme, hand-rolled components in
  `src/components/ui`. Responsive (sidebar → hamburger, tables → cards under `md`).

## Develop

```bash
npm install
cp .env.example .env        # set VITE_API_TARGET if the ingest-api isn't on :46005
npm run dev                 # http://localhost:5173
```

The dev server proxies `/v1` + `/health` to `VITE_API_TARGET` (default
`http://localhost:46005`), so the browser stays same-origin — no CORS. Point it at
a local `docker compose up` or an SSH tunnel to the prod host
(`ssh -L 46005:<overlay-ip>:46005 <infra-host>`).

## Production (nginx)

Served by the optional `frontend` service in the root `docker-compose.yml`, behind
the `dashboard` profile. A multi-stage image builds the SPA and nginx serves it +
reverse-proxies `/v1` and `/health` to `ingest-api:8080` on the internal network
(same-origin, so the API base `/v1/admin` needs no build-time URL).

```bash
# from the repo root
docker compose --profile dashboard build frontend
docker compose --profile dashboard up -d            # brings up the whole stack incl. UI
# → dashboard on http://<FRONTEND_BIND>:<FRONTEND_EXT_PORT>  (default 127.0.0.1:46006)
```

Env knobs (root `.env`): `FRONTEND_BIND`, `FRONTEND_EXT_PORT`. In production bind to
the host's private-overlay IP, never a public NIC — the
only auth is the admin JWT. Ensure `JWT_SECRET` and a strong `ADMIN_PASSWORD` are
set for the ingest-api before deploying.

`npm run build` (tsc + vite) and `npm run typecheck` run locally without Docker.
