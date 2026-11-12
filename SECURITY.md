# Security policy

## Supported versions

No release has been published yet. Until one is, fixes land on `main`; after that, the
latest release line receives security fixes.

## Report privately

Use GitHub's private vulnerability reporting:
<https://github.com/openbga/pack-ie/security/advisories/new> ("Report a vulnerability" under
the repository's Security tab). It reaches the maintainer privately; do not open a
public issue for a suspected vulnerability. If that form is unavailable, contact the maintainer
through the GitHub profile linked from the repository and ask for a private channel.

Do not open a public issue with exploit details. Include the affected version, platform,
impact and a minimal synthetic reproduction. Do not send patient data, laboratory exports
or credentials. If sensitive material seems necessary, describe that need first.

Reports are handled on a best-effort basis. The maintainer may ask for a reproduction,
assess the impact and coordinate a fix and disclosure with the reporter. There is no
guaranteed response or resolution time.

## Scope and threat model

The engine, desktop console and packaging are in scope, including input handling, local IPC
restrictions, secret handling and unintended disclosure of stored messages.

PACK-IE runs on a laboratory-controlled computer inside a private lab network, with listeners
bound to configured addresses and management confined to local IPC. Local management users
are trusted to inspect messages and change configuration; the operating system controls who
can reach that interface and access its files.

Under this model, the absence of a console login, TLS on raw TCP/MLLP, database encryption
or remote management is not itself a vulnerability. An authorized local management user
reading message bodies, or a host administrator modifying the process or its files, is also
within that trust model. These limits do not excuse malformed-input crashes, allowlist or
IPC access bypasses, unintended data disclosure, or unsafe installer permissions.

Body-free diagnostic logs still contain peer IP addresses and engine-generated IDs; they are
not de-identified under HIPAA. Use public issues for non-sensitive design questions. See the
[architecture](docs/architecture.md) and [security decisions](docs/decisions.md) for the current
boundaries.
