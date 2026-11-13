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
- W nazwie aplikacji i ikonie nie używaj znaków towarowych producenta płyty. Nazwa producenta może paść tylko opisowo w opisie projektu („do płyt głównych MSI”: strona, README, metainfo, plik `.desktop`, okno „O programie”, opis repozytorium), z zastrzeżeniem o znakach towarowych wszędzie tam, gdzie opis ma na nie miejsce (D58). W pozostałych tekstach interfejsu nie, poza danymi identyfikacyjnymi płyty z DMI w profilach i ich wyświetlaniem jako informacji o sprzęcie.
- Do repozytorium nie trafiają lokalne ścieżki, nazwy plików notatek, numery seryjne ani inne dane prywatne.

## Sprzęt

- Zapis do sprzętu (`pwm*` i `pwm*_enable` w sysfs, `SET_FEATURE` na `/dev/hidraw*`, ładowanie i wyładowanie modułów) i polecenia z `sudo` tylko za zgodą właściciela komputera, za każdym razem osobno.
- Do kontrolera RGB wolno wysyłać wyłącznie raport 0x50 (odczyt i zapis) oraz 0x51 (zapis). Bajt store w 0x50 domyślnie 0.
- Zasady bezpieczeństwa wentylatorów (`docs/PROJEKT.md` §4, B1–B18) są twarde. Ich złagodzenie wymaga decyzji właściciela projektu zapisanej w projekcie.
- Do sysfs pisze tylko pomocnik. Aplikacja, testy i skrypty nie piszą do sysfs. Testy używają sztucznego drzewa sysfs w `tests/fixtures/`.

## Git

- Commity małe, jedna logiczna zmiana na commit.
- Wiadomości po angielsku, w trybie rozkazującym, temat do 72 znaków (np. „Add board profile parser”).
- Przed każdym commitem `git status` ma być czysty poza zmianami z danego kroku.

## Stan prac i decyzje

- `docs/PROJEKT.md` to specyfikacja: jak aplikacja działa dziś i plan. Bez historii prac (jest w gicie).
- Po każdym zakończonym kroku zastąp „Stan prac” w `docs/PROJEKT.md` (§7; lista: wydane, w toku, otwarte przed następnym wydaniem, później; do 250 słów) w tym samym commicie co zmiana.
- Decyzje zapisuj tylko w tabeli `docs/DECYZJE.md`, z datą; decyzję zastąpioną oznacz „zastąpione przez Dxx”.

## Struktura katalogów i build

- Struktura katalogów zgodna z `docs/PROJEKT.md` §3.6. Nowy katalog najwyższego poziomu tylko za zgodą właściciela projektu.
- Pliki robocze wyłącznie poza repozytorium, nigdy w drzewie projektu.
- Build zawsze w tym samym katalogu `build/` (ignorowanym), bez dodatkowych katalogów buildu.

## Wygląd: nowoczesny minimalizm (D63)

Obowiązuje każdą stronę, także przyszłe (podświetlenie, wydajność):

- na pierwszym planie zwykłe słowa i najważniejsze liczby;
- oznaczenia sterowników, czujników i złączy, ścieżki plików i wartości pomocnicze o jedno kliknięcie dalej;
- mało ramek, dużo powietrza, jeden kolor akcentu, układy libadwaita jak w Ustawieniach GNOME;
- nic nie ginie: wszystko, co dziś widać, da się nadal znaleźć i skopiować do zgłoszenia błędu;
- ostrzeżenia i informacje o bezpieczeństwie zostają, prostym językiem.

## Tłumaczenia

- gettext od pierwszego kodu. Każdy tekst widoczny dla użytkownika przechodzi przez `gettext`/`ngettext`/`pgettext`. msgid po angielsku.
- Nowy lub zmieniony tekst: zaktualizuj `po/POTFILES.in` i `po/pl.po` w tym samym commicie (`meson compile -C build baseboard-pot baseboard-update-po`, potem `msgattrib --no-obsolete -o po/pl.po po/pl.po`).
- Plik `.desktop`, metainfo i akcje polkit tłumaczone przez `msgfmt` w meson: `custom_target` zależny od `po/LINGUAS` i plików `.po` (nie `i18n.merge_file`, który nie przebudowuje pliku po zmianie tłumaczeń).
- Nie składaj zdań z kawałków. Liczby formatuj zgodnie z lokalizacją.

## Kod i jakość

- Przed commitem: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` i to samo z `--release`, `meson test` (z testem `tour`). Wymagania i automaty: `docs/PROJEKT.md` §8.
- `unsafe` tylko w modułach FFI (libsensors, libudev, i18n: `setlocale` i `nl_langinfo`, ioctl hidraw), każdy blok z komentarzem `// SAFETY:`.
- Pomocnik przyjmuje z D-Bus wyłącznie klucze kanałów z profilu, nigdy ścieżek. Profile czyta tylko z katalogów systemowych.
- Każdy plik źródłowy (Rust, meson, skrypty, pliki danych, w których komentarz jest przyjęty) zaczyna się od `SPDX-License-Identifier: GPL-3.0-or-later`. Metainfo ma `CC0-1.0`. Ikon SVG nie zmieniamy.
- Język: `docs/` po polsku; README, komentarze w kodzie i wiadomości commitów po angielsku.

## Gotowe: pytania przed każdym commitem

Na każde odpowiedź „tak”; inaczej commit nie jest gotowy.

1. `cargo fmt --check`, `cargo clippy` (debug i release) z `-D warnings` i `meson test` (z `tour`) są zielone?
2. Każda zmiana zachowania ma test, który przed zmianą nie przechodził, a wiadomość commita mówi, jak to sprawdzono?
3. Nowy klikalny element (na stronie, podstronie albo w oknie dialogowym) ma oczekiwanie w `tour`?
4. Zmiana układu usuwa stary kod, pola modelu, testy, style, ikony, teksty i makiety, a wiadomość commita wymienia je w sekcji „Removed:”?
5. Każde nowe pole, parametr, opcja albo abstrakcja ma dziś co najmniej dwie wartości albo dwa użycia (wyjątek: krok planu na liście wyjątków `tests/core-api-exceptions.txt`)?
6. Teksty idą przez gettext, `POTFILES.in` jest aktualny, `pl.po` nie ma fuzzy ani `#~`, a żadne pojęcie nie dostało drugiego słowa (D66)?
7. Komentarze mówią, co jest dziś i dlaczego: bez historii zmian, a komentarze dla tłumaczy bez numerów decyzji?
8. Specyfikacja w `docs/PROJEKT.md` zgadza się z kodem, „Stan prac” jest zastąpiony, nie dopisany, a decyzja jest w `docs/DECYZJE.md` (zastąpiona oznaczona)?
9. To, co zmieniłem, kliknąłem w prawdziwej aplikacji (albo sprawdza to `tour`), a w jej wyjściu nie ma ostrzeżeń?
10. `git status` jest czysty poza zmianami kroku, commit jest mały, po angielsku, a temat ma do 72 znaków?
