# Verifying the installers

Manual checks of what only a real installation shows: service registration, permissions,
restart and reboot behavior, single-instance protection, upgrade and removal. Continuous
integration builds and tests the engine on all three systems but does not install the
packages. Allow about 30 minutes per system. Use a test machine or VM, not a production lab
computer. The packages are unsigned: Windows SmartScreen will warn ("More info" → "Run anyway").

Tick each box and note any output that differs from **Expected**. Steps run in order.

| | |
| --- | --- |
| Tester | |
| Date | |
| Package version (`packie --version`) | |
| Windows edition and build (`winver`) | |
| Linux distribution and version (`cat /etc/os-release`) | |

## Windows

Use the installer `packie-<version>-windows-x64-setup.exe`. The installer adds the program
directory to the system `PATH` and sets `PACKIE_BOOTSTRAP` to the default bootstrap, so `packie` works
in any terminal opened **after** the installation. Then open **two** PowerShell windows:
**Admin** ("Run as administrator") and **User** (normal).

**Install**

- [ ] W1. Run the installer and accept the defaults.
  Expected: it finishes without an error box. The details pane shows
  "Service installed and started (system service)".
- [ ] W1a. In **User**, run `where.exe packie` and then `$env:PACKIE_BOOTSTRAP`.
  Expected: `C:\Program Files\Positive ACK\packie.exe` and
  `C:\ProgramData\Positive ACK\bootstrap.json`.
- [ ] W2. In **User**, run `packie service status`.
  Expected: `Service    running (system service)`, a PID, a `Since` time,
  `Bootstrap  C:\ProgramData\Positive ACK\bootstrap.json` and `IPC        \\.\pipe\packie-lab-node`.
- [ ] W3. In **Admin**, run `sc.exe qc packie`.
  Expected: `START_TYPE : 2 AUTO_START`, `SERVICE_START_NAME : LocalSystem`,
  `DISPLAY_NAME : Positive ACK`, and a `BINARY_PATH_NAME` with
  `C:\Program Files\Positive ACK\packie.exe`, `run --bootstrap`, the bootstrap path above and `--service`.
- [ ] W4. In **Admin**, run `sc.exe qfailure packie` and then `sc.exe qfailureflag packie`.
  Expected: three `RESTART -- Delay = 5000 milliseconds` actions and
  `FAILURE_ACTIONS_ON_NONCRASH_FAILURES: TRUE`.
- [ ] W5. Run `Get-EventLog -LogName Application -Source packie -Newest 1 | Format-List EventID,Message`.
  Expected: `EventID 1`, `Positive ACK started`.

**Data protection (SEC-03, SEC-04)**

- [ ] W5a. In **Admin**, run `icacls "C:\ProgramData\Positive ACK\state"` and
  `icacls "C:\ProgramData\Positive ACK\logs"`.
  Expected for each: exactly two entries, `NT AUTHORITY\SYSTEM:(OI)(CI)(F)` and
  `BUILTIN\Administrators:(OI)(CI)(F)` (localized names on non-English Windows). No
  `BUILTIN\Users`, `Everyone` or `Authenticated Users` entry, and no `(I)` (inherited) entries.
- [ ] W5b. In **Admin**, run `icacls "C:\ProgramData\Positive ACK\state\store\messages.sqlite3"` and the same
  for the newest file in `logs`.
  Expected: only SYSTEM and Administrators, as `(I)(F)` inherited from the directory.
- [ ] W5c. In **User**, run `Get-ChildItem "C:\ProgramData\Positive ACK\state"`.
  Expected: `Access to the path … is denied.` The bootstrap one level up stays readable:
  `Get-Content "C:\ProgramData\Positive ACK\bootstrap.json"` prints it (it holds no secrets).
- [ ] W5d. Secrets: with the default provider (`keystore`), secrets are in the Windows
  Credential Manager of the service account (LocalSystem). In **User**, `cmdkey /list` shows
  no `packie` entry. If the bootstrap uses `"provider": "file"`, run `icacls` on that file.
  Expected: only SYSTEM and Administrators.

**Operators group and attribution (P1, P3)**

- [ ] W5e. In **Admin**, run `net localgroup "PACK-IE Operators"`.
  Expected: the group exists and lists the account that ran the installer. Running the
  installer again succeeds without a duplicate-group error.
