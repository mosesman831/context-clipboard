# Security policy

## Threat model (summary)

Context Clipboard is a local clipboard history store. Full detail lives in [SPEC.md](./SPEC.md) §5.

**In scope**

- Other local users or malware reading `history.db` from disk. Mitigated by AEAD encryption and a key held in the OS keychain / secret service.
- Accidental long-term storage of secrets. Mitigated by app denylist, concealed pasteboard types, secret heuristics, retention, pause, and delete.
- UI XSS if clipboard HTML were rendered in the webview. Mitigated by never rendering raw HTML, CSP, and a minimal Tauri IPC allowlist.

**Out of scope (v1)**

- An attacker who already has the unlocked user session and keychain access (they can read the live clipboard anyway).
- Evil-maid with keychain unlock.
- Compelled cloud disclosure (there is no cloud).

IPC uses a Unix domain socket with restrictive permissions and peer UID checks. There is no localhost HTTP server in v1.

## Reporting a vulnerability

Please use [GitHub private vulnerability advisories](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability) on this repository.

You may also email `security@latticeag.dev` if the advisory UI is unavailable.

Do not open a public issue for security reports.

## What to include

- Affected version or commit, OS, and steps to reproduce
- Impact (read history, bypass exclusion, crash daemon, etc.)

**Never paste real clipboard contents into a report if you can avoid it.** Use redacted or synthetic samples. Screenshots of history UIs are especially risky; prefer descriptions of the bug class.

## Out of scope for this policy

- Feature requests and non-security bugs (use ordinary issues)
- Issues that require physical access plus an unlocked session with keychain access, unless they reveal a design flaw beyond that baseline
