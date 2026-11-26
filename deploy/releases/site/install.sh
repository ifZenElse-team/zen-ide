#!/bin/sh
# Installs the latest Zen Toolbox, the entry point to the Zen IDE family.
#
#   curl -fsSL https://ide.ifzenelse.space/install.sh | sh
#
# What it does: reads the release manifest over HTTPS, checks its signature against the publisher's
# public key below, and downloads the newest Zen Toolbox archive for this computer, checking it against
# the SHA-256 in that manifest.
#
# - On a Mac (Apple Silicon): copies "Zen Toolbox.app" into /Applications (or ~/Applications if
#   /Applications is not writable) and opens it.
# - On Linux (x86_64, glibc 2.35 or later, such as Ubuntu 22.04 and later): installs it for this user
#   only, without sudo: the files in ~/.local/share/zen-toolbox/app, the command
#   ~/.local/bin/zen-toolbox, an entry in the applications menu with its icon, and starts it.
#
# Nothing else on the system is changed.
#
# Environment: ZEN_RELEASES (default https://ide.ifzenelse.space), ZEN_PUBLIC_KEY (the key of another
# release server), ZEN_INSTALL_DIR (overrides the destination), ZEN_NO_OPEN=1 (do not open the app
# afterwards).
set -eu

RELEASES="${ZEN_RELEASES:-https://ide.ifzenelse.space}"
PUBLIC_KEY="${ZEN_PUBLIC_KEY:-464c3ceb2e14dfdf571c14dbb4114f21b3ee9014a0cacca158933ed6dab9bf33}"
APP="Zen Toolbox.app"

say() { printf '%s\n' "$*"; }
fail() { printf 'zen-toolbox install: %s\n' "$*" >&2; exit 1; }

# Downloads the manifest with its signature; $1 is the folder. The signature is a hex Ed25519 one.
fetch_manifest() {
    curl -fsSL "$RELEASES/releases/manifest.json" -o "$1/manifest.json" || fail "cannot reach $RELEASES"
    curl -fsSL "$RELEASES/releases/manifest.json.sig" -o "$1/manifest.json.sig" || fail "cannot read the manifest's signature"
    SIGNATURE="$(tr -d '[:space:]' < "$1/manifest.json.sig")"
    case "$SIGNATURE" in
        *[!0-9a-f]*) fail "the release manifest's signature is malformed" ;;
    esac
    [ "${#SIGNATURE}" -eq 128 ] || fail "the release manifest's signature is malformed"
}

# ---- Linux -------------------------------------------------------------------------------------
install_linux() {
    case "$(uname -m)" in
        x86_64 | amd64) PLATFORM="linux-x86_64" ;;
        *) fail "only x86_64 Linux is supported for now" ;;
    esac
    command -v python3 >/dev/null 2>&1 || fail "python3 is needed to read the release manifest"
    WORK="$(mktemp -d)"
    trap 'rm -rf "$WORK"' EXIT INT TERM

    say "Checking the latest Zen Toolbox..."
    fetch_manifest "$WORK"
    python3 - "$WORK/manifest.json" "$SIGNATURE" "$PUBLIC_KEY" <<'VERIFY_PY' || fail "the release manifest's signature does not verify: nothing was installed"
import hashlib, sys

