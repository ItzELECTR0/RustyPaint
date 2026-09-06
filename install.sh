#!/bin/sh
# curl -fsSL https://raw.githubusercontent.com/ItzELECTR0/RustyPaint/main/install.sh | sh
set -eu

REPO=ItzELECTR0/RustyPaint
APP_ID=net.electris.RustyPaint
BIN=rustypaint

CHANNEL=${RUSTYPAINT_CHANNEL:-}
FORMAT=${RUSTYPAINT_FORMAT:-}
ASSUME_YES=${RUSTYPAINT_YES:-0}
GITHUB_TOKEN=${GITHUB_TOKEN:-}
DRY_RUN=0
TTY=0
STAGE=
TMP=
ASSETS=
TAG=
VERSION=
INSTALLED_AT=
UNINSTALL=

usage() {
    cat <<'EOF'
RustyPaint installer.

    curl -fsSL https://raw.githubusercontent.com/ItzELECTR0/RustyPaint/main/install.sh | sh

Options need "-s --" when the script is piped in, as in "| sh -s -- --stable".

    --stable            The latest tagged release
    --experimental      The rolling build from the tip of main
    --format FORMAT     appimage, flatpak, deb, rpm, arch, apk, dmg, msi or nsis
    --yes               Ask nothing, take the detected answers, install straight away
    --noconfirm         The same thing, spelled the way pacman spells it
    --dry-run           Say what would be installed and stop
    --help              This

Environment:

    RUSTYPAINT_CHANNEL  stable or experimental
    RUSTYPAINT_FORMAT   the same values as --format
    RUSTYPAINT_YES      1 means --yes
    GITHUB_TOKEN        raises the GitHub API rate limit
EOF
}

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_BOLD=$(printf '\033[1m'); C_DIM=$(printf '\033[2m')
    C_RED=$(printf '\033[31m'); C_YEL=$(printf '\033[33m'); C_OFF=$(printf '\033[0m')
else
    C_BOLD=; C_DIM=; C_RED=; C_YEL=; C_OFF=
fi

say()  { printf '%s\n' "$*"; }
step() { printf '%s==>%s %s\n' "$C_BOLD" "$C_OFF" "$*"; }
note() { printf '%s%s%s\n' "$C_DIM" "$*" "$C_OFF"; }
warn() { printf '%swarning:%s %s\n' "$C_YEL" "$C_OFF" "$*" >&2; }
die()  { STAGE=; printf '%serror:%s %s\n' "$C_RED" "$C_OFF" "$*" >&2; exit 1; }
has()  { command -v "$1" >/dev/null 2>&1; }

cleanup() {
    status=$?
    trap - EXIT
    if [ -n "$TMP" ]; then rm -rf "$TMP"; fi
    if [ "$status" -ne 0 ] && [ "$STAGE" = install ]; then
        printf '%serror:%s The %s install stopped. What it printed above says why.\n' \
            "$C_RED" "$C_OFF" "$FORMAT" >&2
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

parse_args() {
    while [ $# -gt 0 ]; do
        case $1 in
            --stable) CHANNEL=stable ;;
            --experimental|--exp) CHANNEL=experimental ;;
            --format) shift; [ $# -gt 0 ] || die "--format needs a value."; FORMAT=$1 ;;
            --format=*) FORMAT=${1#--format=} ;;
            --channel) shift; [ $# -gt 0 ] || die "--channel needs a value."; CHANNEL=$1 ;;
            --channel=*) CHANNEL=${1#--channel=} ;;
            -y|--yes|--noconfirm) ASSUME_YES=1 ;;
            -n|--dry-run) DRY_RUN=1 ;;
            -h|--help) usage; exit 0 ;;
            *) die "Unknown option: $1 (try --help)" ;;
        esac
        shift
    done

    case $CHANNEL in ''|stable|experimental) ;; *) die "Unknown channel: $CHANNEL" ;; esac
    case $FORMAT in ''|appimage|flatpak|deb|rpm|arch|apk|dmg|msi|nsis) ;; *) die "Unknown format: $FORMAT" ;; esac
}

# The script itself is on stdin under "curl | sh", so questions go to the terminal directly.
open_tty() {
    if [ "$ASSUME_YES" = 1 ]; then return 0; fi
    # A failed exec redirection kills a non-interactive shell, so try it somewhere disposable.
    if ( exec 3</dev/tty && exec 4>/dev/tty ) 2>/dev/null; then
        exec 3</dev/tty
        exec 4>/dev/tty
        TTY=1
    fi
}

