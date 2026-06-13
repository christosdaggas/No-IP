Name:           noip-duc
Version:        3.3.0
Release:        1%{?dist}
Summary:        No-IP Dynamic DNS Update Client
License:        Apache-2.0
URL:            https://www.noip.com
Source0:        %{name}-%{version}.tar.gz

# Build deps
BuildRequires:  rust >= 1.76
BuildRequires:  cargo
BuildRequires:  pkgconfig(dbus-1)
BuildRequires:  pkgconfig(gtk+-3.0)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  systemd-rpm-macros
# OpenSSL is not required (rustls), but reqwest's blocking client pulls in
# rustls-platform-verifier which can use system roots — keep cert deps clean.
BuildRequires:  ca-certificates

# Runtime deps
Requires:       systemd
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd
# The GUI uses freedesktop Secret Service (GNOME Keyring or KWallet) for
# password storage. Recommend (not require) so headless installs of just
# the CLI daemon stay lean.
Recommends:     gnome-keyring or kwallet5

%description
No-IP Dynamic Update Client (DUC) keeps your dynamic IP address synchronized
with No-IP's DNS servers. Ships two binaries:

  * noip-duc      – the CLI / systemd-managed daemon
  * noip-duc-gui  – an optional egui-based desktop GUI

The systemd unit runs as a transient unprivileged user (DynamicUser=yes)
inside a tight sandbox; the GUI stores the No-IP password in the platform
keyring via the freedesktop Secret Service API, never on disk in plaintext.

%prep
%autosetup -n %{name}-%{version}

%build
# `--locked` keeps Cargo.lock authoritative; `--features gui` enables the
# desktop binary plus the keyring backend.
cargo build --release --locked --features gui

%install
install -d %{buildroot}%{_bindir}
install -m 0755 target/release/noip-duc      %{buildroot}%{_bindir}/noip-duc
install -m 0755 target/release/noip-duc-gui  %{buildroot}%{_bindir}/noip-duc-gui

# systemd unit + default sysconfig stub
install -d %{buildroot}%{_unitdir}
install -m 0644 noip-duc.service %{buildroot}%{_unitdir}/noip-duc.service

install -d %{buildroot}%{_sysconfdir}/sysconfig
cat > %{buildroot}%{_sysconfdir}/sysconfig/noip-duc <<'EOF'
# /etc/sysconfig/noip-duc — environment for the systemd-managed daemon.
# Set permissions to 0600 (root:root) since this contains a password.
#
# NOIP_USERNAME=
# NOIP_PASSWORD=
# NOIP_HOSTNAMES=host1.ddns.net,host2.ddns.net
#
# NOIP_CHECK_INTERVAL=5m
# NOIP_HTTP_TIMEOUT=10s
# NOIP_IP_METHOD=dns,http,http-port-8245
# NOIP_LOG_LEVEL=info
EOF
chmod 0600 %{buildroot}%{_sysconfdir}/sysconfig/noip-duc

# Desktop integration (GUI)
install -d %{buildroot}%{_datadir}/applications
install -m 0644 data/com.noip.DUC.desktop \
    %{buildroot}%{_datadir}/applications/com.noip.DUC.desktop

install -d %{buildroot}%{_metainfodir}
install -m 0644 data/com.noip.DUC.metainfo.xml \
    %{buildroot}%{_metainfodir}/com.noip.DUC.metainfo.xml

# Icons (hicolor theme)
for sz in 16 32 48 64 128 256 512; do
    install -d %{buildroot}%{_datadir}/icons/hicolor/${sz}x${sz}/apps
    install -m 0644 data/icons/${sz}.png \
        %{buildroot}%{_datadir}/icons/hicolor/${sz}x${sz}/apps/com.noip.DUC.png
done
install -d %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -m 0644 data/icons/com.noip.DUC.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.noip.DUC.svg

# Docs
install -d %{buildroot}%{_docdir}/%{name}
install -m 0644 README.md   %{buildroot}%{_docdir}/%{name}/README.md
install -m 0644 INSTALL.md  %{buildroot}%{_docdir}/%{name}/INSTALL.md
install -m 0644 CHANGELOG.md %{buildroot}%{_docdir}/%{name}/CHANGELOG.md

%files
%license LICENSE
%doc README.md INSTALL.md CHANGELOG.md
%{_bindir}/noip-duc
%{_bindir}/noip-duc-gui
%{_unitdir}/noip-duc.service
%config(noreplace) %attr(0600, root, root) %{_sysconfdir}/sysconfig/noip-duc
%{_datadir}/applications/com.noip.DUC.desktop
%{_metainfodir}/com.noip.DUC.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/com.noip.DUC.*

%post
%systemd_post noip-duc.service

%preun
%systemd_preun noip-duc.service

%postun
%systemd_postun_with_restart noip-duc.service

# Refresh icon cache and AppStream metadata when GUI components change.
%posttrans
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f %{_datadir}/icons/hicolor || :
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q %{_datadir}/applications || :
fi

%changelog
* Thu Apr 30 2026 No-IP Team <support@noip.com> - 3.3.0-1
- clap 4 + hickory-resolver upgrade
- Hardened systemd unit (DynamicUser, sandbox, syscall filter)
- Keyring-backed credential storage in the GUI (Secret Service / Keychain)
- Redacted password from CLI Debug output
- 14 new unit tests covering update API response codes
