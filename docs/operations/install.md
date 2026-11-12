# Install and control PACK-IE

PACK-IE runs on one laboratory computer. The engine runs as an OS service; PACK-IE Console connects
locally and can be closed without stopping traffic. Use an administrator terminal on Windows
or `sudo` on macOS and Linux for system service changes.

Before clinical use, review the [laboratory compliance guide](compliance.md) for access,
encryption, backup, validation and go-live checks.

## Packages and installed files

Packages are built by the scripts in [packaging](../../packaging/README.md). They are unsigned;
the macOS package and application are not notarized. A console bundle is included on macOS
and Windows only when it was available to the packaging script.

| Platform | Package and installation | Verification status |
| --- | --- | --- |
| macOS | `packie-<version>-macos-<arch>.pkg`; `sudo installer -pkg /path/to/package.pkg -target /` | Installed and running as a system daemon on macOS arm64. Restart-after-kill, configuration-error stop, reboot and uninstall (VERIFY M5, M5a, M6) not yet run. |
| Windows | `packie-<version>-windows-x64-setup.exe`; run as administrator, or use `/S` for silent installation | The engine builds and passes its tests on Windows in CI. The installer and service checks in [VERIFY.md](../../packaging/VERIFY.md) have not been run on a Windows machine; run them before relying on it. |
| Debian-family Linux with systemd | `packie_<version>_<arch>.deb`; `sudo apt install ./packie_<version>_<arch>.deb` | Install, service, reboot, upgrade and removal checked on Ubuntu 24.04 arm64; x86_64 and physical serial hardware not yet verified. Linux console is not packaged. |

The installers create the bootstrap only if it is missing, register the service and start it.
The Debian installer skips service registration when systemd is not running; run the Linux
service install command below on the target computer.

| Item | macOS | Windows (default installation directory) | Linux |
| --- | --- | --- | --- |
| Engine | `/usr/local/bin/packie` | `C:\Program Files\Positive ACK\packie.exe` | `/usr/bin/packie` |
| Bootstrap | `/Library/Application Support/Positive ACK/bootstrap.json` | `C:\ProgramData\Positive ACK\bootstrap.json` | `/etc/packie/bootstrap.json` |
| State | `/Library/Application Support/Positive ACK/state` | `C:\ProgramData\Positive ACK\state` | `/var/lib/packie` |
| Logs | `/Library/Logs/Positive ACK` | `C:\ProgramData\Positive ACK\logs` | `/var/log/packie` |
| Service | `/Library/LaunchDaemons/org.openbga.positiveack.plist` | Service `packie`, display name Positive ACK | `/etc/systemd/system/packie.service` |
| Service account | `root` | `LocalSystem` | System user and group `packie`; supplementary `dialout` when present |
| Console | `/Applications/PACK-IE Console.app` when included | Separate PACK-IE Console installation when included | Not included in the `.deb` |

Inside the state directory, runtime configuration is `runtime.json`, saved versions are in
`config/`, and message storage is in `store/` (`messages.sqlite3`, its SQLite sidecar files,
and `bodies/`). Audit files are in `audit/`, backup status in `backup/status.json`, and
previous state from a restore in `pre-restore-<UTC time>/`. Treat the whole state directory
as laboratory data. Do not edit the database
or remove its sidecars to troubleshoot a running engine.

Defaults are node id `lab-node`, display name Laboratory node and environment `prod`. Secrets
are kept in the OS key store on macOS (Keychain) and Windows (Credential Manager); the Linux
package uses the secrets file `/var/lib/packie/secrets.json` (owner-only, readable by the `packie`
service account alone), because a server has no Secret Service. A Linux bootstrap that sets
`"provider": "keystore"` without a reachable Secret Service stops the engine at start with a
message naming the file provider. Edit the installed bootstrap and restart the service to change startup
settings. Configure endpoints and channels through the console or CLI; runtime changes are
validated and versioned. See [configuration](../spec/configuration.md).

## Service commands

`service install` both registers and starts the service. `service uninstall` stops it and
removes its registration, retaining the binary, bootstrap, state and logs. `service stop`
stops the current run; the installed service remains configured to start with the computer.
Allow shutdown to finish: the service manager permits the configured drain time plus 30 seconds.

