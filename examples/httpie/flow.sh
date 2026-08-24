#!/usr/bin/env bash
set -euo pipefail

coordinator_url="${SPACL_COORDINATOR_URL:-http://127.0.0.1:8080}"
robot_url="${SPACL_ROBOT_URL:-http://127.0.0.1:8081}"
workspace="${SPACL_DATA_DIR:-.spacl-dev}"

http GET "$coordinator_url/v1/status"
http POST "$coordinator_url/v1/tokens" < "$workspace/config/sample-token-request.json" > "$workspace/tokens/httpie-token.json"
http POST "$robot_url/v1/emergency-stop" active:=false

