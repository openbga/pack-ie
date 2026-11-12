# Positive ACK Interface Engine (PACK-IE)

[Operator guides](docs/operations/README.md) ·
[Developer guide](docs/development/README.md) ·
[Contributing](CONTRIBUTING.md) ·
[Security](SECURITY.md)

Positive ACK is a free, open-source (AGPL-3.0-only) interface engine for clinical
laboratories: the middleware between analyzers, LIS/LIMS, EHRs and other applications.
It receives and sends instrument traffic (ASTM, vendor protocols), exchanges HL7 v2 over
MLLP, imports and exports files, bridges serial and TCP ports, and keeps a durable record
of every message it handles. It runs on one computer the laboratory owns, inside the lab
network, with no cloud service required. It is an [OpenBGA](https://github.com/openbga)
project.

**Status: development on synthetic data.** Version 0.2.0 is built and tested, but no
release has been published, installers are unsigned and the software is not clinically
validated. [STATUS.md](STATUS.md) records what is verified on each platform and what remains.

## What it does

- **Instrument interfaces:** ASTM E1381/E1394 (CLSI LIS01-A2/LIS02-A2) and the AU400/AU640
  fixed-width protocol, over serial ports and TCP, as receiver and as sender.
- **HL7 v2:** MLLP client and listener with original and enhanced acknowledgements tracked
  per message.
- **Files:** delimited files read and written from templates, in watched folders and over SFTP.
- **Bridge channels:** a live byte stream between two endpoints (serial, virtual serial, TCP)
  with bounded buffers, reconnection and a traffic transcript.
- **Message channels:** a source, declarative filters and mappings (code tables, exact-decimal
  calculations, presets for common instrument layouts) and one or more destinations. A message
  is committed to the store before it is acknowledged; deliveries retry, duplicates are
  detected and a message can be resent with a reason.
- **Records a laboratory needs:** operator attribution from the OS account, a hash-chained
  audit log, configuration change control with sign-off, backup and restore, transmission
  reports and exports, e-mail and webhook alerts.
- **One computer:** Windows, macOS and Linux. No server to run, no subscription.

## Shape

- **`packie`** — the engine, a Rust binary that runs as a console process or as an OS service
  (Windows service, launchd, systemd) and also serves as the command-line client.
- **PACK-IE Console** — a desktop application (Tauri 2, React) that attaches to the running
  engine: dashboard, channels, endpoints, messages, logs, configuration.

## Read

| Question | Document |
| --- | --- |
| How do I install, configure and operate it? | [Operator guides](docs/operations/README.md) |
| What does it do, for whom, and what is "version 1"? | [docs/requirements.md](docs/requirements.md) |
| How is it built: process model, channel model, crates? | [docs/architecture.md](docs/architecture.md) |
| Why these choices? | [docs/decisions.md](docs/decisions.md) |
| How is it tested? | [docs/testing.md](docs/testing.md) |
| How do I develop and extend it? | [Developer guide](docs/development/README.md) |
| How do I contribute? | [Contributing](CONTRIBUTING.md) |
| Behavior specifications per area | [docs/spec/](docs/spec/) |
| What is verified, and what remains? | [STATUS.md](STATUS.md) |
| Working on this with an agent? | [AGENTS.md](AGENTS.md) |

## Getting started

There is no published installer yet. Build the engine and the console from source as
[Contributing](CONTRIBUTING.md#build) describes, then follow [Install](docs/operations/install.md)
and configure a [first channel](docs/operations/first-channel.md). Use synthetic data only.

## Support

Community support is through [GitHub issues](https://github.com/openbga/pack-ie/issues), on a
best-effort basis. Report vulnerabilities privately as the [security policy](SECURITY.md)
describes. Are you looking for support? Contact [bgasoft.com](https://bgasoft.com).

## License and development disclaimer

Copyright (C) 2026 BGASoft Inc. and OpenBGA contributors. The project's own code,
documentation and synthetic fixtures are licensed under the
[GNU Affero General Public License, version 3 only (AGPL-3.0-only)](LICENSE), unless otherwise
identified. Third-party material retains its own terms. Use, modification and redistribution,
including commercial sale, are permitted. Distributors, and operators who offer a modified
version to users over a network, must preserve the AGPL rights and provide the corresponding
source as the license requires.

The ASTM and AU codecs and the console's component kit derive from
[Steady State LIMS](https://github.com/openbga/steady-state-lims), copyright 2026 Steady State
LIMS contributors, published under GPL-3.0-only. See [NOTICE](NOTICE).

OpenBGA and the OpenBGA product names and logos are trademarks of BGASoft Inc. The license
does not grant permission to use them (AGPL-3.0 section 7(e)), except as needed to describe
the origin of the software.

Positive ACK is developed with substantial AI assistance, with human direction, review and
testing. That effort does not guarantee correctness, security, regulatory compliance or fitness
for clinical use. The software is provided as is, with the warranty disclaimers and liability
limitations in AGPLv3 sections 15–17; it is built and tested on synthetic data and is not
clinically validated.
