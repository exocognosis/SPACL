# Deployment Guide

## Development Deployment

Run one coordination process and one robot process per simulated robot. Bind all interfaces to `127.0.0.1` unless you provide an authenticated tunnel.

Keep each data directory on durable local storage. Back up public identities and audit files. Do not copy private identities between hosts after enrollment.

## Container Image

Build the image:

```bash
docker build -t spacl:0.2.0 .
```

Run a coordination node:

```bash
docker run --rm \
  --read-only \
  --tmpfs /tmp \
  --user 65532:65532 \
  -p 127.0.0.1:8080:8080 \
  -v "$PWD/data/coordinator:/var/lib/spacl" \
  spacl:0.2.0 --data-dir /var/lib/spacl coordinator --bind 0.0.0.0:8080
```

## File Permissions

- Private identity: owner read and write only (`0600`)
- Data directory: owner read, write, and execute only (`0700`)
- Public identity: read-only after enrollment
- Audit file: append access for the service account; read access for the audit exporter

## Production Gate

Do not deploy v0.2.0 on a physical robot. Before a production pilot, complete all required work in [Security Model](security.md). Add service authentication, encrypted transport, operator authentication, backup recovery tests, dependency scanning, rate limits, and an external security review.