# ask <prompt> <default> -> REPLY
ask() {
    REPLY=$2
    if [ "$TTY" != 1 ]; then return 0; fi
    printf '%s ' "$1" >&4
    if IFS= read -r line <&3; then
        if [ -n "$line" ]; then REPLY=$line; fi
    else
        printf '\n' >&4
    fi
}

http_get() {
    if has curl; then
        if [ -n "$GITHUB_TOKEN" ]; then
            curl -fsSL --retry 2 -H "Authorization: Bearer $GITHUB_TOKEN" "$1"
        else
            curl -fsSL --retry 2 "$1"
        fi
    else
        if [ -n "$GITHUB_TOKEN" ]; then
            wget -q -O - --header "Authorization: Bearer $GITHUB_TOKEN" "$1"
        else
            wget -q -O - "$1"
        fi
    fi
}

http_download() {
    if has curl; then
        curl -fL --retry 2 --progress-bar -o "$2" "$1"
    elif wget --help 2>&1 | grep -q -- --show-progress; then
        wget -q --show-progress -O "$2" "$1"
    else
        wget -O "$2" "$1"
    fi
}

read_os_release() {
    # shellcheck disable=SC1091
    if [ -r /etc/os-release ]; then
        DISTRO=$(. /etc/os-release 2>/dev/null; printf '%s' "${ID:-}")
        DISTRO_LIKE=$(. /etc/os-release 2>/dev/null; printf '%s' "${ID_LIKE:-}")
        DISTRO_NAME=$(. /etc/os-release 2>/dev/null; printf '%s' "${PRETTY_NAME:-${NAME:-}}")
    fi
}

detect_platform() {
    if ! has curl && ! has wget; then die "This needs curl or wget."; fi

    case $(uname -s) in
        Linux)  OS=linux ;;
        Darwin) OS=macos ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT) OS=windows ;;
        *) die "Unsupported system: $(uname -s). RustyPaint ships for Linux, macOS and Windows." ;;
    esac

    case $(uname -m) in
        x86_64|amd64) ARCH=x86_64; DEB_ARCH=amd64; MAC_ARCH=x64; WIN_ARCH=x64 ;;
        aarch64|arm64) ARCH=aarch64; DEB_ARCH=arm64; MAC_ARCH=aarch64; WIN_ARCH=arm64 ;;
        *) die "Unsupported processor: $(uname -m). Builds exist for x86_64 and aarch64." ;;
    esac

    DISTRO=; DISTRO_LIKE=; DISTRO_NAME=
    LIBC=glibc
    IMMUTABLE=0

    if [ "$OS" = linux ]; then
        read_os_release

        if [ -n "$(find /lib /lib64 /usr/lib -maxdepth 1 -name 'ld-musl-*' 2>/dev/null | head -1)" ]; then
            LIBC=musl
        elif has ldd && ldd --version 2>&1 | head -1 | grep -qi musl; then
            LIBC=musl
        fi

        if [ -f /run/ostree-booted ]; then IMMUTABLE=1; fi
        case $DISTRO in steamos|nixos) IMMUTABLE=1 ;; esac
    fi

    if has doas; then ESCALATE=doas
    elif has sudo; then ESCALATE=sudo
    else ESCALATE=
    fi

    if [ "$(id -u)" -eq 0 ]; then
        ROOT=1
        SUDO=
    else
        ROOT=0
        SUDO=$ESCALATE
    fi
}

# What to tell someone to run later, which is not what this run needs if it is already root.
root_hint() {
    if [ -n "$ESCALATE" ]; then
        printf '%s %s\n' "$ESCALATE" "$*"
    else
        printf '%s\n' "$*"
    fi
}

run_root() {
    if [ "$ROOT" = 1 ]; then
        "$@"
    elif [ -n "$SUDO" ]; then
        note "Running: $SUDO $*"
        $SUDO "$@"
    else
        die "This install needs root and neither doas nor sudo is here. Re-run as root, or use --format appimage."
    fi
}

choose_channel() {
    if [ -n "$CHANNEL" ]; then return 0; fi
    if [ "$TTY" != 1 ]; then CHANNEL=stable; return 0; fi

    printf '\n%sWhich release do you want?%s\n' "$C_BOLD" "$C_OFF" >&4
    printf '  1) Stable        the latest tagged release\n' >&4
    printf '  2) Experimental  built from the tip of main, replaced on every push, can be broken\n' >&4
    while :; do
        ask "Choice [1]:" 1
        case $REPLY in
            1|s|S|stable) CHANNEL=stable; break ;;
            2|e|E|exp|experimental) CHANNEL=experimental; break ;;
            *) printf 'Answer 1 or 2.\n' >&4 ;;
        esac
    done
    printf '\n' >&4
}

