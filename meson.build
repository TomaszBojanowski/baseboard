# SPDX-License-Identifier: GPL-3.0-or-later

project(
  'baseboard',
  version: '0.2.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.1.0',
)

i18n = import('i18n')
gnome = import('gnome')
fs = import('fs')

app_id = 'me.bojanowski.Baseboard'

# One version everywhere (D43): the workspace in Cargo.toml (the app's own
# version, shown in About) and the newest release in the metainfo must say
# the same as the project.
version = meson.project_version()
if not fs.read('Cargo.toml').contains('\nversion = "@0@"\n'.format(version))
  error('Cargo.toml: the workspace version is not @0@'.format(version))
endif
releases = fs.read('data' / app_id + '.metainfo.xml.in').split('<release version="')
if releases.length() < 2 or releases[1].split('"')[0] != version
  error('metainfo: the newest <release> is not @0@'.format(version))
endif

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

# Checks of the source tree (docs/PROJEKT.md §8): every public item of
# core is used or planned (Z3), resources and styles have no orphans (Z6),
# and "Stan prac" stays short (Z13).
python = find_program('python3')
foreach check : ['core_api', 'resources', 'work_status']
  test(check.replace('_', '-'), python,
    args: [meson.project_source_root() / 'tests' / check + '.py', meson.project_source_root()],
  )
endforeach

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
