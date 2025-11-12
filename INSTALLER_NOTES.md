You don’t need crates.io at all. Treat ELF like a normal app suite: build release binaries in CI, publish tarballs/zip files to a place you control (GitHub Releases, S3, on-prem), and provide tiny installers that fetch + unpack to a versioned prefix and symlink into the user’s PATH.

Here’s a practical, cross-platform plan that scales from “copy binaries” to proper packages—pick what you need now and grow later.

# 1) Fast path: portable tarballs + tiny installer

**New recording banner:** The GUI now surfaces the Parquet recorder state (Idle/Starting/Recording/Error) in the ECG controls. When you update installers or release notes, call this out so lab operators know to look at the left rail for “Parquet recording” before unplugging sensors.

**Artifacts (per OS/arch)**

* `elf-<version>-x86_64-apple-darwin.tar.xz` (contains `elf-cli`, `elf-gui`, `elf-run`, etc.)
* Same for Linux and Windows (zip).

**Installer script (no root required)**

* Unpack to `~/.local/opt/elf/<version>/bin`
* Symlink into `~/.local/bin` (on Linux/macOS) or `%LOCALAPPDATA%\Programs\elf\bin` (Windows)
* Maintain a `current` symlink for rollbacks.

`install.sh` (drop at the repo root and host alongside your tarballs):

```bash
#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-latest}"
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=aarch64 ;; esac

# Where to put things
PREFIX="${PREFIX:-$HOME/.local}"
OPT="$PREFIX/opt/elf"
BIN="$PREFIX/bin"
mkdir -p "$OPT" "$BIN"

# Resolve asset URL (adapt to your hosting)
BASE="https://example.com/elf/releases"
if [ "$VERSION" = "latest" ]; then
  VERSION="$(curl -fsSL "$BASE/LATEST.txt")"
fi
FILE="elf-${VERSION}-${ARCH}-${OS}.tar.xz"
URL="$BASE/${VERSION}/${FILE}"
SHASUM_URL="$URL.sha256"

echo "Downloading $URL ..."
curl -fsSL "$URL" -o "/tmp/$FILE"
curl -fsSL "$SHASUM_URL" | sha256sum -c -  # or shasum -a 256 -c -

DEST="$OPT/$VERSION"
rm -rf "$DEST" && mkdir -p "$DEST"
tar -xJf "/tmp/$FILE" -C "$DEST"

# Symlink current + tools
rm -f "$OPT/current" && ln -s "$DEST" "$OPT/current"
for exe in elf-cli elf-gui elf-run; do
  ln -sf "$OPT/current/bin/$exe" "$BIN/$exe"
done

echo "Installed ELF $VERSION to $DEST"
echo "Ensure $BIN is on your PATH (e.g., export PATH=\"$BIN:\$PATH\")"
```

`uninstall.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
OPT="$PREFIX/opt/elf"
BIN="$PREFIX/bin"
rm -f "$BIN/elf-cli" "$BIN/elf-gui" "$BIN/elf-run"
rm -f "$OPT/current"
echo "Binaries removed. Delete $OPT/* to remove cached versions."
```

> System-wide? Use `PREFIX=/usr/local` and run with `sudo`. Prefer per-user installs unless you really need global.

# 2) Make it reproducible in CI

* Build all targets (`cargo build --release`) and bundle per-OS archives.
* Emit `SHA256SUMS` and a `LATEST.txt` file (the installer reads this).
* Optionally sign checksums (e.g., `minisign` or `cosign`) and verify in `install.sh`.

A minimal `justfile` target:

```make
release:
	cargo build --release --workspace
	./scripts/package.sh
	./scripts/checksums.sh
```

# 3) Optional “nice” packages (no crates.io needed)

Pick any/all as your user base grows:

* **Homebrew (macOS/Linux)**: create a private Tap that points to your tarballs. Users do:

  ```
  brew tap yourorg/elf https://github.com/yourorg/homebrew-elf
  brew install elf
  ```

  The formula just downloads your tarball and puts binaries on PATH.

* **Debian/Ubuntu**: use `cargo-deb` to make a `.deb`.

  ```toml
  # crates/elf-cli/Cargo.toml
  [package.metadata.deb]
  maintainer = "ELF Team <ops@example.com>"
  depends = "libc6 (>= 2.31)"
  assets = [
    ["target/release/elf-cli", "usr/bin/", "755"],
  ]
  ```

  CI: `cargo deb` → upload `.deb`.

* **RPM**: `cargo-rpm` for `.rpm`.

* **Windows**:

  * Easiest: zip + `install.ps1` (adds a per-user bin dir to PATH and symlinks).
  * Nicer: Scoop bucket (private) or `choco` package pointing at your zip.
  * Fancy: MSI via `cargo-wix` if you need a proper Windows installer.

* **Nix**: a small `flake.nix` lets Nix users `nix run .#elf-cli`. Great for labs.

* **AppImage** (Linux GUI): wrap `elf-gui` for portable double-clickable GUI.

# 4) Keep GUI + CLI separate but co-installable

Publish one archive with `bin/elf-cli`, `bin/elf-run`, `bin/elf-gui`. Your installer symlinks all. Users can also delete `elf-gui` if they only need headless servers.

# 5) Versioned bundles for experiments

Your experiment runner already writes BIDS-like run folders. For the *software* bundle itself, embed:

* `elf --version` → git SHA + build time
* `elf env` → prints exact crate versions and CPU/OS for provenance
* Include `RUN_MANIFEST.json` alongside a run with `elf-run`’s exact version + SHA.

# 6) When you need zero-touch upgrades

Later you can add:

* `elf self update` (checks `LATEST.txt`, downloads, swaps `current` symlink).
* A `--channel` flag for `stable` vs `nightly` URLs.

# 7) Code signing / Gatekeeper (macOS)

If you distribute to non-dev users on macOS:

* Notarize and sign the app (for `elf-gui.app`) or sign the binaries; otherwise users will need to bypass Gatekeeper. This doesn’t require crates.io—just Apple dev certs.

---

## TL;DR

Start with **portable tarballs + a tiny `install.sh`** that installs to `~/.local` and symlinks into `~/.local/bin`. Grow into **Homebrew tap**, **.deb/.rpm**, **Scoop**, and **AppImage** as needed. No crates.io dependency anywhere; you’re shipping binaries.