# The API is the good path. This is the one that still works once it rate limits.
scrape_release() {
    page=https://github.com/$REPO/releases/latest
    if [ "$CHANNEL" = experimental ]; then page=https://github.com/$REPO/releases/tag/experimental; fi

    frag=$(http_get "$page" 2>/dev/null | grep -o 'expanded_assets/[^"]*' | head -1) || frag=
    if [ -z "$frag" ]; then return 0; fi

    http_get "https://github.com/$REPO/releases/$frag" 2>/dev/null \
        | grep -o "/$REPO/releases/download/[^\"]*" \
        | sed 's|^|https://github.com|' || true
}

resolve_release() {
    step "Looking up the $CHANNEL release"

    path=releases/latest
    if [ "$CHANNEL" = experimental ]; then path=releases/tags/experimental; fi

    ASSETS=$(http_get "https://api.github.com/repos/$REPO/$path" 2>/dev/null \
        | grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*"' \
        | sed 's|.*"\(https[^"]*\)".*|\1|') || ASSETS=

    if [ -z "$ASSETS" ]; then ASSETS=$(scrape_release); fi
    if [ -z "$ASSETS" ]; then
        die "Could not read the $CHANNEL release from GitHub. Check the network, or take a package from https://github.com/$REPO/releases"
    fi

    TAG=$(printf '%s\n' "$ASSETS" | sed -n '1s|.*/download/\([^/]*\)/.*|\1|p')
    VERSION=$(printf '%s\n' "$ASSETS" | sed -n 's|.*/rustypaint_\([0-9][^_]*\)_.*|\1|p' | head -1)
    if [ "$CHANNEL" = experimental ]; then
        say "Found the experimental build, packaged as ${VERSION:-an untagged version}."
    else
        say "Found ${TAG:-the latest release}."
    fi
}

asset_pattern() {
    case $1 in
        appimage) printf '%s\n' "_${ARCH}[.]AppImage$" ;;
        flatpak)  printf '%s\n' "-${ARCH}[.]flatpak$" ;;
        deb)      printf '%s\n' "_${DEB_ARCH}[.]deb$" ;;
        rpm)      printf '%s\n' "[.]${ARCH}[.]rpm$" ;;
        arch)     printf '%s\n' "-${ARCH}[.]pkg[.]tar[.]zst$" ;;
        apk)      printf '%s\n' "-${ARCH}[.]apk$" ;;
        dmg)      printf '%s\n' "_${MAC_ARCH}[.]dmg$" ;;
        msi)      printf '%s\n' "_${WIN_ARCH}_[^/]*[.]msi$" ;;
        nsis)     printf '%s\n' "_${WIN_ARCH}-setup[.]exe$" ;;
    esac
}

asset_url() {
    printf '%s\n' "$ASSETS" | grep -E -- "$(asset_pattern "$1")" | head -1 || true
}

have_asset() {
    [ -n "$(asset_url "$1")" ]
}

format_label() {
    case $1 in
        appimage) printf '%s\n' "AppImage, one file that runs almost anywhere" ;;
        flatpak)  printf '%s\n' "Flatpak bundle, sandboxed, needs flatpak installed" ;;
        deb)      printf '%s\n' "Debian package, for apt systems" ;;
        rpm)      printf '%s\n' "RPM package, for dnf and zypper systems" ;;
        arch)     printf '%s\n' "Arch package, for pacman systems" ;;
        apk)      printf '%s\n' "Alpine package, for apk systems" ;;
        dmg)      printf '%s\n' "macOS disk image" ;;
        msi)      printf '%s\n' "Windows MSI installer" ;;
        nsis)     printf '%s\n' "Windows setup program" ;;
    esac
}