- [ ] W5f. In **Admin**, run `(Get-Acl \\.\pipe\packie-lab-node).Sddl`.
  Expected: `D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x12019f;;;S-1-5-21-…)` (or `GA` for `FA`), the
  SID being that of `PACK-IE Operators` (`Get-LocalGroup "PACK-IE Operators" | Select SID`); no `BU`.
- [ ] W5g. Sign in as a standard user who is **not** in `PACK-IE Operators` and run `packie status`.
  Expected: refused (`the engine is not running or not reachable … Access is denied`).
- [ ] W5h. As **User** (a member), run `packie status`, then in **Admin** look at the newest
  `operator.command` / `ipc.connected` lines in `C:\ProgramData\Positive ACK\logs\packie.<date>.log`.
  Expected: `operator_account` is `COMPUTER\user` (or `DOMAIN\user`), `uid_or_sid` an
  `S-1-5-21-…` SID and `pid` the client's process id. The console header shows the same account.
- [ ] W5i. Remove the group (`net localgroup "PACK-IE Operators" /delete`), restart the service.
  Expected: the log shows `ipc.group_unavailable`; an administrator can still connect; a
  standard user cannot. Run the installer again to restore the group.

**Management pipe and console**

- [ ] W6. In **User** (not elevated), run `packie status`.
  Expected: `Node      Laboratory node (lab-node)`, `State     degraded` (no runtime
  configuration is applied yet), a `Store     ok, …` line and `Channels (0)`.
- [ ] W7. Start **PACK-IE Console** from the Start menu as the same normal user.
  Expected: it attaches without a prompt and the Dashboard shows the node "Laboratory node".
- [ ] W8. From another computer on the network (optional), try to open `\\<this-pc>\pipe\packie-lab-node`,
  for example with the console's connection setting.
  Expected: refused. The pipe accepts local clients only.

**Single instance and restart**

- [ ] W9. In **Admin**, run `packie run`.
  Expected: exit code 1 and
  `packie: another engine is already running on C:\ProgramData\Positive ACK\state (pid N)`.
- [ ] W10. In **Admin**, run `taskkill /F /IM packie.exe`, wait 10 seconds, then run `packie service status`.
  Expected: `running` again, with a different PID.
- [ ] W10a. In **Admin**, break the bootstrap (for example set `"node_id": "Bad Id"`), run
  `packie service stop` and `packie service start`, wait 30 seconds, then run `packie service status`.
  Expected: `stopped`, not restarted; Event Viewer → Windows Logs → Application shows an
  error from source `packie`, ID 3, naming `node.node_id`. Restore the bootstrap and start again.
- [ ] W11. In **User**, run `packie service stop`.
  Expected: `packie: this needs administrator rights: run the terminal as administrator and try again`.
- [ ] W12. In **Admin**, run `packie service stop`, then `packie service status`.
  Expected: `Service stopped…`, then `stopped`. Event log (W5 command, `-Newest 2`): an
  `EventID 2` entry, `Positive ACK stopped`.
- [ ] W13. In **Admin**, run `packie service start`, then `packie service status` after 5 seconds.
  Expected: `running`.
- [ ] W14. Restart Windows and do not sign in for a minute. Then sign in and, in **User**, run
  `packie service status`.
  Expected: `running`, with `Since` close to the boot time.

**Upgrade and removal**

- [ ] W15. Run the same installer again while the service runs.
  Expected: it finishes. Afterwards `packie service status` is `running` and
  `C:\ProgramData\Positive ACK\bootstrap.json` is unchanged.
- [ ] W16. Uninstall "Positive ACK" in Settings → Apps.
  Expected: `sc.exe query packie` then says `The specified service does not exist as an installed service.`
  `C:\Program Files\Positive ACK` is gone and `C:\ProgramData\Positive ACK` (state, logs, bootstrap) is still there.
  In a new terminal, `where.exe packie` finds nothing and `$env:PACKIE_BOOTSTRAP` is empty.
- [ ] W17. Uninstall "PACK-IE Console" separately in Settings → Apps.
  Expected: it is removed.

## Linux (Debian or Ubuntu with systemd)

