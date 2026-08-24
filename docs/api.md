# API Reference

SPACL v0.1.0 uses JSON over HTTP. The default examples use the coordination node at `127.0.0.1:8080` and one robot runtime at `127.0.0.1:8081`.

## Health

```text
GET /healthz
```

## Enroll a Robot

```text
POST /v1/robots
Content-Type: application/json
```

```json
{
  "robot_id": "robot-1",
  "display_name": "Mobile Base 1",
  "identity": {
    "key_id": "sha256:<hex>",
    "subject": "robot-1",
    "algorithm": "ML-DSA-65+Ed25519",
    "ml_dsa_65_public_key": "<base64url>",
    "ed25519_public_key": "<base64url>"
  }
}
```

Generate the `identity` object with `spacl keygen`. Do not send the private identity file.

## List the Fleet

```text
GET /v1/fleet
```

## Issue a Token

```text
POST /v1/tokens
Content-Type: application/json
```

```json
{
  "robot_id": "robot-1",
  "action": {
    "skill": "move",
    "arguments": {"distance_m": 2.0},
    "requested_speed_mps": 0.5
  },
  "context": {
    "task_id": "order-1042",
    "zone": "aisle-3",
    "state_hash": "sha256:world-snapshot-reference"
  },
  "ttl_seconds": 30,
  "constraints": {
    "allowed_skills": ["move"],
    "allowed_zones": ["aisle-3"],
    "max_speed_mps": 0.75
  },
  "risk": "normal",
  "approvals": []
}
```

Use two distinct approval objects when `risk` is `high`.

## Execute a Token

Send the complete token response and the current context to the robot runtime.

```text
POST /v1/execute
Content-Type: application/json
```

```json
{
  "token": {"schema": "spacl.action-token.v1", "claims": {}, "signature": {}},
  "context": {
    "task_id": "order-1042",
    "zone": "aisle-3",
    "state_hash": "sha256:world-snapshot-reference"
  }
}
```

The runtime returns an execution receipt or a rejection reason.

## Set Emergency Stop

```text
POST /v1/emergency-stop
Content-Type: application/json
```

```json
{"active": true}
```

The emergency-stop state persists across process restarts. The current API can also clear it. A production system must require a local, authenticated reset procedure.

## Revoke a Robot

```text
POST /v1/robots/robot-1/revoke
```

Revocation prevents new token issuance. It does not delete an identity or recall a token that the coordination node already issued.