# Sets GUESS, plus SURE=1 when the answer is settled enough not to ask about.
guess_format() {
    GUESS=
    SURE=0

    case $OS in
        macos) GUESS=dmg; SURE=1; return 0 ;;
        windows)
            # The MSI is the one to take where it exists, which is x86_64 only.
            if have_asset msi; then GUESS=msi; else GUESS=nsis; fi
            SURE=1
            return 0 ;;
    esac

    if [ "$IMMUTABLE" = 1 ]; then
        GUESS=flatpak; SURE=1
        WHY="This system keeps its base read-only, so applications belong in Flatpak."
        return 0
    fi
    if [ "$LIBC" = musl ]; then
        if has apk && have_asset apk; then
            GUESS=apk; SURE=1
            WHY="This system runs musl, and the Alpine package is the build linked against it."
        else
            GUESS=flatpak; SURE=1
            WHY="This system runs musl and every other build is linked against glibc."
        fi
        return 0
    fi

    case $DISTRO in
        debian|ubuntu|linuxmint|pop|elementary|zorin|neon|kali|raspbian|raspios|devuan|mx|antix|deepin|parrot|trisquel|tuxedo|pureos|siduction|sparky)
            GUESS=deb; SURE=1 ;;
        fedora|rhel|centos|rocky|almalinux|ol|oracle|amzn|mageia|openmandriva|nobara|ultramarine|eurolinux|navylinux|scientific|risios|azurelinux|mariner)
            GUESS=rpm; SURE=1 ;;
        opensuse|opensuse-leap|opensuse-tumbleweed|opensuse-slowroll|opensuse-microos|sles|sled|suse)
            GUESS=rpm; SURE=1 ;;
        arch|manjaro|endeavouros|cachyos|garuda|artix|arcolinux|archcraft|parabola|archbang|blackarch|obarun)
            GUESS=arch; SURE=1 ;;
    esac
    if [ -n "$GUESS" ]; then return 0; fi

    for like in $DISTRO_LIKE; do
        case $like in
            debian|ubuntu) GUESS=deb; SURE=1; return 0 ;;
            fedora|rhel|centos|suse|opensuse|mandriva) GUESS=rpm; SURE=1; return 0 ;;
            arch|archlinux) GUESS=arch; SURE=1; return 0 ;;
        esac
    done

    # Nothing named this system, so a package manager on PATH is a hint and not an answer.
    if has pacman; then GUESS=arch
    elif has apt-get || has dpkg; then GUESS=deb
    elif has dnf5 || has dnf || has zypper || has rpm; then GUESS=rpm
    fi
    return 0
}

available_formats() {
    out=
    for candidate in appimage flatpak deb rpm arch apk dmg msi nsis; do
        case $OS in
            linux)   case $candidate in dmg|msi|nsis) continue ;; esac ;;
            macos)   if [ "$candidate" != dmg ]; then continue; fi ;;
            windows) case $candidate in msi|nsis) ;; *) continue ;; esac ;;
        esac
        if have_asset "$candidate"; then out="$out $candidate"; fi
    done
    printf '%s\n' "$out"
}

ask_format() {
    printf '\n%sNot sure which package %s wants.%s\n' "$C_BOLD" "${DISTRO_NAME:-this system}" "$C_OFF" >&4
    printf 'Take the first one if you do not know.\n' >&4

    i=0
    default=1
    for candidate in $AVAILABLE; do
        i=$((i + 1))
        if [ "$candidate" = appimage ]; then default=$i; fi
        mark=
        if [ "$candidate" = "$GUESS" ]; then mark="  <- looks like this system"; fi
        printf '  %d) %s%s\n' "$i" "$(format_label "$candidate")" "$mark" >&4
    done

    while :; do
        ask "Choice [$default]:" "$default"
        picked=
        case $REPLY in
            ''|*[!0-9]*)
                for candidate in $AVAILABLE; do
                    if [ "$REPLY" = "$candidate" ]; then picked=$candidate; fi
                done ;;
            *)
                if [ "$REPLY" -ge 1 ] && [ "$REPLY" -le "$i" ]; then
                    j=0
                    for candidate in $AVAILABLE; do
                        j=$((j + 1))
                        if [ "$j" = "$REPLY" ]; then picked=$candidate; fi
                    done
                fi ;;
        esac
        if [ -n "$picked" ]; then FORMAT=$picked; break; fi
        printf 'Answer with a number from 1 to %d, or the name.\n' "$i" >&4
    done
    printf '\n' >&4
}

