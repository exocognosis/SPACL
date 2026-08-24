#!/usr/bin/env python3
"""Issue and execute a short SPACL plan with the Python standard library."""

import json
import os
import urllib.error
import urllib.request

COORDINATOR = os.getenv("SPACL_COORDINATOR_URL", "http://127.0.0.1:8080")
ROBOT = os.getenv("SPACL_ROBOT_URL", "http://127.0.0.1:8081")


def post(url: str, body: dict) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        detail = json.load(error)
        raise SystemExit(f"{detail['code']}: {detail['message']}\nNext: {detail['action']}")


def main() -> None:
    context = {
        "task_id": "python-plan",
        "zone": "cell-1",
        "state_hash": "sha256:development-world-state",
    }
    for skill in ("move", "wait"):
        token = post(
            f"{COORDINATOR}/v1/tokens",
            {
                "robot_id": "robot-1",
                "action": {"skill": skill, "arguments": {}},
                "context": context,
                "ttl_seconds": 30,
                "constraints": {
                    "allowed_skills": [skill],
                    "allowed_zones": ["cell-1"],
                },
                "risk": "normal",
                "approvals": [],
            },
        )
        receipt = post(f"{ROBOT}/v1/execute", {"token": token, "context": context})
        print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()

