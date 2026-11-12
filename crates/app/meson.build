# SPDX-License-Identifier: GPL-3.0-or-later

cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'cargo',
]

if get_option('buildtype') == 'release'
  rust_target = 'release'
  cargo_build_options = cargo_options + ['--release']
else
  rust_target = 'debug'
  cargo_build_options = cargo_options
endif

cargo_env = [
  'BASEBOARD_LOCALEDIR=' + localedir,
  'BASEBOARD_PKGDATADIR=' + datadir / 'baseboard',
]

app = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: 'baseboard',
  console: true,
  install: true,
  install_dir: bindir,
  command: [
    'env', cargo_env,
    cargo, 'build', cargo_build_options, '--package', 'baseboard',
    '&&',
    'cp', meson.project_build_root() / 'cargo' / rust_target / 'baseboard', '@OUTPUT@',
  ],
)

test(
  'cargo-test',
  cargo,
  args: ['test', cargo_build_options, '--workspace'],
  env: cargo_env,
  timeout: 600,
)

# The tour of every page and subpage on an invisible X server
# (tests/tour.sh); only debug builds have it.
if rust_target == 'debug'
  test(
    'tour',
    find_program(meson.project_source_root() / 'tests' / 'tour.sh'),
    args: [app],
    depends: app,
    env: [
      'GSETTINGS_SCHEMA_DIR=' + meson.project_build_root() / 'data',
      'BASEBOARD_LOCALEDIR=' + meson.project_build_root() / 'po',
      'BASEBOARD_BOARDS_DIR=' + meson.project_source_root() / 'data' / 'boards',
    ],
    timeout: 900,
  )
endif