choose_format() {
    WHY=
    guess_format

    AVAILABLE=$(available_formats)
    if [ -z "$AVAILABLE" ]; then die "The $CHANNEL release has nothing for $OS $ARCH."; fi

    if [ -n "$FORMAT" ]; then
        case " $AVAILABLE " in
            *" $FORMAT "*) ;;
            *)
                if have_asset "$FORMAT"; then die "The $FORMAT build is not for $OS."; fi
                die "The $CHANNEL release has no $FORMAT build for $ARCH." ;;
        esac
        if [ "$LIBC" = musl ]; then
            case $FORMAT in
                flatpak|apk) ;;
                *) warn "The $FORMAT build is linked against glibc and this system runs musl, so it may not start." ;;
            esac
        fi
        return 0
    fi

    case " $AVAILABLE " in
        *" $GUESS "*) ;;
        *) GUESS=; SURE=0 ;;
    esac

    # shellcheck disable=SC2086
    set -- $AVAILABLE
    if [ $# -eq 1 ]; then
        FORMAT=$1
        say "Only the $FORMAT build exists for $OS $ARCH, so that is the one."
        return 0
    fi

    if [ "$SURE" = 1 ]; then
        FORMAT=$GUESS
        say "Detected ${DISTRO_NAME:-$OS} on $ARCH, taking the $FORMAT build."
        if [ -n "$WHY" ]; then note "$WHY"; fi
        note "Pass --format to override."
        return 0
    fi

    if [ "$TTY" != 1 ]; then
        FORMAT=appimage
        if ! have_asset appimage; then FORMAT=$1; fi
        warn "Could not tell what ${DISTRO_NAME:-this system} wants and there is nobody to ask, so taking the $FORMAT build."
        return 0
    fi

    ask_format
}

fallback_hint() {
    if [ "$FORMAT" != apk ] && [ "$LIBC" = musl ] && has apk && have_asset apk; then
        printf '%s\n' "The apk build is the one for this system, so re-run with --format apk."
    elif [ "$LIBC" = musl ] || [ "$IMMUTABLE" = 1 ]; then
        printf '%s\n' "The other Linux builds will not run here, so flatpak is the way in."
    elif have_asset appimage; then
        printf '%s\n' "Otherwise re-run with --format appimage, which needs nothing installed."
    fi
}

# Refuse before spending a download on something this system cannot install.
preflight() {
    case $FORMAT in
        flatpak)
            if ! has flatpak; then
                die "Flatpak is not installed. Install it and run this again. $(fallback_hint)"
            fi ;;
        deb)
            if ! has apt-get && ! has apt && ! has dpkg; then
                die "No apt or dpkg here, so a Debian package cannot be installed. $(fallback_hint)"
            fi ;;
        rpm)
            if ! has dnf && ! has dnf5 && ! has zypper && ! has yum && ! has rpm; then
                die "No rpm tool here. $(fallback_hint)"
            fi ;;
        arch)
            if ! has pacman; then
                die "No pacman here. $(fallback_hint)"
            fi ;;
        apk)
            if ! has apk; then
                die "No apk here, so an Alpine package cannot be installed. $(fallback_hint)"
            fi ;;
        msi)
            if ! has msiexec && ! has msiexec.exe; then
                die "msiexec is not on PATH, so this cannot run the installer for you."
            fi ;;
    esac

    case $FORMAT in
        appimage|flatpak|dmg|msi|nsis) ;;
        *)
            if [ "$ROOT" != 1 ] && [ -z "$SUDO" ]; then
                die "Installing the $FORMAT build needs root and neither doas nor sudo is here. $(fallback_hint)"
            fi ;;
    esac
}

# pacman asks before it touches anything, and a piped installer that does not is worse than one that
# does, so this is the last chance to walk away.
confirm_install() {
    if [ "$ASSUME_YES" = 1 ] || [ "$TTY" != 1 ]; then return 0; fi

    printf '\n%sReady to install%s\n' "$C_BOLD" "$C_OFF" >&4
    if [ "$CHANNEL" = experimental ]; then
        printf '  Release  experimental%s\n' "${VERSION:+, packaged as $VERSION}" >&4
    else
        printf '  Release  %s\n' "${TAG:-the latest release}" >&4
    fi
    printf '  Package  %s\n' "$(format_label "$FORMAT")" >&4
    printf '  File     %s\n' "$(basename "$(asset_url "$FORMAT")")" >&4
    case $FORMAT in
        appimage|flatpak|dmg|msi|nsis) ;;
        *)
            if [ "$ROOT" = 1 ]; then
                printf '  Needs    root, which you already are\n' >&4
            else
                printf '  Needs    root, through %s\n' "$SUDO" >&4
            fi ;;
    esac

    while :; do
        ask "Install? [Y/n]:" y
        case $REPLY in
            y|Y|yes|Yes|YES) break ;;
            n|N|no|No|NO) printf '\n' >&4; say "Nothing was installed."; exit 0 ;;
            *) printf 'Answer y or n.\n' >&4 ;;
        esac
    done
    printf '\n' >&4
}

