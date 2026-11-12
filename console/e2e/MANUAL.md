# Manual check of the Tauri window (about ten minutes)

The browser journeys in this folder (`npm run e2e`) prove what the pages do against the mock
engine. They cannot prove what only the desktop app does: the window itself, the connection to
the real engine through the Rust shell, closing and reloading, light and dark mode, the
service-start button. This checklist is for a maintainer. Tick each box; a "no" on any
line is a defect to report with a screenshot. Run it in a logged-in, unlocked desktop session: a
locked session or a sleeping display shows no window and cannot be screenshotted, so the steps
that look at the window (and any screenshot) cannot be done there.

## Set up (2 minutes)

Run these from the repository root, in separate terminals. Node 22 or newer is needed for the
peers; nothing else is installed.

```sh
# 1. The engine, with its own state under console/e2e/manual/ (ignored by git)
cargo run -p packie -- run --bootstrap console/e2e/manual/bootstrap.json
# 2. Load the check configuration: a bridge (5301 <-> 5302) and an HL7 message channel (5401 -> 5402)
cargo run -p packie -- --bootstrap console/e2e/manual/bootstrap.json config apply console/e2e/manual/runtime.json
# 3. The peers (one terminal each)
node console/e2e/manual/peers.mjs bridge     # instrument and system around the bridge
node console/e2e/manual/peers.mjs sink       # the "LIS": answers AA to HL7 messages on 5402
node console/e2e/manual/peers.mjs feed       # sends one HL7 result every 2 s to 5401
```

Open the console: in development `cd console && PACKIE_BOOTSTRAP=$PWD/e2e/manual/bootstrap.json npm run tauri dev`;
from an installed build, start the app (it finds the installed engine itself; see step 36).

## The check

