# SPDX-License-Identifier: GPL-3.0-or-later

# Only comments marked for translators reach them, not the doc comments of
# the code.
i18n.gettext(
  meson.project_name(),
  args: ['--from-code=UTF-8', '--add-comments=Translators:'],
)

# Every language of LINGUAS valid and complete, without fuzzy or obsolete
# entries (docs/PROJEKT.md §8, Z7).
find_program('msgattrib')
test('translations',
  find_program(meson.project_source_root() / 'tests' / 'translations.sh'),
  args: [meson.current_source_dir()],
)
