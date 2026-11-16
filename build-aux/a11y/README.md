# Accessibility check

What a screen reader gets from Baseboard, checked over AT-SPI instead of
by hand with Orca (docs/PROJEKT.md M2.8 and §10.7, decision D79). Run it
before a release and after changes to a page.

```sh
meson compile -C build          # the debug build in build/
sh build-aux/a11y/run.sh        # results in build/a11y/
```

`run.sh` starts the debug build on the v1 test tree, on an invisible X
server, with a private session bus without services and a private
accessibility bus, nine times in parallel (X displays 261–269):

- the Overview, the Fans page, the curve details of SYS 1 and the System
  page, in English and in Polish;
- the Overview while the processor goes to 93 °C, so that the status
  sentence turns into a warning.

Each run dumps the app's AT-SPI tree and presses Tab over AT-SPI through
the page (`dump.py`). `check.py` then prints the elements Tab reaches, in
order, with their role, name and description, the images (charts) with
their names, and the texts that are the same in both languages. It exits
with 1 on a problem: an element Tab reaches without a name or with a role
that says nothing of it, an image without a name, an element that can be
clicked but not reached with the keyboard, English text in the Polish
version (an English msgid of `po/pl.po`), or a GTK or GLib warning in the
app's log. The list of Tab stops is for a person to read: whether each
name and description says what the element shows.

Known exception: the rows of the sidebar come from libadwaita's
`AdwSidebar` and have no name of their own; a screen reader gets the label
inside them, and the check lists them so.

Needs `xvfb-run`, `dbus-daemon`, `at-spi2-core` (`at-spi2-registryd` in
`/usr/libexec`), Python with GObject introspection and the `Atspi` typelib
(`python3-gobject`), and the Polish locale. `at-spi-bus-launcher` is not
used: it needs a session of its own, and GTK and libatspi both take the
accessibility bus from `AT_SPI_BUS_ADDRESS` first.
