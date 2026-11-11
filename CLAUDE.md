# Zasady pracy w tym repozytorium

Natywna aplikacja GNOME (Rust, GTK4/libadwaita, meson) do monitoringu i sterowania sprzętem płyty głównej oraz pomocnik systemowy (D-Bus, polkit). Pełny projekt: `docs/PROJEKT.md`. Przed zmianą zachowania sprawdź, czy jest z nim zgodna. Jeśli nie jest, najpierw zaktualizuj projekt.

Nazwa aplikacji: **Baseboard** (decyzja D1). App ID `me.bojanowski.Baseboard`, pliki binarne `baseboard` i `baseboard-helper`, domena gettext `baseboard`, D-Bus `me.bojanowski.Baseboard.Helper`, polkit `me.bojanowski.Baseboard.*`. Licencja GPL-3.0-or-later (D2).

Zasady lokalne właściciela (notatki na jego komputerze, uprawnienia, git, pamięć) są w pliku poza repozytorium: @CLAUDE.local.md

## Źródła wiedzy i czystość (`docs/PROJEKT.md` §2)

- Z notatek z analizy interoperacyjności oprogramowania producenta (leżą poza repozytorium) do repozytorium trafiają wyłącznie fakty o interfejsach sprzętu, opisane własnymi słowami (np. „`pwm16` steruje złączem SYS_FAN1”). W szczególności nie przenoś:
  - kodu ani fragmentów dekompilacji,
  - nazw wewnętrznych klas, funkcji i plików producenta,
  - adresów funkcji, kluczy, stałych „magicznych”,
  - plików danych producenta,
  - dosłownych cytatów z notatek.
- Kodu z projektów o innych licencjach (OpenRGB, LibreHardwareMonitor, nct6687d i podobnych) też nie kopiujemy. Bierzemy tylko opis protokołu i podajemy źródło w `docs/`.
- Aplikacja nie potrzebuje adresów rejestrów EC: wentylatorami steruje wyłącznie przez sysfs sterownika jądra. Nie dodawaj dostępu do portów I/O, `/dev/port` ani `/dev/mem`.
- W nazwie aplikacji, ikonie i tekstach interfejsu nie używaj znaków towarowych producenta płyty. Wyjątek: dane identyfikacyjne płyty z DMI w profilach i ich wyświetlanie jako informacja o sprzęcie.
- Do repozytorium nie trafiają lokalne ścieżki, nazwy plików notatek, numery seryjne ani inne dane prywatne.

## Sprzęt

- Zapis do sprzętu (`pwm*` i `pwm*_enable` w sysfs, `SET_FEATURE` na `/dev/hidraw*`, ładowanie i wyładowanie modułów) i polecenia z `sudo` tylko za zgodą właściciela komputera, za każdym razem osobno.
- Do kontrolera RGB wolno wysyłać wyłącznie raport 0x50 (odczyt i zapis) oraz 0x51 (zapis). Bajt store w 0x50 domyślnie 0.
- Zasady bezpieczeństwa wentylatorów (`docs/PROJEKT.md` §4, B1–B15) są twarde. Ich złagodzenie wymaga decyzji właściciela projektu zapisanej w projekcie.
- Do sysfs pisze tylko pomocnik. Aplikacja, testy i skrypty nie piszą do sysfs. Testy używają sztucznego drzewa sysfs w `tests/fixtures/`.

## Git

- Commity małe, jedna logiczna zmiana na commit.
- Wiadomości po angielsku, w trybie rozkazującym, temat do 72 znaków (np. „Add board profile parser”).
- Przed każdym commitem `git status` ma być czysty poza zmianami z danego kroku.

## Stan prac i decyzje

- Po każdym zakończonym kroku zaktualizuj stan w `docs/PROJEKT.md` (§7, „Stan prac”) w tym samym commicie co zmiana.
- Decyzje zapisuj tylko w tabeli decyzji `docs/PROJEKT.md` (§12), z datą.

## Struktura katalogów i build

- Struktura katalogów zgodna z `docs/PROJEKT.md` §3.6. Nowy katalog najwyższego poziomu tylko za zgodą właściciela projektu.
- Pliki robocze wyłącznie poza repozytorium, nigdy w drzewie projektu.
- Build zawsze w tym samym katalogu `build/` (ignorowanym), bez dodatkowych katalogów buildu.

## Tłumaczenia

- gettext od pierwszego kodu. Każdy tekst widoczny dla użytkownika przechodzi przez `gettext`/`ngettext`/`pgettext`. msgid po angielsku.
- Nowy lub zmieniony tekst: zaktualizuj `po/POTFILES.in` i `po/pl.po` w tym samym commicie.
- Plik `.desktop`, metainfo i akcje polkit tłumaczone przez `msgfmt` w meson: `custom_target` zależny od `po/LINGUAS` i plików `.po` (nie `i18n.merge_file`, który nie przebudowuje pliku po zmianie tłumaczeń).
- Nie składaj zdań z kawałków. Liczby formatuj zgodnie z lokalizacją.

## Kod i jakość

- Przed commitem: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `meson test`.
- `unsafe` tylko w modułach FFI (libsensors, libudev, i18n: `setlocale` i `nl_langinfo`, ioctl hidraw), każdy blok z komentarzem `// SAFETY:`.
- Pomocnik przyjmuje z D-Bus wyłącznie klucze kanałów z profilu, nigdy ścieżek. Profile czyta tylko z katalogów systemowych.
- Każdy plik źródłowy (Rust, meson, skrypty, pliki danych, w których komentarz jest przyjęty) zaczyna się od `SPDX-License-Identifier: GPL-3.0-or-later`. Metainfo ma `CC0-1.0`. Ikon SVG nie zmieniamy.
- Język: `docs/` po polsku; README, komentarze w kodzie i wiadomości commitów po angielsku.
