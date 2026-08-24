declare const process: { env: Record<string, string | undefined> };
export {};

const coordinator = process.env.SPACL_COORDINATOR_URL ?? "http://127.0.0.1:8080";
const robot = process.env.SPACL_ROBOT_URL ?? "http://127.0.0.1:8081";

async function post(url: string, body: unknown) {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const value = await response.json();
  if (!response.ok) {
    throw new Error(`${value.code}: ${value.message}\nNext: ${value.action}`);
  }
  return value;
}

const context = {
  task_id: "typescript-plan",
  zone: "cell-1",
  state_hash: "sha256:development-world-state",
};

for (const skill of ["move", "wait"]) {
  const token = await post(`${coordinator}/v1/tokens`, {
    robot_id: "robot-1",
    action: { skill, arguments: {} },
    context,
    ttl_seconds: 30,
    constraints: { allowed_skills: [skill], allowed_zones: ["cell-1"] },
    risk: "normal",
    approvals: [],
  });
  console.log(await post(`${robot}/v1/execute`, { token, context }));
}
