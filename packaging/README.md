# Packaging

Installers lay the files down and call `packie service install`; service registration lives in
the `packie` binary, not here. Output goes to `target/packaging/`. Nothing is signed or notarized.
Build the binary first (`cargo build --release -p packie`); the console bundle
(`npm run tauri build` in `console/`) is optional and is included when present.

The version comes from `[workspace.package] version` in the root `Cargo.toml`
(`packaging/version.sh`, `packaging/version.ps1`).

Each script packages `PACKIE_BIN` (or its first argument), defaulting to `target/release/packie[.exe]`
or the only `target/<triple>/release/packie[.exe]`, and stops with a message when it is missing.
The console is taken from `PACKIE_CONSOLE_APP` (macOS) or `PACKIE_CONSOLE_INSTALLER` (Windows), else
from the Tauri bundle output under `console/src-tauri/target[/<triple>]/release/bundle/`.

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Build on | macOS | any Linux with `dpkg-deb` | Windows with NSIS 3 (`makensis` on PATH) |
| Command | `packaging/macos/build-pkg.sh` | `packaging/linux/build-deb.sh [bin] [arch]` | `packaging\windows\build.ps1` |
| Artifact | `packie-<ver>-macos-<arch>.pkg` | `packie_<ver>_<arch>.deb` | `packie-<ver>-windows-x64-setup.exe` |
| Install | double-click, or `sudo installer -pkg <pkg> -target /` | `sudo apt install ./packie_<ver>_<arch>.deb` | run as administrator (`/S` for silent) |
| Binary | `/usr/local/bin/packie` | `/usr/bin/packie` | `C:\Program Files\Positive ACK\packie.exe` |
| Bootstrap (written only if missing) | `/Library/Application Support/Positive ACK/bootstrap.json` | `/etc/packie/bootstrap.json` (template `/usr/share/packie/bootstrap.json`) | `C:\ProgramData\Positive ACK\bootstrap.json` |
| State | `/Library/Application Support/Positive ACK/state` | `/var/lib/packie` | `C:\ProgramData\Positive ACK\state` |
| Logs | `/Library/Logs/Positive ACK` | `/var/log/packie` | `C:\ProgramData\Positive ACK\logs` |
| Console | `/Applications/PACK-IE Console.app` | not packaged | Tauri setup run silently |
| Remove | `sudo packaging/macos/uninstall.sh [--purge]` | `sudo apt remove packie` / `sudo apt purge packie` | Apps and features, or `uninstall.exe` |

The Windows installer also adds the program directory to the system `PATH` and sets the
machine variable `PACKIE_BOOTSTRAP` to the default bootstrap, so `packie status` works in any new
terminal; uninstalling removes both. On Linux and macOS, the user who ran `sudo` joins the
`packie` group; without one (a root shell, an unattended install) `packie service install` prints the
command that adds a user (`sudo usermod -aG packie <user>` on Linux).

Defaults: node id `lab-node`, environment `prod`, secrets provider `keystore` (the OS key
store; the Linux package uses `file`, because a server has no Secret Service). Edit the
bootstrap and restart the service to change them. On Linux and macOS the management socket
is usable by the `packie` group.

Removal keeps the bootstrap, state and logs (they hold laboratory data). `--purge` (macOS) and
`apt purge` (Linux) delete them as well; on Windows delete `C:\ProgramData\Positive ACK`
by hand. The Windows console has its own entry in Apps and features.

Prerequisites: macOS needs `pkgbuild` and `productbuild` (Xcode command line tools); the
Debian package needs `systemd` on the target and runs without service registration (with a
message) when systemd is not running, for example in a container image build.

## macOS end to end

Prerequisites: macOS with the Xcode command line tools (`pkgbuild`, `productbuild`), Rust,
and Node with the console dependencies (`npm ci` in `console/` when `node_modules` is missing).
The build is native: an Apple Silicon host produces an `arm64` package (the installer's
`hostArchitectures` is set from the binary).

```sh
(cd console && npm run tauri build)   # console/src-tauri/target/release/bundle/macos/PACK-IE Console.app (+ dmg/)
cargo build --release -p packie          # target/release/packie
packaging/macos/build-pkg.sh          # target/packaging/packie-<ver>-macos-<arch>.pkg
```

The Tauri build needs no signing identity and does not notarize. `build-pkg.sh` finds the
bundle by itself, copies it into the package, and seals it ad hoc when the bundler left it
unsealed (a properly signed `PACKIE_CONSOLE_APP` is kept as is). The package is unsigned and not
notarized: Gatekeeper rejects it when downloaded; install it with `sudo installer -pkg <pkg> -target /`.

Inspect it without installing:

```sh
pkg=target/packaging/packie-<ver>-macos-arm64.pkg
pkgutil --expand-full "$pkg" /tmp/packie-pkg      # Distribution, packie-engine.pkg/Scripts/{postinstall,bootstrap.json},
                                               # packie-engine.pkg/Payload/usr/local/bin/packie,
                                               # packie-console.pkg/Payload/PACK-IE Console.app
lsbom -s /tmp/packie-pkg/packie-console.pkg/Bom      # file list of one component (also packie-engine.pkg/Bom)
plutil -lint "/tmp/packie-pkg/packie-console.pkg/Payload/PACK-IE Console.app/Contents/Info.plist"
codesign --verify --deep --strict -v "/tmp/packie-pkg/packie-console.pkg/Payload/PACK-IE Console.app"
```

The bills of materials also list `._*` entries: they carry the `com.apple.provenance`
attribute macOS puts on every file it creates and cannot be removed. The installer restores
them as attributes, not as files.

Manual qualification on Windows, Linux and the macOS system daemon: [VERIFY.md](VERIFY.md).

## Reproducible engine build

The engine binary is reproducible on Linux and macOS: building the same commit twice, from
any two checkout paths, gives the same SHA-256. Build paths are the only source of
differences, so `packaging/repro-flags.sh` maps them to fixed prefixes (rustc
`--remap-path-prefix`, `-ffile-prefix-map` for the C code of the bundled SQLite and ring, and
on macOS ld64 `-oso_prefix`, without which object paths change the Mach-O UUID):

```sh
while IFS='=' read -r name value; do export "$name=$value"; done < <(packaging/repro-flags.sh)   # bash
cargo build --release --locked -p packie
shasum -a 256 target/release/packie
```

The toolchain is pinned by `rust-toolchain.toml` and dependencies by `Cargo.lock`. The release
workflow's `reproducible` job builds twice from two paths on Linux and macOS and fails on any
difference, and the release builds use the same flags, so a published binary can be compared
with a local build of its tag.