P = 2**255 - 19
Q = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
ROOT = pow(2, (P - 1) // 4, P)

def add(a, b):
    e, f, g, h = (a[1] - a[0]) * (b[1] - b[0]) % P, (a[1] + a[0]) * (b[1] + b[0]) % P, 2 * a[3] * b[3] * D % P, 2 * a[2] * b[2] % P
    i, j, k, l = f - e, h - g, h + g, f + e
    return (i * j % P, k * l % P, j * k % P, i * l % P)

def times(n, point):
    total = (0, 1, 1, 0)
    while n:
        if n & 1:
            total = add(total, point)
        point = add(point, point)
        n >>= 1
    return total

def decode(raw):
    y = int.from_bytes(raw, "little") & (2**255 - 1)
    if y >= P:
        return None
    square = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    x = pow(square, (P + 3) // 8, P)
    if (x * x - square) % P:
        x = x * ROOT % P
    if (x * x - square) % P or (x == 0 and raw[31] >> 7):
        return None
    if x & 1 != raw[31] >> 7:
        x = P - x
    return (x, y, 1, x * y % P)

def verify(message, signature, public):
    key, r = decode(public), decode(signature[:32])
    s = int.from_bytes(signature[32:], "little")
    if key is None or r is None or s >= Q:
        return False
    k = int.from_bytes(hashlib.sha512(signature[:32] + public + message).digest(), "little") % Q
    y = 4 * pow(5, P - 2, P) % P
    x = decode(y.to_bytes(32, "little"))[0]
    if x & 1:
        x = P - x
    left, right = times(s, (x, y, 1, x * y % P)), add(r, times(k, key))
    return (left[0] * right[2] - right[0] * left[2]) % P == 0 and (left[1] * right[2] - right[1] * left[2]) % P == 0

sys.exit(0 if verify(open(sys.argv[1], "rb").read(), bytes.fromhex(sys.argv[2]), bytes.fromhex(sys.argv[3])) else 1)
VERIFY_PY
    # The newest stable release for this platform: version, archive path and SHA-256.
    FOUND="$(python3 - "$WORK/manifest.json" "$PLATFORM" <<'PY'
import json, sys
manifest = json.load(open(sys.argv[1]))
def key(version):
    return [int(part) if part.isdigit() else 0 for part in version.split("-")[0].split(".")]
best = None
for product in manifest.get("products", []):
    if product.get("id") != "toolbox":
        continue
    for release in product.get("releases", []):
        if release.get("channel") != "stable":
            continue
        for artifact in release.get("artifacts", []):
            if artifact.get("platform") == sys.argv[2] and (best is None or key(release["version"]) > key(best[0])):
                best = (release["version"], artifact["url"], artifact["sha256"])
if best:
    print(" ".join(best))
PY
)" || fail "cannot read the release manifest"
    [ -n "$FOUND" ] || fail "no Zen Toolbox release for $PLATFORM yet"
    set -- $FOUND
    VERSION="$1"; URL="$2"; EXPECTED="$3"

    say "Downloading Zen Toolbox ${VERSION}..."
    curl -fSL --progress-bar "$RELEASES/releases/$URL" -o "$WORK/toolbox.tar.gz" || fail "cannot download Zen Toolbox"
    ACTUAL="$(sha256sum "$WORK/toolbox.tar.gz" | awk '{print $1}')"
    [ "$EXPECTED" = "$ACTUAL" ] || fail "the download does not match its checksum"

    DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
    DEST="${ZEN_INSTALL_DIR:-$DATA/zen-toolbox/app}"
    BIN="$HOME/.local/bin"
    # Quit only the copy being replaced.
    if pgrep -f "$DEST/bin/zen-toolbox" >/dev/null 2>&1; then
        say "Quitting the running Zen Toolbox..."
        pkill -f "$DEST/bin/zen-toolbox" || true
        sleep 1
    fi
    rm -rf "$DEST/bin"
    mkdir -p "$DEST" "$BIN" "$DATA/applications" "$DATA/icons/hicolor/scalable/apps"
    tar -xzf "$WORK/toolbox.tar.gz" -C "$DEST" || fail "cannot unpack Zen Toolbox"
    chmod 755 "$DEST/bin/zen-toolbox"
    ln -sf "$DEST/bin/zen-toolbox" "$BIN/zen-toolbox"
    curl -fsSL "$RELEASES/icons/products/toolbox.svg" -o "$DATA/icons/hicolor/scalable/apps/zen-toolbox.svg" || true
    cat > "$DATA/applications/zen-toolbox.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Zen Toolbox
Comment=Installs, updates and launches the Zen IDEs
Exec=$DEST/bin/zen-toolbox
Icon=zen-toolbox
Categories=Development;
Terminal=false
DESKTOP

    say "Zen Toolbox ${VERSION} is installed in $DEST."
    case ":$PATH:" in
        *":$BIN:"*) ;;
        *) say "Add $BIN to PATH to run zen-toolbox from a terminal." ;;
    esac
    if [ "${ZEN_NO_OPEN:-0}" != "1" ] && { [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; }; then
        nohup "$DEST/bin/zen-toolbox" >/dev/null 2>&1 &
        say "It lives in the tray, or opens as a window where there is no tray."
    fi
}