The service manager restarts the engine after a failure, but not after a configuration error
(exit code 2: an invalid bootstrap or an unusable secret store), which would only repeat. On
Linux the unit stays `failed` (`RestartPreventExitStatus=2`) and `journalctl -u packie` shows the
reason; on macOS the daemon stops and the reason is in `/var/log/packie/launchd.stderr.log`; on
Windows the service stops with an Application event-log error (source `packie`, ID 3) naming the
reason. Fix the bootstrap, then start the service again.

On macOS, use `sudo` consistently for the packaged system daemon. Without elevation, the CLI
selects a user agent instead; `service status` without elevation reports the system daemon
when no user agent is installed, as far as it can be read without administrator rights (its
state, PID and start time, from launchd and the group-readable `packie.instance.json`), and says
so. These commands target the system daemon:

| Action | Command |
| --- | --- |
| Register and start | `sudo /usr/local/bin/packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service install` |
| Start | `sudo /usr/local/bin/packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service start` |
| Show status | `sudo /usr/local/bin/packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service status` |
| Stop | `sudo /usr/local/bin/packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service stop` |
| Remove registration | `sudo /usr/local/bin/packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service uninstall` |

For a macOS evaluation under your own account, use a separate bootstrap with state and logs
in directories you own. Run `packie --bootstrap /absolute/path/bootstrap.json service install --user`
without `sudo`; use the same `--user` flag with `start`, `status`, `stop` and `uninstall`.
Its definition is `~/Library/LaunchAgents/org.openbga.positiveack.plist` and it starts at login.
Do not point a user agent at the packaged service's state directory.

On Windows, use PowerShell as administrator. These paths assume the default install location:

| Action | Command |
| --- | --- |
| Register and start | `& 'C:\Program Files\Positive ACK\packie.exe' --bootstrap 'C:\ProgramData\Positive ACK\bootstrap.json' service install` |
| Start | `& 'C:\Program Files\Positive ACK\packie.exe' --bootstrap 'C:\ProgramData\Positive ACK\bootstrap.json' service start` |
| Show status | `& 'C:\Program Files\Positive ACK\packie.exe' --bootstrap 'C:\ProgramData\Positive ACK\bootstrap.json' service status` |
| Stop | `& 'C:\Program Files\Positive ACK\packie.exe' --bootstrap 'C:\ProgramData\Positive ACK\bootstrap.json' service stop` |
| Remove registration | `& 'C:\Program Files\Positive ACK\packie.exe' --bootstrap 'C:\ProgramData\Positive ACK\bootstrap.json' service uninstall` |

On Linux:

| Action | Command |
| --- | --- |
| Register and start | `sudo /usr/bin/packie --bootstrap /etc/packie/bootstrap.json service install` |
| Start | `sudo /usr/bin/packie --bootstrap /etc/packie/bootstrap.json service start` |
| Show status | `sudo /usr/bin/packie --bootstrap /etc/packie/bootstrap.json service status` |
| Stop | `sudo /usr/bin/packie --bootstrap /etc/packie/bootstrap.json service stop` |
| Remove registration | `sudo /usr/bin/packie --bootstrap /etc/packie/bootstrap.json service uninstall` |