| # | Do | Expect | OK |
| --- | --- | --- | --- |
| 1 | Stop the engine (Ctrl-C in its terminal), then start the console. | The window opens on **The engine is not running**: it says where it looked (a socket path or pipe name), shows a **Start the service** button and the administrator-terminal command for your system. No blank page, no error dialog. | ☐ |
| 2 | Start the engine again (step 1 of the set-up; the configuration is kept). | Within about 10 seconds, with no click, the console continues to the **Dashboard**; the top bar shows "Manual check node". | ☐ |
| 3 | Look at the **Dashboard** without scrolling. | Two channels, both **Running**, each row with state words and a shape (not colour alone); the strip shows channels by state, no alerts; Traffic shows bytes moving (bridge) and received / acknowledged counts growing (HL7). | ☐ |
| 4 | Stop the `sink` peer (Ctrl-C). Wait about 30 seconds. | **HL7 results to LIS** turns **Degraded**, moves to the top, shows a critical or warning alert with a next step; the endpoint "LIS HL7 out" shows Retrying with a countdown. The bridge row stays calm. Restart `sink`: it clears by itself. | ☐ |
| 5 | Open **Instrument to system**. Press **Pause**. | A dialog says what happens (session closes, accepted messages stay queued) and offers Cancel; Cancel changes nothing. Confirm: the page reads **Paused by operator** with no error styling and offers **Resume**. | ☐ |
| 6 | Press **Resume**. | No confirmation; a message "Starting from configuration version N."; the channel runs again and bytes move. | ☐ |
| 7 | On the channel page start the **live transcript**. | Two panes, A → B and B → A, each segment timestamped (`ping 12` one way, `pong 12` the other); **Freeze** stops the display, Stop ends it. | ☐ |
| 8 | Open **Messages**. | HL7 results appear newest first with their deliveries ("Written"/"Received"/"Accepted" wording, never "delivered" for a lower level). Type a control id such as `CTRL-MANUAL-3` in **Identifier**: one row. | ☐ |
| 9 | Open that message. Press **Show original message**, then **Hex**, then **Fields** and **Show content**. | The HL7 text with control characters named, the hex dump, and, only after **Show content**, the engine's list of fields (`PID.3`, `OBX.5`…); choosing Fields alone reads nothing. The window title and address bar never contain the message. Press **Resend…**: both modes explained; confirm duplicate risk is requested for an acknowledged delivery. | ☐ |
| 10 | Open **Logs**. | Live events scroll in at the top; scroll down and **Jump to live** appears; click it. Filter by level **Warning and above** and by channel. No message bodies in any row. | ☐ |
| 11 | **Settings > Engine**, then **Settings > Versions**. | Engine settings read-only with a note that edits need a restart; the version list shows version 1 as current. | ☐ |
| 12 | **Channels > HL7 results to LIS > Edit**. Change the label, press **Validate**, then **Save and apply**. | A plan dialog lists the channel as updated or restarted and the other as untouched, and asks for a **Change note**: **Apply** without one is refused. With a note, a toast names version 2; **Settings > Versions** shows it with your account, the time and your note. | ☐ |
| 13 | In an editor, type in the Label field and then close the window (the red button / Alt-F4). | A dialog asks **Discard unsaved changes and close?**; Cancel keeps the window and your text; confirming closes it. Reloading (Cmd/Ctrl-R) with unsaved text also asks first. | ☐ |
| 14 | Stop the engine while the console is open and showing the Dashboard. | The pages stay, dimmed, under **Lost the connection to the engine** (reconnecting). Start the engine: the banner goes and data is live again without reloading. | ☐ |
| 15 | Open **Help** (top bar) on three different pages. | A drawer titled **Page help** with a short topic for each page; Escape closes it and focus returns to the button. | ☐ |
| 16 | Switch the operating system between light and dark. | The console follows at once; text and state colours stay readable. | ☐ |
| 17 | Make the window as small as it will go (1024 × 640), then large. | No overlapping text; the Dashboard keeps Channel, State and Endpoints; grids keep their action column. | ☐ |
| 18 | Close the console with the engine running. | The engine keeps running (check its terminal); starting the console again attaches. | ☐ |
| 19 | Look at the top bar. | **Signed in as** names your operating system account, as `whoami` prints it (`DOMAIN\name` on Windows). It is the account the audit log and the version history use. | ☐ |
| 20 | Open **Audit** (under Diagnose). | What you did in steps 5 to 12 is listed newest first with your account and the origin **console**: the pause and resume (each recorded as requested, then its outcome), the identifier search, the body and fields views, the configuration apply with its note. Choose a row: the detail shows the note in full. | ☐ |
| 21 | On **Audit**, press **Export…**, choose CSV, then **Export**. | The operating system's **Save** dialog opens named `packie-audit-<date>.csv`. Cancel: nothing is written and nothing is announced. Export again and save: a message names the number of events and the file; open it in a spreadsheet (a header row, the filtered events oldest first); on macOS and Linux its permissions are owner-only. Export again choosing the same file name and confirm the dialog's replace prompt: the first file stays exactly as it was and the new one is saved as `<name>-1.csv`, which the message names (an export never replaces or follows a link). | ☐ |
| 22 | On **Audit**, press **Verify chain**. | **The chain is intact**, with the events, the first and last number and the head hash. `cargo run -p packie -- --bootstrap console/e2e/manual/bootstrap.json audit verify` prints the same head hash (it differs by the events these two checks added). | ☐ |
| 23 | **Settings > Versions**, open version 2, then **Mark as verified…** with a reference such as `VR-1`. | Without a reference it is refused. After it, the row says **Verified** and the detail names your account, the time and `VR-1`. | ☐ |
| 24 | **Settings > Backup**, then **Back up now** and confirm. | The status shows the directory `console/e2e/manual/backups`; after a few seconds a result lists the directory, files, size, messages and the configuration version. `cargo run -p packie -- --bootstrap console/e2e/manual/bootstrap.json backup verify <that directory>` reports no problem. The restore section has no button; it shows the commands naming the bootstrap file `console/e2e/manual/bootstrap.json` as an absolute path (the path the console found, not a default), then start, the health checks and the secrets reminder. | ☐ |
| 25 | **Settings > Privacy**: set 1 minute and Save. Open a message, **Show original message**, then leave the window alone for 70 seconds. | The message detail closes and the **Messages** list is behind a cover saying content was hidden after 1 minute without activity, with a **Continue** button (it has focus). Continue shows the list; open the message again: no text is shown until **Show original message** is pressed. Set the time back to 15. | ☐ |
| 26 | On **Messages**, narrow the filter (for example one channel), press **Export…**, choose CSV and Export. | The dialog restates the filter, warns that the file lists patient identifiers and says where to save it. The native **Save** dialog opens named `packie-messages-<date>.csv`; Cancel writes nothing and announces nothing. Saved: a message names the count and the file; the file has a header row and the filtered messages oldest first; on macOS and Linux it is owner-only. Audit shows `message_export` with the filter and count. | ☐ |
| 27 | Export again with **Include message content** ticked. Stop the engine while the window is saving a large export, if you can (otherwise skip). | A stronger warning appears before Export; the file has a `body` column and Audit records it as a content view. A broken export reports a failure and leaves no file at the chosen path. | ☐ |
| 28 | In the terminal where `feed` runs, type `revise` and press Enter: the feed sends `CTRL-REVISED-1` twice over its own connection, the second time corrected (the HL7 listener takes one peer at a time, so a second script cannot connect while `feed` runs; with `feed` stopped, `node console/e2e/manual/peers.mjs revise` does the same on its own connection). Open the older of the two `CTRL-REVISED-1` messages and press **Resend…**. | The detail says a newer revision supersedes it; the dialog names the newer message with its received time and a link, and **Resend** stays off until the confirmation is ticked. Resend: Audit's `message_resend` shows `confirm_superseded`. | ☐ |
| 29 | **Diagnose > Transmission report**: choose **HL7 results to LIS**, the last 24 hours, **Run report**, then **Export CSV…**. | Totals per destination and one row per delivery (control id, content hash, ack level, ack code, acknowledged time). The Save dialog opens named `packie-transmission-<channel>-<date>.csv`; the saved file holds the rows shown. Audit shows `transmission_verify`. **Show source and versions** adds the source endpoint and peer and the configuration and mapping versions (also in the file). With a period of more than 10,000 deliveries (not reachable with the manual feed; use the simulator or a busy system) the page says the rows stop early while the totals cover the whole period, the export dialog warns, and the file ends with `# listed:` and `# truncated:` lines. | ☐ |
| 30 | **Settings > Versions**, **Export change report…**, choose JSON and All versions. | The Save dialog opens named for the report; the saved file lists every version with account, time, note, per-channel outcomes, field changes with values and each verification (version 2 shows your `VR-1`). The dialog warned that notes can hold patient identifiers. **Audit** shows **Client export written** (kind `change_report`, the number of versions) before the file was written; stop the audit log from being written (make its directory read-only) and the export is refused with nothing saved. | ☐ |
| 31 | In the Versions list, open version 1 and **Roll back to this version…**. | The dialog shows the engine's plan (what restarts, updates in place or stays untouched) before you can confirm; Cancel changes nothing. | ☐ |
| 32 | **Settings > Notifications**. With the manual bootstrap (it has no `notifications` section) the tab says notifications are off. Add a `notifications.webhook` to `console/e2e/manual/bootstrap.json` pointing at `http://127.0.0.1:5499/` (a closed port), restart the engine, reopen the tab. | The webhook is listed read-only with its origin only (no path), its severities and timeout, and a note that changes need the bootstrap file and a restart. **Send test…** asks first; the result names the webhook with the engine's failure text; within a few seconds an open **notifications_failing** alert appears on the Alerts page with a link to this tab. Remove the section again afterwards. | ☐ |
| 33 | **Endpoints > New endpoint**, kind **Serial**. Open the **Port** field. | The list shows the serial ports of this computer (name, bus, product, `vid:pid`), with ports other endpoints name marked **used by …** and **open by …**; with no adapter attached it says no ports were found and a name can still be typed. Plug in a USB adapter and press **Refresh**: it appears. | ☐ |
| 34 | In an endpoint editor for a TCP client, set the host to `127.0.0.1` and the port to `5302` (the bridge's system peer, running) and press **Test connection**; then change the port to `5999`. | Steps resolve and connect with times, and the summary says the connection worked; no data reaches the peer (the `bridge` peer's terminal prints `[system] the bridge connected` once for each test, because the test opens and closes a connection; nothing else about it, and the `[instrument] got pong N` lines carry on unchanged) and nothing is saved (leaving asks nothing). At port 5999 the connect step fails with the reason. **Audit** shows both tests as **Endpoint tested**. On the page of a running listener endpoint the test reports the port as in use and says that is expected. For a folder endpoint the panel says before the test that it creates and deletes one empty hidden file in the folder (no test file is left behind). | ☐ |
| 35 | **Settings > About**. | **Console version** is the version of the window you opened: the `version` in `console/src-tauri/tauri.conf.json` (the installer carries the same number); **Engine version** is the engine's own and **Protocol version** the one both speak. | ☐ |

## Installed build only

| # | Do | Expect | OK |
| --- | --- | --- | --- |
| 36 | With the service installed but stopped, open the installed console. | It finds the installed engine's bootstrap itself (no environment variable) and shows **The engine is not running**. **Start the service** either starts it or shows the engine's own refusal text under the administrator command. | ☐ |
| 37 | Run the shown command in an administrator terminal. | Within about 10 seconds the console continues to the Dashboard. | ☐ |

Clean up: stop the peers and the engine; `console/e2e/manual/state`, `logs` and `backups` can be deleted.
