# Troubleshooting

## Start With Status

Run:

```bash
spacl status --robot-url http://127.0.0.1:8081
```

This command reads local coordinator state. It also checks the running coordinator and robot runtime.

## Identity Is Not Enrolled

Error code: `IDENTITY_NOT_ENROLLED`

The coordinator does not have the target robot public identity. Run `spacl init` for the sample robot. For another robot, generate its identity on the robot host and send only the public identity to `POST /v1/robots`.

## Sequence Gap

Error code: `SEQUENCE_GAP`

The runtime has not consumed an earlier token. Check `next_sequence` with `spacl status`. Execute or reconcile the missing sequence before you send a later token.

Do not edit a sequence number. The coordinator signature covers it.

## Token Expired

Error code: `TOKEN_EXPIRED`

Issue a new token. Use the current task, zone, and world-state hash. The default token lifetime is 30 seconds.

## Context Mismatch

Error code: `CONTEXT_MISMATCH`

The execution context does not match the signed context hash. Refresh the world state. Then issue a new token for that state.

## Two-Person Approval Is Missing

Error code: `TWO_PERSON_APPROVAL_REQUIRED`

Supply two distinct operator IDs:

```bash
spacl token issue \
  --robot-id robot-1 \
  --skill pick \
  --task-id task-1 \
  --zone cell-1 \
  --high-risk \
  --approver alice \
  --approver bob
```

Version 0.2.0 does not authenticate these operator IDs. They are workflow assertions only.

## Emergency Stop Is Active

Error code: `EMERGENCY_STOP_ACTIVE`

The stop state persists across restarts. Clear it only through an authorized local procedure. Do not treat the development HTTP reset as a production safety control.

## Private Identity Permissions

SPACL rejects a private identity file that grants group or world access on Unix. Repair the mode:

```bash
chmod 600 /path/to/robot.identity.json
```

## Service Is Offline

Check the bind address. Then check whether another process uses the port. The default coordinator port is `8080`. The default robot port is `8081`.

Use `--json-logs` when another system collects logs. Omit it for readable terminal logs.