`service status` reports registration, process, bootstrap and management endpoint. To check
the engine's actual channel, endpoint, store and alert state, run `packie --bootstrap <installed-bootstrap>
status` using the executable and bootstrap paths above. This command uses local IPC and does
not need administrator privileges when the operator has IPC access. The CLI's default
`config/bootstrap.json` is relative to its working directory; installed paths must be named
explicitly or supplied through `PACKIE_BOOTSTRAP`.

For the short `packie` commands used elsewhere in this guide, prepare the current shell once.
On macOS (zsh or bash):

```sh
export PACKIE_BOOTSTRAP="/Library/Application Support/Positive ACK/bootstrap.json"
export PATH="/usr/local/bin:$PATH"
packie status
```

On Linux (bash or a compatible shell):

```sh
export PACKIE_BOOTSTRAP=/etc/packie/bootstrap.json
packie status
```

On Windows (PowerShell):

```powershell
$env:PACKIE_BOOTSTRAP = 'C:\ProgramData\Positive ACK\bootstrap.json'
Set-Alias -Name packie -Value 'C:\Program Files\Positive ACK\packie.exe'
packie status
```

These settings apply to that shell and programs launched from it. The PowerShell alias is
for terminal commands; the console's service-start button uses `PACKIE_BIN` or the executable
search described below. If Windows was installed elsewhere, substitute that engine path.

## IPC access and opening the console

Management uses a Unix socket or Windows named pipe, never a TCP management port.

| Platform | Default IPC endpoint | Who can attach |
| --- | --- | --- |
| macOS | `/Library/Application Support/Positive ACK/state/packie.sock` | Owner and members of `packie` |
| Windows | `\\.\pipe\packie-lab-node` | Members of local `PACK-IE Operators`, Administrators and LocalSystem; remote clients are rejected |
| Linux | `/var/lib/packie/packie.sock` | Owner and members of `packie` |

On Unix the bootstrap's `ipc.group` defaults to `packie`. An available group gives the socket
mode `0660`; a missing group or `ipc.group: null` leaves it owner-only (`0600`). Service
installation creates `packie`; a direct `sudo ... service install` adds `$SUDO_USER`. The macOS
package also identifies the signed-in user. Sign out and in again after adding membership.
For another operator, or a Linux package installation that did not add the operator, use:

| Platform | Add the named operator (replace `operator`) |
| --- | --- |
| macOS | `sudo dseditgroup -o edit -a operator -t user packie` |
| Linux | `sudo usermod --append --groups packie operator` |
| Windows (administrator terminal) | `net localgroup "PACK-IE Operators" operator /add` |

On Windows, `ipc.group: "packie"` selects the local `PACK-IE Operators` group. Another value names
a local or domain group. Service installation creates `PACK-IE Operators` and adds the elevated
installing account; add each intended operator separately, especially when a different
administrator installed it. Sign out and in again after membership changes. A missing group
or `ipc.group: null` leaves SYSTEM and Administrators only; a missing group logs
`ipc.group_unavailable`. An installation without the group needs it and its memberships set up before
ordinary operators can attach. Restart after changing bootstrap settings or creating a missing
group so the pipe/socket permissions are rebuilt.

Use an individual OS account per person. The engine attributes management actions and
content views to the OS identity of the connecting process; a shared account cannot identify
the person. IPC permission does not grant direct access to private store files.

The console resolves a saved endpoint override first, then `PACKIE_IPC`, then a bootstrap
file: `PACKIE_BOOTSTRAP` if set, else the bootstrap the installer wrote for this operating
system (the default paths in the table above), else `config/bootstrap.json` in its working
directory or nearest parent. A console started normally on a computer with the packaged engine
therefore attaches without any setting. Set `PACKIE_BOOTSTRAP` only for a different engine,
such as a macOS `--user` agent or a development checkout; for example, from Terminal:

```sh
PACKIE_BOOTSTRAP=/absolute/path/bootstrap.json "/Applications/PACK-IE Console.app/Contents/MacOS/packie-console"
```

On Windows, set the variable in your user environment and open a new login session, or set it in
PowerShell and launch the console executable from that shell. Linux users must build or provide
the console separately (it finds `/etc/packie/bootstrap.json` itself), or operate the engine
through the CLI.

If a saved override points elsewhere, it takes precedence over environment variables; correct
the `ipc_endpoint` setting in the console's per-user `console.json` while the console is
closed. The disconnected screen reports the endpoint it tried.

The console offers **Start the service**, but the current button does not request elevation.
For the packaged system service, use the administrator commands above if it fails. On macOS,
the unelevated button targets the user agent. The button also needs the engine available as
`PACKIE_BIN`, next to the console executable, or on `PATH`.

## Serial port permissions

Discover the actual port with `packie endpoints list-serial`, then test it with
`packie endpoints test <endpoint-id>` under the running service. Discovery opens no port;
the test opens/closes a free port without sending application data. An occupied engine port
is reported in use. Schedule intrusive port access around the lab's active interfaces.

| OS | Service-account access and port selection |
| --- | --- |
| Linux | The installed systemd unit runs as `packie` and adds supplementary group `dialout` when it exists. Check the actual `/dev/ttyUSB*` or `/dev/ttyACM*` device owner/group and the service's groups. If device access uses another group, have IT grant that group to the service account/unit and restart the service. Use a persistent device rule/name where reconnects change numbering; keep device permissions restricted. |
| macOS | Packaged system service runs as root; a `--user` agent runs as the login account. Install the adapter's driver if needed, choose the OS-reported `/dev/tty.*` port, and check that the account running the engine can open it. Close competing serial applications. |
| Windows | Use the COM name reported by discovery/Device Manager, such as `COM3` or `COM10`. Install the adapter driver and confirm that LocalSystem (the packaged service account) can open it; COM ports are exclusive, so close vendor terminals or other programs holding it. Recheck the COM assignment after reconnecting or replacing an adapter. |

IPC group membership gives management access; it does not by itself grant serial device
access. See [serial troubleshooting](troubleshooting.md#connections-and-bridge-traffic).

## Notification relay setup

Notifications use an SMTP relay and/or a webhook configured in the installed bootstrap;
see the [notification fields](configuration-reference.md#bootstrap). Allow the engine's
service account and computer to reach only the intended targets. Put SMTP passwords and
webhook bearer tokens in the configured secrets provider with `packie secrets set <name>`;
bootstrap fields contain secret references. Keep node/channel/endpoint names free of PHI.
Restart after target-setting changes, then run `packie notify test` and confirm receipt at each
target. The command reports each result and is audited.

Remote SMTP uses `starttls` (usually port 587) or `tls` (usually 465); webhook URLs use
HTTPS. By default certificates are verified against bundled Mozilla roots. For a private
lab CA, set `notifications.ca_file` to the path of its PEM CA certificate bundle, readable
by the engine's service account. A relative path resolves against the bootstrap file's
directory. SMTP trusts those certificates in addition to bundled roots; HTTPS webhooks
trust **only the file's certificates** when it is configured.
Include every root needed by the chosen webhook receiver. Keep the bundle managed by IT
and accessible within the service's filesystem restrictions; restart after changing its
path or contents because it is read at startup. An unreadable, empty or invalid bundle logs
`notification.ca_invalid` and causes TLS sends to fail; it does not disable verification.
Run `packie notify test` after a CA change and confirm receipt.

Plain SMTP (`security: "none"`) and HTTP are allowed only on loopback (`localhost`,
`127.0.0.1`, `::1`; bracket IPv6 in URLs), for a local relay/proxy managed by IT. Keep its
onward TLS settings and local access restricted. Webhooks do not follow redirects;
configure the final receiver URL without credentials in it.

See [notification behavior](running-and-monitoring.md#alert-notifications) and
[failed sends](troubleshooting.md#notification-failures). Notification delivery cannot
replace an independent check for a stopped computer or engine.

## Backup and restore

Set `PACKIE_BOOTSTRAP` as above, or pass `--bootstrap <installed-bootstrap>` to each command.
Use an encrypted backup volume and a directory writable by the actual service account
(root on packaged macOS, `packie` on Linux, LocalSystem on Windows). Create the backup root
with restricted OS permissions. On Windows check the target ACL explicitly; on Unix keep
it private to the service account. A signed-in user's mapped drive or credentials may be
unavailable to the service. On Linux the packaged unit permits writes only in state/log
directories: use an encrypted location under state for local backups, or have IT configure
the service's write access to an external mounted target.

Merge this `backup` object into the installed bootstrap, substituting an absolute backup
root appropriate for your OS; do not replace the rest of the document:

```json
{
  "backup": {
    "directory": "/absolute/path/on/encrypted-volume/packie-backups",
    "schedule": { "daily_at": "02:00", "keep": 7 },
    "stale_after_hours": 26
  }
}
```

Restart the engine after bootstrap edits. `daily_at` is local time; a time skipped by a
daylight-saving change runs at the next valid instant. Scheduling is off by default.
With a schedule, the default staleness threshold is 26 hours. After a scheduled backup,
only complete `packie-backup-*` directories beyond the newest `keep` are removed; other
directories and pre-restore state are left alone. Use the lab's backup system to take
copies off this computer and manage those copies' retention separately.

| Task | Command and effect |
| --- | --- |
| Back up now to the configured root | `packie backup create` creates a new `packie-backup-<UTC time>` directory. |
| Choose a different destination | `packie backup create <new-or-empty-directory>` writes there; the directory must be accessible to the service when it runs. |
| Check schedule and latest results | `packie backup status` shows next run, newest attempt/error, newest success and staleness; add `--json` for scripts. |
| Verify a backup without an engine | `packie backup verify <backup-directory>` checks the manifest's sizes/hashes, store integrity, referenced bodies and audit chain; exits 1 on problems. |

Creation uses consistent SQLite snapshots while the engine runs. With the engine stopped,
the CLI takes the instance lock and creates the backup offline; run it with access to the
service's files. The backup includes store, spilled bodies, audit files, configuration
versions/notes/sign-offs, the active runtime document, reference bootstrap and secret names.
It does not include secret values, diagnostic logs or folder/SFTP endpoint files; preserve
those separately where required. A directory without `manifest.json` is incomplete. See the
[backup specification](../spec/backup.md) for exact contents.

For a restore, use an administrator/service-account context with access to the state and
backup. This replaces the current store, audit and configuration:

1. Stop the engine with the elevated platform [service stop command](#service-commands),
   or Ctrl-C for a foreground engine; confirm it is stopped. Use the intended bootstrap.
2. Run `packie backup verify <backup-directory>` and investigate any failure.
3. Run `packie backup restore <backup-directory> --yes`. Restore takes the instance lock and
   verifies again; it refuses a running engine or a bad backup. Existing store, audit,
   configuration history and runtime are preserved in `<state>/pre-restore-<UTC time>/`.
   On Windows a folder cannot be moved while a file in it is open: when another program (an
   editor, a file-level backup or scanning agent) keeps a file in `store`, `audit` or
   `config` open beyond a short retry, the restore fails with "Access is denied" and
   changes nothing; close that program and run it again.
4. The installed bootstrap and secret values are unchanged. On a replacement computer,
   prepare the correct bootstrap/paths and service permissions. Keep interfaces isolated
   under the lab's downtime procedure until recovery checks are complete; the backup's
   `secrets.names.json`, when present, identifies secrets to re-enter; otherwise inspect the
   restored runtime and notification secret references. With the file provider, create the
   configured file containing `{}` with service-account access only (`0600` on Unix) before
   startup. Secret values are entered after startup through the engine.
5. Start the service. On a replacement computer, re-enter missing secret values using
   `packie secrets set <name>`; that CLI command needs the engine running. Check the active
   configuration, audit, channels and queues, and follow
   [restart reconciliation](compliance.md#7-downtime-and-restart-reconciliation). A restore
   reverts records to the snapshot: reconcile subsequent receipts/results at both peers
   before resending. Record the lab's recovery test and approval.

Restore is recorded in the restored audit chain; if that record cannot be written, restore
puts the previous state back and fails. Keep pre-restore copies private; they contain PHI and
remain until an operator removes them under the lab's retention/hold policy.

## Upgrade and removal

Before upgrading, take and verify a backup using the current engine, and preserve required
diagnostic/endpoint evidence and the service account's secrets through its configured provider.
For an older installation without the backup commands, stop the engine and retain a protected
copy of the bootstrap, complete state and any runtime file outside state.
Keep the previous installer and record `packie --version` and the console version. Message-store
migrations are forward-only; installing an older executable over a migrated store is not a
rollback method.

Install the replacement package for the same OS and architecture. Its scripts remove the old
service registration, preserve an existing bootstrap and data, then install and start the new
service. Reopen the console and check engine version, active configuration, channels, store,
queues and alerts. Resolve any `outcome_unknown` deliveries by checking the destination before
resending; a service restart can interrupt an attempt.

If `/etc/packie/bootstrap.json` selects the key store (`"secrets": {"provider": "keystore"}`),
an upgrade keeps that file and the package prints a note: without a Secret Service (a server or VM) the engine refuses to start and
the service stays failed. Change it to `"secrets": {"provider": "file", "file":
"/var/lib/packie/secrets.json"}`, run `sudo systemctl restart packie` and store the secrets again
with `packie secrets set`.

| Removal | Command or action | Retained data |
| --- | --- | --- |
| Service registration only | Platform `service uninstall` command above | Binary, console, bootstrap, state, logs and secrets |
| macOS application and engine | From a source checkout: `sudo packaging/macos/uninstall.sh` | Bootstrap, state and logs; add `--purge` only to delete those directories too |
| Linux package | `sudo apt remove packie` | Bootstrap, state (including the secrets file `/var/lib/packie/secrets.json`) and logs; `sudo apt purge packie` deletes them |
| Windows engine | Apps and features → Positive ACK, or `& 'C:\Program Files\Positive ACK\uninstall.exe'` in PowerShell | `C:\ProgramData\Positive ACK` remains; delete it manually only when its data is no longer needed |
| Windows console | Remove PACK-IE Console separately through Apps and features | It has a separate uninstall entry |

Purge deletes laboratory messages, audit and configuration history in the removed state path,
including pre-restore copies there. Backups outside that path remain. Package removal does
not erase secrets from the OS key store or remove a separately configured secrets file; manage those
through the configured provider when decommissioning the node.

For startup, permissions or connection failures, see [troubleshooting](troubleshooting.md).
