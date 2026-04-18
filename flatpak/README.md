# Flatpak Build Guide

This directory contains the Flatpak manifest (`com.ykwbridge.desktop`) for YKW Bridge, used to convert the generated `.deb` artifact into an installable `.flatpak` package via CI or local builds.

## Dependencies

- `flatpak`
- `flatpak-builder`
- Flathub remote (for installing `org.gnome.Platform//46` runtime)

For Ubuntu/Debian:

```bash
sudo apt install flatpak flatpak-builder
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y --user flathub org.gnome.Platform//46 org.gnome.Sdk//46
```

## Local Build (Generate .flatpak from .deb)

1) Build the deb on Linux first:

```bash
pnpm tauri build -- --bundles deb
```

2) Copy the generated deb to this directory:

```bash
cp "$(find src-tauri/target/release/bundle -name '*.deb' | head -n 1)" flatpak/ykw-bridge.deb
```

3) Build the local Flatpak repository and export the `.flatpak`:

```bash
flatpak-builder --force-clean --user --disable-cache --repo flatpak-repo flatpak-build flatpak/com.ykwbridge.desktop.yml
flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo flatpak-repo YKW-Bridge-Linux.flatpak com.ykwbridge.desktop
```

4) Install and run:

```bash
flatpak install --user ./YKW-Bridge-Linux.flatpak
flatpak run com.ykwbridge.desktop
```

## Permissions Note

The current manifest uses `--filesystem=home` by default for "download and run" convenience, allowing the app to directly read/write YKW Bridge and Claude workflow configuration files on the host (and supporting the "directory override" feature).

If you prefer minimal permissions (e.g., for Flathub submission or security concerns), you can replace `--filesystem=home` in `flatpak/com.ykwbridge.desktop.yml` with more precise grants:

```yaml
  - --filesystem=~/.ykw-bridge:create
  - --filesystem=~/.claude:create
  - --filesystem=~/.claude.json
```

Note: Flatpak's `:create` modifier only works with directories, not files. Therefore, `~/.claude.json` cannot use `:create`. If this file doesn't exist on the user's machine, the app may not be able to create it with restricted permissions. Users should either run Claude Code once to generate it, or manually create an empty JSON file (content: `{}`). If you override data directories in the app, grant those custom paths explicitly as well.

If you plan to publish on Flathub or want stricter permission control, adjust the `finish-args` in `flatpak/com.ykwbridge.desktop.yml` accordingly.