Use the package `packie_<version>_amd64.deb`. Run the commands as a normal user with sudo
rights. Call that user **you**.

**Install**

- [ ] L1. Run `sudo apt install ./packie_<version>_amd64.deb`.
  Expected: it ends without errors and prints
  `Service installed and started (system service)` and
  "<you> was added to the packie group; sign out and in again…".
- [ ] L1a. (Unattended installs.) On a second machine, or after `sudo apt purge packie`, install
  from a root shell without sudo or a login user, for example `sudo -i` then
  `SUDO_USER= apt install ./packie_<version>_amd64.deb` in a session where `logname` fails, such as
  a non-interactive SSH command: `ssh root@host "apt install -y /tmp/packie_<version>_amd64.deb"`.
  Expected: it prints "No user was added to the `packie` group (no sudo user is known). To use the
  console or `packie` with the service as <user>, run `sudo usermod -aG packie <user>` and sign in
  again." Then run that command for **you** and continue with L6.
- [ ] L2. Run `systemctl status packie --no-pager`.
  Expected: `active (running)` and `Main PID: N (packie)`.
- [ ] L3. Run `systemctl show packie -p User -p Group -p SupplementaryGroups -p Restart -p UMask`.
  Expected: `User=packie`, `Group=packie`, `SupplementaryGroups=dialout`, `Restart=on-failure`,
  `UMask=0007`.
- [ ] L4. Run `getent passwd packie` and then `getent group packie`.
  Expected: a system user whose home is `/var/lib/packie` and whose shell is `/usr/sbin/nologin`.
  The group lists **you**.
- [ ] L5. Run `sudo ls -la /var/lib/packie`.
  Expected: the directory is `drwxr-s---` owned by `packie packie`. `packie.sock` is
  `srw-rw----` owned by `packie packie`. `packie.instance.json` is `-rw-r-----` (group `packie` may read
  it; it holds no secrets). `packie.lock`, `secrets.json` (once a secret is stored) and the files
  in `store/` are owner-only.

**Management socket**

- [ ] L6. Sign out and in again (or run `newgrp packie`). Then run `packie --bootstrap /etc/packie/bootstrap.json service status`.
  Expected: `running (system service)`, the PID, a `Since` time (not `-`), the bootstrap, and
  `IPC        /var/lib/packie/packie.sock`.
- [ ] L7. As **you**, run `packie --bootstrap /etc/packie/bootstrap.json status`.
  Expected: `Node      Laboratory node (lab-node)`, `State     degraded` (no runtime
  configuration yet) and `Store     ok, …`.
- [ ] L7a. Run `sudo grep '"operator.command"\|"ipc.connected"' /var/log/packie/packie.$(date +%F).log | tail -3`.
  Expected: `operator_account` is **you**, `uid_or_sid` your uid (`id -u`) and `pid` the
  client's process id.
- [ ] L7b. Run `printf 'test-value' | packie --bootstrap /etc/packie/bootstrap.json secrets set verify-secret`,
  then `packie --bootstrap /etc/packie/bootstrap.json secrets list` and `sudo ls -l /var/lib/packie/secrets.json`.
  Expected: the secret is stored (the package uses the secrets file, no Secret Service is
  needed), `list` shows `verify-secret`, and the file is `-rw-------` owned by `packie packie`. Remove
  it with `packie --bootstrap /etc/packie/bootstrap.json secrets delete verify-secret`.
- [ ] L8. As a user outside the `packie` group, run `sudo -u nobody packie --bootstrap /etc/packie/bootstrap.json status`.
  Expected: an error that the engine is not reachable (permission denied). No status is printed.
- [ ] L9. Run `ls -l /dev/ttyS0 /dev/ttyUSB0 2>/dev/null`, or list any serial device present.
  Expected: group `dialout`. The service can open these devices through `SupplementaryGroups`.
  Confirm it later with a serial endpoint, if one is available.

**Single instance and restart**

- [ ] L10. Run `sudo -u packie packie --bootstrap /etc/packie/bootstrap.json run`.
  Expected: exit code 1 and `packie: another engine is already running on /var/lib/packie (pid N)`.
- [ ] L11. Run `sudo kill -9 $(systemctl show -p MainPID --value packie)`, wait 10 seconds, then run
  `systemctl show -p MainPID --value packie`.
  Expected: a different, non-zero PID. `journalctl -u packie -n 5` shows the restart.
