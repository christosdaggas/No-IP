No-IP Linux DUC 3
=================

`noip-duc` is a Linux Dynamic Update Client for No-IP. It can run as a CLI tool, a background service, or a small GUI app.

Features
========

- Updates one or more No-IP hostnames or groups
- Supports CLI and GUI binaries
- Reads configuration from flags or environment variables
- Can run once or continuously
- Supports custom public IP detection methods

Install
=======

For full installation steps, packages, and service setup, see [INSTALL.md](INSTALL.md).

To build from source:

```bash
cargo build --release
```

The compiled binaries will be:

- `target/release/noip-duc`
- `target/release/noip-duc-gui`

Quick Start
===========

Run a one-time update:

```bash
noip-duc --username YOUR_USERNAME --password YOUR_PASSWORD --hostnames example.ddns.net --once
```

Run continuously with environment variables:

```bash
export NOIP_USERNAME=YOUR_USERNAME
export NOIP_PASSWORD=YOUR_PASSWORD
export NOIP_HOSTNAMES=example.ddns.net

noip-duc
```

Configuration
=============

The most common settings are:

```bash
NOIP_USERNAME=
NOIP_PASSWORD=
NOIP_HOSTNAMES=
NOIP_CHECK_INTERVAL=5m
```

You can place these in an environment file for `systemd` or another service manager. More complete examples are in [INSTALL.md](INSTALL.md).

Useful Commands
===============

Show help:

```bash
noip-duc --help
```

Open the GUI:

```bash
noip-duc-gui
```

Use a fixed IP once:

```bash
noip-duc --ip-method static:192.168.1.1 --once
```

Use a custom IP lookup URL:

```bash
noip-duc --ip-method https://myip.dnsomatic.com
```

Migration
=========

To import settings from an old `noip2` config:

```bash
noip-duc --import /usr/local/etc/no-ip2.conf
```
