#!/bin/sh
# pult-desktop installer — macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/lonic-software/pult-desktop/main/install.sh | sh
#
# Installs the desktop app and exposes a `pult-desktop` launcher on your PATH:
#   • macOS  — drops pult-desktop.app into /Applications (falling back to
#              ~/Applications if that's not writable) and installs a
#              `pult-desktop` shim that runs `open -a` on it. The app is
#              unsigned (no Apple Developer account yet), so Gatekeeper
#              quarantines it on first download — this prints the
#              right-click → Open (or xattr) workaround.
#   • Linux  — installs the portable AppImage as `pult-desktop` (plus a
#              best-effort .desktop entry) and points out the .deb from the
#              same release as the apt-native alternative.
#
# Environment overrides:
#   PULT_DESKTOP_VERSION      install a specific tag, e.g. v0.1.0 (default: latest published release)
#   PULT_DESKTOP_INSTALL_DIR  where to put the `pult-desktop` command (default: ~/.local/bin)
#   PULT_DESKTOP_APP_DIR      macOS only: where to put the .app  (default: /Applications)
#   PULT_DESKTOP_REPO         GitHub repo slug                   (default: lonic-software/pult-desktop)
#   PULT_DESKTOP_BASE_URL     full base URL for the release assets (mirrors / air-gapped
#                             setups); overrides PULT_DESKTOP_REPO and skips the GitHub
#                             API entirely. Linux's asset filename embeds the version, so
#                             pair this with PULT_DESKTOP_VERSION there — macOS needs
#                             neither, its .app.tar.gz names are unversioned.
set -eu

REPO="${PULT_DESKTOP_REPO:-lonic-software/pult-desktop}"
VERSION="${PULT_DESKTOP_VERSION:-latest}"
INSTALL_DIR="${PULT_DESKTOP_INSTALL_DIR:-$HOME/.local/bin}"
APP_DIR="${PULT_DESKTOP_APP_DIR:-/Applications}"

say() { printf '%s\n' "$*"; }
err() { printf 'install.sh: error: %s\n' "$*" >&2; exit 1; }

# ── detect platform ─────────────────────────────────────────────
os=$(uname -s)
case "$os" in
    Darwin) platform="macos" ;;
    Linux)  platform="linux" ;;
    *) err "unsupported OS: $os — on Windows use install.ps1" ;;
esac

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        err "need curl or wget"
    fi
}

fetch_stdout() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$1"
    else
        err "need curl or wget"
    fi
}

# ── resolve the release to install ──────────────────────────────
# Releases start as drafts (CI publishes assets to a draft; a human
# publishes it) — /releases/latest and /releases/tags/<tag> both 404 until
# that happens, which is the common failure mode here, so it gets its own
# message rather than a bare "download failed".
if [ -n "${PULT_DESKTOP_BASE_URL:-}" ]; then
    base="$PULT_DESKTOP_BASE_URL"
    tag="$VERSION"
else
    if [ "$VERSION" = "latest" ]; then
        api="https://api.github.com/repos/${REPO}/releases/latest"
    else
        tag_in="$VERSION"; case "$tag_in" in v*) ;; *) tag_in="v$tag_in" ;; esac
        api="https://api.github.com/repos/${REPO}/releases/tags/${tag_in}"
    fi
    resp=$(fetch_stdout "$api" 2>/dev/null) && [ -n "$resp" ] || resp=""
    tag=$(printf '%s' "$resp" | sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' | head -n1)
    [ -n "$tag" ] || err "no published release found for ${REPO} ($api) — it may still be a draft pending publish. Check https://github.com/${REPO}/releases, or pass PULT_DESKTOP_VERSION for an exact tag."
    base="https://github.com/${REPO}/releases/download/${tag}"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

link_hint() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            say ""
            say "note: ${INSTALL_DIR} is not on your PATH. Add this to your shell profile:"
            say "    export PATH=\"${INSTALL_DIR}:\$PATH\""
            ;;
    esac
}

