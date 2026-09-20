#!/bin/bash
# Renders everything the cluster needs into k8s/.rendered/. Run from the repo root.
#
#   k8s/*.yaml  are TEMPLATES, committed with __REGISTRY__, __TAG__ and
#               __TAILNET__ placeholders so this repo carries no one
#               installation's addresses.
#   .env        the DEV file, used by docker compose. Never read here.
#   .env.k8s    the CLUSTER file, authoritative for production. Read here.
#
# Output (all gitignored — the only place real values land on disk):
#   k8s/.rendered/00-config.yaml   the Namespace + ConfigMap + Secret
#   k8s/.rendered/*.yaml           each template with placeholders substituted
#
# Then:  kubectl apply -f k8s/.rendered/
#
# TAG selects the image tag to render. It defaults to the current git short SHA;
# pass TAG=<tag> to pin an existing one. Applying the rendered manifests sets
# the image, so pass the tag that is already running if you only mean to update
# other fields.
#
# This script applies NO value overrides. If dev and prod differ, they differ
# because the two files say different things.
#
# NOTE: this repo is PUBLIC. Neither .env nor .env.k8s is ever committed --
# only the .example files. Encrypted copies live in .backup/ (gitignored).
set -euo pipefail

TAG="${TAG:-$(git rev-parse --short HEAD 2>/dev/null || echo latest)}" \
python3 - <<'PY'
import os, sys, glob, re

def load(path):
    env={}
    for line in open(path):
        line=line.strip()
        if not line or line.startswith('#') or '=' not in line: continue
        k,v=line.split('=',1); env[k.strip()]=v.strip().strip('"').strip("'")
    return env

if not os.path.exists('.env.k8s'):
    sys.exit("error: .env.k8s not found (copy .env.k8s.example, or see .backup/)")
cfg_all = load('.env.k8s')

# Key names only; values live in .env.k8s. These render into a k8s Secret.
SECRET={'JWT_SECRET','ADMIN_PASSWORD'}

# Recorded in .env.k8s so they survive, but consumed by the APPS, not by this
# stack: each app holds its own ingest key. Putting them in the cluster would
# hand every clicklog pod a credential it has no use for.
REFERENCE={'KEEPERPROXY_INGEST_KEY','BODHISTREAMS_INGEST_KEY','PIUMAVAULT_INGEST_KEY'}

# Substituted into the manifests below, not read by any pod.
DEPLOY_ONLY={'REGISTRY_HOST','TAILNET'}

cfg={k:v for k,v in cfg_all.items() if k not in SECRET|REFERENCE|DEPLOY_ONLY}
sec={k:v for k,v in cfg_all.items() if k in SECRET}

# A key added to dev and forgotten here is the failure mode this split
# introduces, so name it rather than rendering a quietly incomplete config.
if os.path.exists('.env'):
    dev=load('.env')
    # Compose expresses container resources and published ports through the
    # environment; in the cluster both live in the manifests instead.
    COMPOSE_ONLY={'CLICKHOUSE_MEM_LIMIT','CLICKHOUSE_CPUS','VK_MEM_LIMIT','VK_CPUS',
                  'INGEST_BIND','INGEST_EXT_PORT','FRONTEND_BIND','FRONTEND_EXT_PORT',
                  'MCP_BIND','MCP_EXT_PORT','COMPOSE_PROFILES','COMPOSE_FILE'}
    missing=sorted(set(dev) - set(cfg_all) - COMPOSE_ONLY)
    if missing:
        print(f"  WARNING: in .env but not .env.k8s -> {missing}", file=sys.stderr)
        print( "           add them to .env.k8s, or to COMPOSE_ONLY if dev-only.", file=sys.stderr)

OUT='k8s/.rendered'
os.makedirs(OUT, exist_ok=True)
os.chmod(OUT, 0o700)

b=lambda d: '\n'.join(f'  {k}: "{v}"' for k,v in sorted(d.items()))
open(f'{OUT}/00-config.yaml','w').write(
 "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: clicklog\n---\n"
 "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: clicklog-config\n  namespace: clicklog\ndata:\n"+b(cfg)+"\n---\n"
 "apiVersion: v1\nkind: Secret\nmetadata:\n  name: clicklog-secret\n  namespace: clicklog\ntype: Opaque\nstringData:\n"+b(sec)+"\n")
os.chmod(f'{OUT}/00-config.yaml',0o600)

# --- the manifest templates -------------------------------------------------
subs = {'__REGISTRY__': cfg_all.get('REGISTRY_HOST',''),
        '__TAILNET__':  cfg_all.get('TAILNET',''),
        '__TAG__':      os.environ.get('TAG','')}
for k,v in subs.items():
    if not v: sys.exit(f"error: nothing to substitute for {k} "
                       f"(set {'TAG in the environment' if k=='__TAG__' else k.strip('_')+'_HOST' if k=='__REGISTRY__' else 'TAILNET'} )")

n=0
for src in sorted(glob.glob('k8s/*.yaml')):
    # 00-config.yaml is GENERATED above, not a template. An older layout wrote
    # it into k8s/ instead of k8s/.rendered/, and a leftover copy there would
    # otherwise be treated as a template and overwrite the fresh render with
    # stale secrets.
    if os.path.basename(src) == '00-config.yaml':
        print(f"  note: ignoring {src} — generated, not a template. Safe to delete.", file=sys.stderr)
        continue
    text=open(src).read()
    for k,v in subs.items(): text=text.replace(k,v)
    left=re.findall(r'__[A-Z_]+__', text)
    if left: sys.exit(f"error: {src} still has placeholders after substitution: {sorted(set(left))}")
    open(os.path.join(OUT, os.path.basename(src)),'w').write(text)
    n+=1

print(f"rendered {OUT}/ — {len(cfg)} config keys, {len(sec)} secrets, "
      f"{n} manifests (registry {subs['__REGISTRY__']}, tag {subs['__TAG__']}, 0700/0600, gitignored)")
PY
