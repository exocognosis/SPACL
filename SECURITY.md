# Security Policy

## Supported Versions

SPACL is in early development. Only the latest release receives security fixes.

## Report a Vulnerability

Use GitHub private vulnerability reporting for this repository. Include the affected version, component, impact, reproduction steps, and proposed mitigation if known.

Do not open a public issue for an unpatched vulnerability. Do not test against systems that you do not own or have permission to assess.

The maintainer will acknowledge a complete report within five business days. This target is not a service-level agreement.

## Scope

Reports about token forgery, signature verification, replay controls, sequence state, revocation, emergency-stop bypass, policy bypass, private key exposure, audit-chain integrity, or API authorization are in scope.

SPACL v0.2.0 is not approved for production robot use. The documented absence of transport security, operator authentication, hardware-backed keys, replicated state, and safety certification is not a new vulnerability.

Read the [repository threat model](docs/threat-model.md) for assets, trust boundaries, attacker capabilities, security invariants, and severity guidance.