# ── macOS: .app bundle + a `pult-desktop` launcher ──────────────
install_macos() {
    arch=$(uname -m)
    case "$arch" in
        arm64|aarch64) asset="pult-desktop_aarch64.app.tar.gz" ;;
        x86_64|amd64)  asset="pult-desktop_x64.app.tar.gz" ;;
        *) err "unsupported macOS architecture: $arch" ;;
    esac

    say "downloading ${base}/${asset}"
    fetch "${base}/${asset}" "${tmp}/${asset}" \
        || err "download failed — does ${tag} have a ${asset} asset? ${base}/${asset}"

    tar -xzf "${tmp}/${asset}" -C "$tmp"
    # shellcheck disable=SC2012 # $tmp is ours alone, extracted from one known archive.
    app=$(cd "$tmp" && ls -d ./*.app 2>/dev/null | head -n1)
    [ -n "$app" ] || err "archive did not contain a .app bundle"
    app_name=$(basename "$app")

    # Prefer APP_DIR (creating it if needed); fall back to ~/Applications when
    # we can't write there (e.g. a locked-down /Applications without admin).
    dest_dir="$APP_DIR"
    if ! mkdir -p "$dest_dir" 2>/dev/null || [ ! -w "$dest_dir" ]; then
        dest_dir="$HOME/Applications"
        mkdir -p "$dest_dir"
    fi
    dest="${dest_dir}/${app_name}"

    rm -rf "$dest"
    mv "${tmp}/${app_name}" "$dest"
    say "installed ${dest}"

    mkdir -p "$INSTALL_DIR"
    cmd="${INSTALL_DIR}/pult-desktop"
    cat > "$cmd" <<EOF
#!/bin/sh
# Launch pult-desktop. Generated by install.sh.
exec open -a "${dest}" "\$@"
EOF
    chmod 755 "$cmd"
    say "installed ${cmd} — run 'pult-desktop' to launch"
    link_hint

    say ""
    say "macOS note: this build is unsigned (no Apple Developer account yet),"
    say "so Gatekeeper will refuse the first launch. Either right-click the app"
    say "in Finder and choose Open, or clear the quarantine flag yourself:"
    say "    xattr -dr com.apple.quarantine \"${dest}\""
}

# ── Linux: portable AppImage installed as `pult-desktop` ────────
install_linux() {
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) ;;
        *) err "no Linux build for $arch yet (only x86_64 / amd64) — see https://github.com/${REPO}/releases" ;;
    esac

    if [ -n "${PULT_DESKTOP_BASE_URL:-}" ] && [ "$VERSION" = "latest" ]; then
        err "PULT_DESKTOP_BASE_URL is set but PULT_DESKTOP_VERSION is not — the Linux asset filename embeds the version, so set PULT_DESKTOP_VERSION too (e.g. v0.1.0)"
    fi
    num="${tag#v}"
    asset="pult-desktop_${num}_amd64.AppImage"

    say "downloading ${base}/${asset}"
    fetch "${base}/${asset}" "${tmp}/${asset}" \
        || err "download failed — does ${tag} have a ${asset} asset? ${base}/${asset}"

    mkdir -p "$INSTALL_DIR"
    cmd="${INSTALL_DIR}/pult-desktop"
    staged="${INSTALL_DIR}/.pult-desktop.new.$$"
    cp "${tmp}/${asset}" "$staged"
    chmod 755 "$staged"
    mv -f "$staged" "$cmd"
    say "installed ${cmd} — run 'pult-desktop' to launch"
    say "(on a .deb-based distro, pult-desktop_${num}_amd64.deb from the same release is the apt-native alternative)"

    # Best-effort desktop entry so it appears in the application menu.
    apps="$HOME/.local/share/applications"
    if mkdir -p "$apps" 2>/dev/null; then
        cat > "${apps}/pult-desktop.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=pult-desktop
Comment=Desktop companion for pult
Exec=${cmd} %U
Terminal=false
Categories=Development;
EOF
    fi
    link_hint
}

case "$platform" in
    macos) install_macos ;;
    linux) install_linux ;;
esac