- [ ] L12. Run `packie --bootstrap /etc/packie/bootstrap.json service stop` without sudo.
  Expected: `packie: this needs root: run it again with sudo`.
- [ ] L13. Run `sudo packie --bootstrap /etc/packie/bootstrap.json service stop`, then `systemctl is-active packie`.
  Expected: `inactive`.
- [ ] L14. Run `sudo packie --bootstrap /etc/packie/bootstrap.json service start`, then `systemctl is-active packie`.
  Expected: `active`.
- [ ] L15. Run `sudo reboot`. After boot, run `systemctl is-active packie` and `systemctl is-enabled packie`.
  Expected: `active` and `enabled`.

**Logs**

- [ ] L16. Run `sudo ls -l /var/log/packie`.
  Expected: `packie.<date>.log` files, mode `-rw-------`, owner `packie`.

**Upgrade and removal**

- [ ] L17. Run `sudo apt install ./packie_<version>_amd64.deb` again (reinstall).
  Expected: it succeeds and `systemctl is-active packie` is `active`. `/etc/packie/bootstrap.json` is unchanged.
- [ ] L17a. Upgrade over an older bootstrap: set `"secrets": {"provider": "keystore"}` in
  `/etc/packie/bootstrap.json`, then run `sudo apt install ./packie_<version>_amd64.deb` again.
  Expected: apt succeeds and prints the `packie: note:` lines about the key store and the file
  provider; the file is unchanged. `systemctl is-active packie` settles at `failed` (no restart
  every 5 s: `systemctl show packie -p NRestarts` stays the same over 30 seconds) and
  `journalctl -u packie -n 5` names the file provider. Set the provider back to `file` and run
  `sudo systemctl restart packie`: `active`.
- [ ] L18. Run `sudo apt remove packie`.
  Expected: `systemctl status packie` says `Unit packie.service could not be found.` and
  `/etc/packie`, `/var/lib/packie` and `/var/log/packie` still exist.
- [ ] L19. Run `sudo apt purge packie`.
  Expected: `/etc/packie`, `/var/lib/packie` and `/var/log/packie` are gone.
  The `packie` user and group remain. This is Debian practice for system users.

The console is not part of the `.deb`. On Linux, use the `packie` command line (L7) or build
the console from `console/`.

## macOS system daemon (optional)

These steps check the system daemon that the `.pkg` installs. A user agent (`service install
--user`) is for evaluation under one account and is not covered here.

- [ ] M1. Run `sudo installer -pkg packie-<version>-macos-<arch>.pkg -target /`.
  Expected: `installer: The install was successful.`
- [ ] M2. Run `packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service status`
  in a new terminal after signing out and in.
  Expected: `running (system service)` and
  `Definition /Library/LaunchDaemons/org.openbga.positiveack.plist`.
- [ ] M3. Run `dseditgroup -o checkmember packie`.
  Expected: `yes <you> is a member of packie`.
- [ ] M4. Open **PACK-IE Console** from Applications.
  Expected: it attaches to the node "Laboratory node".
- [ ] M5. Run `sudo pkill -9 -f "packie run"`, wait 15 seconds, then repeat M2.
  Expected: `running` with a new PID.
- [ ] M5a. Break the bootstrap (for example set `"node_id": "Bad Id"`), run
  `sudo packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service stop`
  and `… service start`, wait 30 seconds, then repeat M2.
  Expected: `stopped`, not relaunched; `/var/log/packie/launchd.stderr.log` ends with the
  `node.node_id` error and "the service is not restarted until this is fixed". Restore the
  bootstrap and start again.
- [ ] M6. Run `sudo packaging/macos/uninstall.sh` from a checkout, or the copy shipped with the release.
  Expected: `packie service status` then prints `Not installed`, and the state under
  `/Library/Application Support/Positive ACK/state` is kept.

## Reporting a failure

Record every difference under its step. For a failure, also collect:

- Windows: `C:\ProgramData\Positive ACK\logs\packie.<date>.log` (the last 200 lines)
- Linux: `journalctl -u packie -n 200 --no-pager`

The logs contain no message bodies.
