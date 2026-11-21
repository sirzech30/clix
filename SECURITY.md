# Security Policy

## Scope & Privilege Model

`clix` interacts with kernel input interfaces (`/dev/uinput` for virtual mouse injection and `/dev/input/event*` for global hotkey detection).

- **Do not run as root**: Use the provided `udev/99-clix-uinput.rules` and `input` group membership so only authorized local users can access `/dev/uinput`.
- **Input group awareness**: Membership in the `input` group allows processes running under that user account to read `/dev/input/event*` devices. Only grant `input` group access on trusted single-user workstations or understand the local threat model.

## Reporting a Vulnerability

If you discover a security vulnerability in `clix` (for example, unsafe FFI handling, privilege escalation, or configuration parsing issues):

1. **Do not** open a public issue with exploit details.
2. Report it privately via GitHub Security Advisories or email the maintainer (`xenonargon30@gmail.com`).
3. Include reproduction steps, affected version/commit, and potential impact.
