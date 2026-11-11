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

custom_target(
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
