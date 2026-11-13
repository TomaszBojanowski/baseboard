# SPDX-License-Identifier: GPL-3.0-or-later

# Only comments marked for translators reach them, not the doc comments of
# the code.
i18n.gettext(
  meson.project_name(),
  args: ['--from-code=UTF-8', '--add-comments=Translators:'],
)

msgfmt = find_program('msgfmt', required: false)
if msgfmt.found()
  foreach lang : ['pl']
    test('validate-po-' + lang, msgfmt,
      args: ['--check', '--output-file=/dev/null', meson.current_source_dir() / lang + '.po'],
    )
  endforeach
endif
