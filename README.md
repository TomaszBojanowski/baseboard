# Baseboard

A native GNOME app for the sensors and fans of motherboards on Linux.

Website: https://baseboard.bojanowski.me (coming soon)

> **Early version.** Baseboard has finished its first milestone (M1): it only reads. There are no releases or packages yet; build it from source. The design, in Polish, is in [docs/PROJEKT.md](docs/PROJEKT.md).

## What it is for

- A native GNOME app (GTK 4, libadwaita) for MSI motherboards on Linux. The point of reference is the sensor and fan part of MSI Center on Windows.
- For sensors and fans it relies only on the kernel drivers and their standard hwmon interface in sysfs. It has no hardware access of its own: no I/O ports, `/dev/port` or `/dev/mem`, and nothing in user space writes to the embedded controller (EC).
- What it does today (M1), read only:
  - temperatures, fan speeds and voltages, named after the fan headers and rails in the board profile, with voltages rated against their tolerance and the minimum and maximum since start;
  - a System page with the board, BIOS, processor, memory, graphics card, disks and the state of the fan driver, and a "Copy Information" button for bug reports (no serial numbers or MAC addresses);
  - English and Polish.
- One verified board: **MAG B850M MORTAR WIFI (MS-7E61)**, PCB revision 1. On other computers Baseboard runs in a generic mode: readings without the board's header names and no fan control.

## Roadmap and kernel support

Fan control on these boards depends on the `nct6683` kernel driver. Baseboard does not check the kernel version. It reads the sysfs attributes of each fan channel and decides from them what the driver allows. M1 already detects this level and shows it on the System page. Fan control itself comes with M2; from then on each level below unlocks by itself once the kernel provides it, without a new release of Baseboard.

If a patch in review changes its interface, Baseboard will have to follow it. Choosing the temperature source will be enabled only for the interface that is sent to the mailing list.

| Level | What Baseboard does (M2) | What it needs from the kernel | State in the kernel |
|---|---|---|---|
| Read only | Readings, and the fans shown read only with a note that control needs a newer driver | The `nct6683` driver for the board's NCT6687D-R chip | In current kernels (checked with Fedora's 7.3.3 kernel) |
| Manual control | A fixed duty cycle per header, set through the helper | Writable `pwmN` and `pwmN_enable`: manual fan control for MSI boards with NCT6687D-R, and its restore after resume | In `hwmon-next`, the hwmon maintainer's tree; not in a released kernel yet |
| Curve view | The fan curves stored in the EC and their temperature source, as a preview | Readable `pwmN_auto_point*` and `pwmN_auto_channels_temp` | In `hwmon-next`; not in a released kernel yet |
| Curve editing | Editing the seven-point curves in the EC; they keep working without the app | Writable `pwmN_auto_point*` | Patches in review on linux-hwmon: [v1 thread](https://lore.kernel.org/linux-hwmon/20261109223442.509238-1-tomasz@bojanowski.me/) |
| Temperature source | Choosing the sensor a curve follows | Writable `pwmN_auto_channels_temp` | Planned, not sent yet |

Patches in review may change or may not be accepted. Kernel versions are not given until the patches are in the mainline tree.

### Next

- **M2, fans:** a curve preview first, then a small system helper (D-Bus and polkit) that is the only part allowed to change fan settings, then manual mode and a curve editor. The helper follows fixed safety rules:
  - minimum duty cycles for the CPU fan and the pump;
  - the pump never stops;
  - curves for the CPU and the pump reach 100 % by the emergency temperature;
  - headers in manual mode return to firmware control when the helper exits or crashes, before shutdown, and when a temperature passes its emergency threshold;
  - settings last until the next reboot;
  - control only on a board whose profile matches, PCB revision included, and only on headers whose writes were tested on that board.
- **More boards:** profiles of other boards only from their manuals, public sources and tests on the hardware. Until the header mapping is tested, such a profile shows readings and names without fan control.

### Under consideration

These are ideas without a schedule, and they may not happen:

- onboard RGB lighting (M3);
- power profiles and restarting into the firmware setup (M4);
- help with BIOS updates without flashing from the running system (M5).

## Report your board

Open an issue at <https://github.com/TomaszBojanowski/baseboard/issues> and paste the text from **System → Copy Information**. It holds the board model and code, the BIOS, the hwmon devices and the fan driver level, and no serial numbers or MAC addresses.

## Building

Requires Rust 1.92 or newer, GTK 4.22, libadwaita 1.9 and Meson 1.1.

```sh
meson setup build
meson compile -C build
meson test -C build
# run without installing, with the settings schema of the build directory
meson devenv -C build ./crates/app/baseboard
```

### Flatpak

The manifest is in `build-aux/flatpak/`. It uses the GNOME 51 runtime and builds libsensors from lm-sensors. The Rust dependencies come from `cargo-sources.json`, so Cargo does not need the network during the build. After a change of `Cargo.lock`, generate that file again with `flatpak-cargo-generator.py` from [flatpak-builder-tools](https://github.com/flatpak/flatpak-builder-tools).

```sh
flatpak-builder --user --install --install-deps-from=flathub --force-clean \
    build/flatpak build-aux/flatpak/me.bojanowski.Baseboard.json
flatpak run me.bojanowski.Baseboard
```

The Flatpak reads the sensors and the system information with read-only access to the udev database (`/run/udev`). Differences from a native installation:

- It does not see the system's lm-sensors configuration, so labels, `ignore` and `compute` statements from `/etc/sensors.d` do not apply; the board's readings still come scaled and named from its profile. To use the configuration, give the app read-only access to the whole of `/etc`:

  ```sh
  flatpak override --user --filesystem=host-etc:ro me.bojanowski.Baseboard
  ```

- It cannot see other programs that control fans, so the System page says it cannot check them.
- Its settings and own reading names are kept apart from a native installation, under `~/.var/app/me.bojanowski.Baseboard/`.

Fan control (M2) will need a system helper that cannot be part of a Flatpak; it will come as a separate system package.

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).

## Trademarks

MSI, MSI Center and MSI product names are trademarks of Micro-Star International Co., Ltd. Baseboard is an independent project and is not affiliated with or endorsed by MSI.