verify_checksum() {
    file=$1
    name=$(basename "$file")

    sums=$(printf '%s\n' "$ASSETS" | grep -E '/SHA256SUMS$' | head -1) || sums=
    if [ -z "$sums" ]; then
        warn "This release ships no SHA256SUMS, so the download was not verified."
        return 0
    fi
    if ! http_get "$sums" > "$TMP/SHA256SUMS" 2>/dev/null; then
        warn "Could not fetch SHA256SUMS, so the download was not verified."
        return 0
    fi

    expected=
    while read -r sum listed; do
        listed=${listed#\*}
        if [ "$listed" = "$name" ]; then expected=$sum; break; fi
    done < "$TMP/SHA256SUMS"
    if [ -z "$expected" ]; then
        warn "SHA256SUMS does not list $name, so the download was not verified."
        return 0
    fi

    if has sha256sum; then actual=$(sha256sum "$file" | cut -d' ' -f1)
    elif has shasum; then actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    elif has openssl; then actual=$(openssl dgst -sha256 "$file" | sed 's/.*= *//')
    else
        warn "No sha256 tool here, so the download was not verified."
        return 0
    fi

    if [ "$actual" != "$expected" ]; then
        die "Checksum mismatch on $name. Expected $expected, got $actual. Nothing was installed."
    fi
    say "Checksum matches."
}

fetch_asset() {
    url=$(asset_url "$FORMAT")
    if [ -z "$url" ]; then die "No $FORMAT build in the $CHANNEL release."; fi

    FILE=$TMP/$(basename "$url")
    step "Downloading $(basename "$url")"
    http_download "$url" "$FILE" || die "Download failed: $url"
    verify_checksum "$FILE"
}

# A native package can install cleanly and still not run, when the build is newer than this glibc.
check_runnable() {
    if ! has ldd || [ ! -x "$1" ]; then return 0; fi
    needs=$(ldd "$1" 2>&1 | sed -n "s/.*version \`\\(GLIBC_[0-9.]*\\)' not found.*/\\1/p" | head -1) || needs=
    if [ -n "$needs" ]; then
        warn "It installed, but this system's C library is older than the build wants ($needs), so it will not start. The flatpak build carries its own runtime: re-run with --format flatpak."
    fi
    return 0
}

refresh_desktop_caches() {
    if has update-desktop-database; then update-desktop-database "$1/applications" >/dev/null 2>&1 || true; fi
    if has update-mime-database; then update-mime-database "$1/mime" >/dev/null 2>&1 || true; fi
    if has gtk-update-icon-cache; then gtk-update-icon-cache -qtf "$1/icons/hicolor" >/dev/null 2>&1 || true; fi
    return 0
}

# An AppImage only mounts itself through libfuse2, which plenty of systems no longer ship.
fuse_works() {
    if ! has fusermount && ! has fusermount3; then return 1; fi
    if has ldconfig && ldconfig -p 2>/dev/null | grep -q 'libfuse[.]so[.]2'; then return 0; fi
    for dir in /usr/lib /usr/lib64 /lib /lib64 /usr/lib/x86_64-linux-gnu /usr/lib/aarch64-linux-gnu; do
        if [ -e "$dir/libfuse.so.2" ]; then return 0; fi
    done
    return 1
}

install_appimage() {
    if [ "$ROOT" = 1 ]; then
        app_dir=/opt/rustypaint
        bin_dir=/usr/local/bin
        data_dir=/usr/local/share
    else
        app_dir=${XDG_DATA_HOME:-$HOME/.local/share}/rustypaint
        bin_dir=$HOME/.local/bin
        data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
    fi

    chmod 0755 "$FILE"
    ( cd "$TMP" && "$FILE" --appimage-extract >/dev/null 2>&1 ) || true
    extracted=$TMP/squashfs-root

    rm -rf "$app_dir"
    mkdir -p "$app_dir" "$bin_dir"

    if [ -d "$extracted/usr/share" ] && ! fuse_works; then
        cp -a "$extracted" "$app_dir/AppDir"
        target=$app_dir/AppDir/AppRun
        note "Nothing here provides libfuse2, which an AppImage needs to mount itself, so this unpacked it instead."
    else
        target=$app_dir/RustyPaint.AppImage
        cp "$FILE" "$target"
        chmod 0755 "$target"
    fi
    ln -sf "$target" "$bin_dir/$BIN"

    if [ -d "$extracted/usr/share" ]; then
        mkdir -p "$data_dir/applications" "$data_dir/icons" "$data_dir/metainfo" "$data_dir/mime/packages"

        src=$extracted/usr/share/applications/$APP_ID.desktop
        if [ ! -f "$src" ]; then src=$extracted/$BIN.desktop; fi
        if [ -f "$src" ]; then
            case $target in
                *' '*) exec_line="Exec=\"$target\" %f" ;;
                *)     exec_line="Exec=$target %f" ;;
            esac
            while IFS= read -r line; do
                case $line in
                    Exec=*) printf '%s\n' "$exec_line" ;;
                    TryExec=*) ;;
                    *) printf '%s\n' "$line" ;;
                esac
            done < "$src" > "$data_dir/applications/$APP_ID.desktop"
        fi

        if [ -d "$extracted/usr/share/icons/hicolor" ]; then
            ( cd "$extracted/usr/share/icons" && find hicolor -type f -name '*.png' | while read -r icon; do
                mkdir -p "$data_dir/icons/$(dirname "$icon")"
                cp "$icon" "$data_dir/icons/$icon"
            done )
        fi
        if [ -f "$extracted/usr/share/metainfo/$APP_ID.metainfo.xml" ]; then
            cp "$extracted/usr/share/metainfo/$APP_ID.metainfo.xml" "$data_dir/metainfo/"
        fi
        if [ -f "$extracted/usr/share/mime/packages/$APP_ID.xml" ]; then
            cp "$extracted/usr/share/mime/packages/$APP_ID.xml" "$data_dir/mime/packages/"
        fi

        check_runnable "$extracted/usr/bin/$BIN"
        refresh_desktop_caches "$data_dir"
    else
        warn "Could not unpack the AppImage, so it has no menu entry. The command still works."
    fi

    INSTALLED_AT=$target
    UNINSTALL="rm -rf '$app_dir' '$bin_dir/$BIN' '$data_dir/applications/$APP_ID.desktop'"

    case ":$PATH:" in
        *":$bin_dir:"*) ;;
        *) warn "$bin_dir is not in PATH. Add it, or run $target directly." ;;
    esac
}

