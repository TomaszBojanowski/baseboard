# SPDX-License-Identifier: GPL-3.0-or-later

# The .desktop file and the metainfo take their translations from po/ with
# msgfmt, as i18n.merge_file does. merge_file depends only on its input, so
# a changed translation would not rebuild them; these targets depend on
# po/LINGUAS and every .po file it lists.
msgfmt = find_program('msgfmt')
po_dir = meson.project_source_root() / 'po'
po_files = files('../po/LINGUAS')
foreach line : fs.read('../po/LINGUAS').split('\n')
  lang = line.strip()
  if lang != '' and not lang.startswith('#')
    po_files += files('../po' / lang + '.po')
  endif
endforeach

desktop_file = custom_target('desktop-file',
  input: app_id + '.desktop.in',
  output: app_id + '.desktop',
  command: [msgfmt, '--desktop', '--template=@INPUT@', '-d', po_dir, '-o', '@OUTPUT@'],
  depend_files: po_files,
  install: true,
  install_dir: datadir / 'applications',
)

desktop_file_validate = find_program('desktop-file-validate', required: false)
if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

metainfo_file = custom_target('metainfo-file',
  input: app_id + '.metainfo.xml.in',
  output: app_id + '.metainfo.xml',
  command: [msgfmt, '--xml', '--template=@INPUT@', '-d', po_dir, '-o', '@OUTPUT@'],
  depend_files: po_files,
  install: true,
  install_dir: datadir / 'metainfo',
)

appstreamcli = find_program('appstreamcli', required: false)
if appstreamcli.found()
  test('validate-metainfo', appstreamcli,
    args: ['validate', '--no-net', '--explain', metainfo_file],
  )
endif

icons_dir = datadir / 'icons' / 'hicolor'
install_data(
  'icons/hicolor/scalable/apps/@0@.svg'.format(app_id),
  install_dir: icons_dir / 'scalable' / 'apps',
)
install_data(
  'icons/hicolor/symbolic/apps/@0@-symbolic.svg'.format(app_id),
  install_dir: icons_dir / 'symbolic' / 'apps',
)

install_data(app_id + '.gschema.xml', install_dir: datadir / 'glib-2.0' / 'schemas')

# Board profiles (docs/PROJEKT.md §6.2). The app reads them, and so will the
# helper, which reads only this folder and /etc/baseboard/boards; `cargo
# test` checks that each one is valid.
install_data(
  'boards/ms-7e61.toml',
  install_dir: datadir / 'baseboard' / 'boards',
)

# Compiled into the build directory for `meson devenv` and the tour.
compiled_schemas = gnome.compile_schemas(
  build_by_default: true,
  depend_files: files(app_id + '.gschema.xml'),
)

glib_compile_schemas = find_program('glib-compile-schemas', required: false)
if glib_compile_schemas.found()
  test('validate-schema', glib_compile_schemas, args: ['--strict', '--dry-run', meson.current_source_dir()])
endif
