#!/usr/bin/env bash
set -euo pipefail

coordinator_url="${SPACL_COORDINATOR_URL:-http://127.0.0.1:8080}"
robot_url="${SPACL_ROBOT_URL:-http://127.0.0.1:8081}"
workspace="${SPACL_DATA_DIR:-.spacl-dev}"

curl --fail --silent --show-error "$coordinator_url/v1/status" | jq .

token_file="$workspace/tokens/curl-token.json"
mkdir -p "$(dirname "$token_file")"

curl --fail --silent --show-error \
  -H 'content-type: application/json' \
  --data @"$workspace/config/sample-token-request.json" \
  "$coordinator_url/v1/tokens" | tee "$token_file" | jq .

jq -n \
  --slurpfile token "$token_file" \
  '{token:$token[0],context:{task_id:"sample-task",zone:"cell-1",state_hash:"sha256:development-world-state"}}' |
  curl --fail --silent --show-error \
    -H 'content-type: application/json' \
    --data @- \
    "$robot_url/v1/execute" | jq .