install_flatpak() {
    scope=--user
    if [ "$ROOT" = 1 ]; then scope=--system; fi

    # The bundle carries no runtime, so the remote it comes from has to exist first.
    flatpak remote-add --if-not-exists $scope flathub https://dl.flathub.org/repo/flathub.flatpakrepo >/dev/null 2>&1 || true

    if flatpak info $scope "$APP_ID" >/dev/null 2>&1; then
        flatpak install $scope -y --reinstall "$FILE"
    else
        flatpak install $scope -y "$FILE"
    fi

    INSTALLED_AT="flatpak run $APP_ID"
    UNINSTALL="flatpak uninstall $scope $APP_ID"
}

install_deb() {
    if has apt-get; then
        run_root apt-get install -y "$FILE"
        UNINSTALL=$(root_hint apt-get remove "$BIN")
    elif has apt; then
        run_root apt install -y "$FILE"
        UNINSTALL=$(root_hint apt remove "$BIN")
    elif has dpkg; then
        run_root dpkg -i "$FILE"
        UNINSTALL=$(root_hint dpkg -r "$BIN")
    fi
    INSTALLED_AT=/usr/bin/$BIN
    check_runnable "$INSTALLED_AT"
}

install_rpm() {
    if has dnf; then
        run_root dnf install -y "$FILE"; UNINSTALL=$(root_hint dnf remove "$BIN")
    elif has dnf5; then
        run_root dnf5 install -y "$FILE"; UNINSTALL=$(root_hint dnf5 remove "$BIN")
    elif has zypper; then
        run_root zypper --non-interactive install --allow-unsigned-rpm "$FILE"; UNINSTALL=$(root_hint zypper remove "$BIN")
    elif has yum; then
        run_root yum install -y "$FILE"; UNINSTALL=$(root_hint yum remove "$BIN")
    else
        run_root rpm -Uvh "$FILE"; UNINSTALL=$(root_hint rpm -e "$BIN")
    fi
    INSTALLED_AT=/usr/bin/$BIN
    check_runnable "$INSTALLED_AT"
}

install_arch() {
    run_root pacman -U --noconfirm "$FILE"
    INSTALLED_AT=/usr/bin/$BIN
    UNINSTALL=$(root_hint pacman -R "$BIN")
    check_runnable "$INSTALLED_AT"
}

install_apk() {
    # The key that signed it is not one apk knows, so the signature cannot be the thing that vouches
    # for the file. The SHA256SUMS check above already did that.
    run_root apk add --allow-untrusted "$FILE"
    INSTALLED_AT=/usr/bin/$BIN
    UNINSTALL=$(root_hint apk del "$BIN")
}