case "$(uname -s)" in
    Darwin) ;;
    Linux) install_linux; exit 0 ;;
    *) fail "only macOS and Linux are supported by this script; on Windows, download the zip from $RELEASES" ;;
esac
case "$(uname -m)" in
    arm64) PLATFORM="macos-aarch64" ;;
    *) fail "only Apple Silicon Macs are supported for now" ;;
esac

WORK="$(mktemp -d -t zen-toolbox-install)"
trap 'rm -rf "$WORK"' EXIT INT TERM

say "Checking the latest Zen Toolbox..."
fetch_manifest "$WORK"
# Ed25519 with JavaScript for Automation, which every Mac has; the hash comes from shasum.
CHALLENGE="$({ printf '%s%s' "$(printf '%s' "$SIGNATURE" | cut -c1-64)" "$PUBLIC_KEY" | xxd -r -p; cat "$WORK/manifest.json"; } | shasum -a 512 | awk '{print $1}')"
osascript -l JavaScript - "$SIGNATURE" "$PUBLIC_KEY" "$CHALLENGE" <<'VERIFY_JS' >/dev/null 2>&1 || fail "the release manifest's signature does not verify: nothing was installed"
function run(argv) {
    const P = 2n ** 255n - 19n;
    const Q = 2n ** 252n + 27742317777372353535851937790883648493n;
    const mod = n => ((n % P) + P) % P;
    const pow = (base, exponent) => {
        let total = 1n;
        base = mod(base);
        while (exponent > 0n) {
            if (exponent & 1n) total = total * base % P;
            base = base * base % P;
            exponent >>= 1n;
        }
        return total;
    };
    const inverse = n => pow(n, P - 2n);
    const D = mod(-121665n * inverse(121666n));
    const ROOT = pow(2n, (P - 1n) / 4n);
    const little = hex => BigInt('0x' + (hex.match(/../g) || []).reverse().join(''));
    const add = (a, b) => {
        const e = mod((a[1] - a[0]) * (b[1] - b[0])), f = mod((a[1] + a[0]) * (b[1] + b[0]));
        const g = mod(2n * a[3] * b[3] * D), h = mod(2n * a[2] * b[2]);
        const i = f - e, j = h - g, k = h + g, l = f + e;
        return [mod(i * j), mod(k * l), mod(j * k), mod(i * l)];
    };
    const times = (n, point) => {
        let total = [0n, 1n, 1n, 0n];
        while (n > 0n) {
            if (n & 1n) total = add(total, point);
            point = add(point, point);
            n >>= 1n;
        }
        return total;
    };
    const decode = hex => {
        const raw = little(hex), sign = raw >> 255n, y = raw & (2n ** 255n - 1n);
        if (y >= P) return null;
        const square = mod((y * y - 1n) * inverse(D * y * y + 1n));
        let x = pow(square, (P + 3n) / 8n);
        if (mod(x * x - square) !== 0n) x = x * ROOT % P;
        if (mod(x * x - square) !== 0n || (x === 0n && sign === 1n)) return null;
        if ((x & 1n) !== sign) x = P - x;
        return [x, y, 1n, x * y % P];
    };
    const [signature, publicKey, challenge] = argv;
    const r = decode(signature.slice(0, 64)), key = decode(publicKey), s = little(signature.slice(64));
    if (r === null || key === null || s >= Q) throw new Error('malformed');
    let y = 4n * inverse(5n) % P, x = mod(y * y - 1n) * inverse(D * y * y + 1n) % P, root = pow(x, (P + 3n) / 8n);
    if (mod(root * root - x) !== 0n) root = root * ROOT % P;
    if (root & 1n) root = P - root;
    const left = times(s, [root, y, 1n, root * y % P]), right = add(r, times(little(challenge) % Q, key));
    if (mod(left[0] * right[2] - right[0] * left[2]) !== 0n || mod(left[1] * right[2] - right[1] * left[2]) !== 0n) {
        throw new Error('the signature does not verify');
    }
    return 'ok';
}
VERIFY_JS

