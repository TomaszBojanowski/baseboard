# SPDX-License-Identifier: GPL-3.0-or-later

project(
  'baseboard',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.1.0',
)

i18n = import('i18n')
gnome = import('gnome')

app_id = 'me.bojanowski.Baseboard'

# GNOME 50 is the minimum.
gtk_dep = dependency('gtk4', version: '>= 4.22')
adw_dep = dependency('libadwaita-1', version: '>= 1.9')

cargo = find_program('cargo')
# Used by build.rs to build the resource bundle.
find_program('glib-compile-resources')

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
localedir = prefix / get_option('localedir')

subdir('data')
subdir('po')
subdir('crates/app')

# `meson devenv -C build ./crates/app/baseboard` runs the app without
# installing it, with the settings schema and the translations of the build
# directory and the board profiles of the source tree.
devenv = environment()
devenv.set('GSETTINGS_SCHEMA_DIR', meson.project_build_root() / 'data')
devenv.set('BASEBOARD_LOCALEDIR', meson.project_build_root() / 'po')
devenv.set('BASEBOARD_BOARDS_DIR', meson.project_source_root() / 'data' / 'boards')
meson.add_devenv(devenv)

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)

summary({
  'Application ID': app_id,
  'GTK': gtk_dep.version(),
  'libadwaita': adw_dep.version(),
})
