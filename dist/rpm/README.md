# RPM packaging

This directory builds a Fedora/RHEL `.rpm` for both the `noip-duc` CLI daemon
and the `noip-duc-gui` desktop client.

## Prerequisites

```sh
sudo dnf install rpm-build rpmdevtools \
                 rust cargo \
                 dbus-devel gtk3-devel libxkbcommon-devel \
                 systemd-rpm-macros pkgconf-pkg-config
```

For a clean chroot build (recommended for distribution):

```sh
sudo dnf install mock
sudo usermod -a -G mock "$USER" && newgrp mock
```

## Build

From the repo root:

```sh
./dist/rpm/build.sh                # SRPM + binary RPM, built on the host
./dist/rpm/build.sh --srpm-only    # only the source RPM
./dist/rpm/build.sh --mock         # SRPM + clean chroot build via mock
```

Outputs land in `dist/rpm/out/`:

```
dist/rpm/out/
├── SRPMS/noip-duc-3.3.0-1.<dist>.src.rpm
└── RPMS/x86_64/noip-duc-3.3.0-1.<dist>.x86_64.rpm
```

## Install

```sh
sudo dnf install ./dist/rpm/out/RPMS/x86_64/noip-duc-3.3.0-*.x86_64.rpm
```

## Configure & start the daemon

The package ships an `EnvironmentFile`-driven systemd unit. Drop credentials
into `/etc/sysconfig/noip-duc` (already created with `0600` perms):

```sh
sudoedit /etc/sysconfig/noip-duc
```

```
NOIP_USERNAME=you@example.com
NOIP_PASSWORD=correct-horse-battery-staple
NOIP_HOSTNAMES=myhost.ddns.net
```

Then:

```sh
sudo systemctl enable --now noip-duc.service
systemctl status noip-duc.service
journalctl -u noip-duc.service -f
```

## What's in the package

| Path                                                   | Purpose                                  |
|--------------------------------------------------------|------------------------------------------|
| `/usr/bin/noip-duc`                                    | CLI / daemon entry point                 |
| `/usr/bin/noip-duc-gui`                                | Desktop GUI entry point                  |
| `/usr/lib/systemd/system/noip-duc.service`             | Hardened systemd unit (DynamicUser, sandboxed) |
| `/etc/sysconfig/noip-duc`                              | Environment file template (0600)         |
| `/usr/share/applications/com.noip.DUC.desktop`         | Desktop launcher                         |
| `/usr/share/metainfo/com.noip.DUC.metainfo.xml`        | AppStream metadata                       |
| `/usr/share/icons/hicolor/{16…512,scalable}/apps/…`    | Icon set                                 |
| `/usr/share/doc/noip-duc/{README,INSTALL,CHANGELOG,LICENSE}.md` | Documentation                  |

## Hardening

The shipped unit runs the daemon under `DynamicUser=yes` (transient kernel UID,
no shell, no home), with `ProtectSystem=strict`, `ProtectHome=yes`,
`NoNewPrivileges=yes`, `RestrictAddressFamilies=AF_INET AF_INET6`,
`MemoryDenyWriteExecute=yes`, syscall filter `@system-service` minus
`@privileged @resources`, plus 64M memory and 10% CPU caps.

Verify after install:

```sh
systemd-analyze security noip-duc.service
```

Exposure level should be in the **OK** band.

## Why no `noip-duc-cli` / `noip-duc-gui` subpackages?

Both binaries share the same `Cargo.toml` and link the same library half, so
splitting buys very little disk and complicates the `Recommends:` chain for
the keyring. If headless installs become a concern, splitting `noip-duc` and
`noip-duc-gui` into separate `%package` blocks is the place to start.