# Pick the newest stable toolbox release for this platform with JavaScript for Automation, which
# every Mac has (no Python or jq needed): its version, archive path and SHA-256.
FOUND="$(osascript -l JavaScript - "$WORK/manifest.json" "$PLATFORM" <<'JS'
function run(argv) {
    const text = $.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null).js;
    const manifest = JSON.parse(text);
    const parse = v => v.split('-')[0].split('.').map(n => parseInt(n, 10) || 0);
    const newer = (a, b) => { const x = parse(a), y = parse(b); for (let i = 0; i < 3; i++) { if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0); } return false; };
    let best = '', found = '';
    for (const product of manifest.products || []) {
        if (product.id !== 'toolbox') continue;
        for (const release of product.releases || []) {
            if (release.channel !== 'stable') continue;
            const artifact = (release.artifacts || []).find(a => a.platform === argv[1]);
            if (!artifact) continue;
            if (best === '' || newer(release.version, best)) {
                best = release.version;
                found = [best, artifact.url, artifact.sha256].join(' ');
            }
        }
    }
    return found;
}
JS
)" || fail "cannot read the release manifest"
[ -n "$FOUND" ] || fail "no Zen Toolbox release for $PLATFORM yet"
VERSION="$(printf '%s' "$FOUND" | cut -d' ' -f1)"
URL="$(printf '%s' "$FOUND" | cut -d' ' -f2)"
EXPECTED="$(printf '%s' "$FOUND" | cut -d' ' -f3)"
say "Downloading Zen Toolbox ${VERSION}..."
curl -fSL --progress-bar "$RELEASES/releases/$URL" -o "$WORK/toolbox.tar.gz" || fail "cannot download Zen Toolbox"
ACTUAL="$(shasum -a 256 "$WORK/toolbox.tar.gz" | awk '{print $1}')"
[ -n "$EXPECTED" ] && [ "$EXPECTED" = "$ACTUAL" ] || fail "the download does not match the signed manifest: nothing was installed"
mkdir "$WORK/unpacked"
tar -xzf "$WORK/toolbox.tar.gz" -C "$WORK/unpacked" "$APP" || fail "cannot unpack Zen Toolbox"
[ -d "$WORK/unpacked/$APP" ] || fail "the archive has no $APP"

if [ -n "${ZEN_INSTALL_DIR:-}" ]; then
    DEST="$ZEN_INSTALL_DIR"
elif [ -w /Applications ]; then
    DEST="/Applications"
else
    DEST="$HOME/Applications"
fi
mkdir -p "$DEST"

# Quit only the copy being replaced, so that a running development build is left alone.
RUNNING="$DEST/$APP/Contents/MacOS/zen-toolbox"
if pgrep -qf "$RUNNING"; then
    say "Quitting the running Zen Toolbox..."
    pkill -f "$RUNNING" || true
    sleep 1
fi
rm -rf "$DEST/$APP"
ditto "$WORK/unpacked/$APP" "$DEST/$APP" || fail "cannot copy the app to $DEST"

say "Zen Toolbox ${VERSION} is installed in $DEST."
if [ "${ZEN_NO_OPEN:-0}" != "1" ]; then
    open "$DEST/$APP"
    say "It lives in the menu bar: look for the ensō icon."
fi