install_nsis() {
    keep=$HOME/$(basename "$FILE")
    cp "$FILE" "$keep" 2>/dev/null || keep=$FILE
    chmod 0755 "$keep" 2>/dev/null || true

    say "Handing the setup program to Windows. It installs for every user, so expect a prompt."
    if ! "$keep" /S; then
        die "The setup program would not run. It is at $keep, so open that instead."
    fi
    if [ "$keep" != "$FILE" ]; then rm -f "$keep"; fi

    INSTALLED_AT="RustyPaint, in the Start menu"
    UNINSTALL="winget uninstall ItzELECTR0.RustyPaint"
}

install_dmg() {
    mnt=$TMP/mnt
    mkdir -p "$mnt"
    hdiutil attach -nobrowse -readonly -quiet -mountpoint "$mnt" "$FILE" ||
        die "Could not mount $(basename "$FILE")."

    app=$(find "$mnt" -maxdepth 1 -name '*.app' | head -1)
    name=$(basename "$app")
    if [ -z "$app" ] || [ -z "$name" ]; then
        hdiutil detach -quiet "$mnt" || true
        die "No application inside the disk image."
    fi

    dest=/Applications
    if [ ! -w "$dest" ]; then dest=$HOME/Applications; fi
    mkdir -p "$dest"
    rm -rf "${dest:?}/${name:?}"
    if ! cp -R "$app" "$dest/"; then
        hdiutil detach -quiet "$mnt" || true
        die "Could not copy the app into $dest."
    fi
    hdiutil detach -quiet "$mnt" || true

    installed=$dest/$name
    # Unsigned, so Gatekeeper refuses to open it until the download flag is gone.
    xattr -dr com.apple.quarantine "$installed" 2>/dev/null || true

    INSTALLED_AT=$installed
    UNINSTALL="rm -rf '$installed'"
}

install_msi() {
    keep=$HOME/$(basename "$FILE")
    cp "$FILE" "$keep" 2>/dev/null || keep=$FILE

    win=$keep
    if has cygpath; then win=$(cygpath -w "$keep"); fi

    say "Handing the installer to msiexec. Answer whatever Windows asks."
    if ! msiexec -i "$win" -qb; then
        die "msiexec would not run it. The installer is at $keep, so open that instead."
    fi
    rm -f "$keep"

    INSTALLED_AT="RustyPaint, in the Start menu"
    UNINSTALL="winget uninstall ItzELECTR0.RustyPaint"
}

report() {
    printf '\n'
    if [ "$CHANNEL" = experimental ]; then
        step "The experimental RustyPaint build is installed"
    else
        step "RustyPaint${VERSION:+ $VERSION} is installed"
    fi
    case $FORMAT in
        flatpak) say "Run it with: flatpak run $APP_ID" ;;
        dmg)     say "It is at $INSTALLED_AT" ;;
        msi|nsis) say "It is $INSTALLED_AT" ;;
        *)       say "Run it with: $BIN" ;;
    esac
    note "Remove it with: $UNINSTALL"

    if [ "$CHANNEL" = experimental ]; then
        note "This one is built from the tip of main and is replaced on every push."
    fi
    case $FORMAT in
        dmg|msi|nsis) note "This build is not code-signed yet, so the system may warn about it." ;;
        apk)     note "apk called it untrusted because the key that signed it is not one it knows." ;;
    esac
}

main() {
    parse_args "$@"
    open_tty
    detect_platform
    choose_channel
    resolve_release
    choose_format
    preflight

    if [ "$DRY_RUN" = 1 ]; then
        say "Would install the $FORMAT build of ${TAG:-$CHANNEL} on ${DISTRO_NAME:-$OS} $ARCH."
        say "$(asset_url "$FORMAT")"
        exit 0
    fi

    confirm_install

    TMP=$(mktemp -d 2>/dev/null || mktemp -d -t rustypaint)
    fetch_asset

    step "Installing the $FORMAT build"
    STAGE=install
    case $FORMAT in
        appimage) install_appimage ;;
        flatpak)  install_flatpak ;;
        deb)      install_deb ;;
        rpm)      install_rpm ;;
        arch)     install_arch ;;
        apk)      install_apk ;;
        dmg)      install_dmg ;;
        msi)      install_msi ;;
        nsis)     install_nsis ;;
    esac
    STAGE=

    report
}

main "$@"
