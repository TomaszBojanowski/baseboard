# Makiety interfejsu

Stan na 2026-11-06, po pierwszym przeglądzie (decyzja D6 w `docs/PROJEKT.md`). Przegląd poprawiony 2026-11-10 (krok 8 M1): teksty jak w aplikacji, według D23, D25–D27. System poprawiony 2026-11-11 (krok 10 M1): zakres M1 i teksty jak w aplikacji. To makiety HTML, bez kodu Rust. `renderuj.sh` tworzy z nich obrazy w trybie jasnym i ciemnym w `png/`; obrazy nie są w repozytorium.

| Strona | Plik | Etap | Dane |
|---|---|---|---|
| Przegląd | `przeglad.html` | M1 | prawdziwe odczyty z tej maszyny |
| Wentylatory | `wentylatory.html` | M2 (podgląd) | obroty prawdziwe; tryby i wypełnienie przykładowe |
| Krzywa wentylatora (podstrona) | `wentylatory-krzywa.html` | M2 (podgląd) | temperatura procesora prawdziwa; punkty krzywej przykładowe |
| RGB | `rgb.html` | M3 (zarys) | przykładowe |
| Wydajność | `wydajnosc.html` | M4 (zarys) | prawdziwe |
| System | `system.html` | M1 (bez kontrolera RGB z M3 i restartu do UEFI z M4) | prawdziwe |

**Oglądanie:** plik HTML w przeglądarce. Dopisek `?theme=dark` albo `?theme=light` wymusza tryb, bez niego obowiązuje ustawienie systemu.

**Nowe PNG:** `makiety/renderuj.sh` (headless Chromium, skala ×2). `SCALE=1` daje mniejsze pliki.

## Skąd dane

- **Przegląd:** 90 odczytów `sensors -j`, co sekundę. „Teraz” to ostatni odczyt, a szary zakres to minimum i maksimum z tych 90 s. Wypełnienie wentylatora procesora, pompy i wentylatora dodatkowego pochodzi z `pwm1`, `pwm2` i `pwm4`.
- **Nazwy:** domyślne, czyli bez opcji etykiet z lm-sensors (§5.1). Kolejność źródeł: role z profilu płyty, potem nazwy wbudowane dla znanych sterowników, potem etykieta sterownika. Podtytuł pokazuje złącze (np. `CPU_FAN1`) albo źródło odczytu (np. `amdgpu · edge`).
- **Wydajność:** stan z D-Bus (`tuned-ppd`, profil „balanced”, brak blokad) i cpufreq.
- **System:** to, co pokazuje aplikacja na tej maszynie z modułem `nct6683` serii v1 (stąd „Krzywe w EC”; ze sterownikiem Fedory wiersz brzmi „Sterowanie wentylatorami niedostępne” z ostrzeżeniem): DMI bez numerów seryjnych, pamięć z bazy udev, sysfs, `/proc` i `/etc/sensors.d`. Data BIOS-u jest zapisana jak w `LC_TIME`, bez nazwy miesiąca.
- **Wygląd:** kolory, promienie i odstępy pochodzą z arkusza stylów zainstalowanego libadwaita 1.10, czcionka to Adwaita Sans. Okno ma 1200 px szerokości i jest tak wysokie, żeby cała treść mieściła się bez przewijania.

## Układ

1. **Przegląd** przy szerokim oknie ma dwie kolumny, a przy wąskim jedną. Kolejność w jednej kolumnie: Procesor, Płyta, Wentylatory, Napięcia, Karta graficzna, Dyski, Pamięć, Inne. W dwóch kolumnach Pamięć trafia do lewej, żeby wyrównać wysokość kolumn.
2. **Minimum i maksimum** stoją jako szary zakres obok bieżącej wartości. Objaśnia je jedno zdanie nad grupami.
3. **Napięcia** oceniamy ikoną i tekstem, nie samym kolorem.
4. **Wentylatory:** tryb wybiera się przełącznikiem segmentowym (AdwToggleGroup) w karcie złącza. Karta w trybie Krzywa ma strzałkę i otwiera podstronę z edytorem (AdwNavigationPage, przycisk „Wstecz” w nagłówku).
5. **Baner** „Ustawienia działają do restartu komputera” z przyciskiem „Przywróć automat BIOS” widać tylko wtedy, gdy któryś kanał jest przejęty. Jest na stronie Wentylatory i na podstronie krzywej. Pasek nagłówka nie ma osobnego przycisku.
6. **Edytor krzywej:** wykres z punktem pracy („Teraz”), wartościami poza skrajnymi punktami (linia przerywana) i progiem awaryjnym źródła, obok lista punktów. Zmiany zaczynają działać po „Zastosuj”, bo zapis wymaga autoryzacji polkit (D3).
7. **Wydajność:** EPP i tryb sterownika są opisane po polsku, a surowa wartość jest w podtytule.

## Uwagi

- **Nazwa karty graficznej** pochodzi najpierw z `/sys/class/drm/card*/device/product_name`. Na tej maszynie amdgpu tego pliku nie tworzy, więc nazwa pochodzi z bazy PCI (hwdata). Baza podaje „Radeon RX 9050 / 9060 XT”, bo podsystemu PowerColor 148c:2437 nie ma na liście.
- **Moduły pamięci** nazywają się tak jak gniazda z bazy udev (DIMMA2, DIMMB2), bo adresy SPD 0x51 i 0x53 jednoznacznie wskazują obsadzone gniazda (§5.1, D8). Adres SPD stoi w podtytule. Bez takiego dopasowania aplikacja pokazuje „Moduł pamięci 1” i „Moduł pamięci 2”.
- **Opis grupy Procesor** to model z `/proc/cpuinfo` bez końcówki „8-Core Processor”; tak samo strona System (D38).
- **Pompa i wentylator dodatkowy** mają w podtytule „złącze nietestowane”, bo przypisanie złączy PUMP_SYS1 i JAF_1 do kanałów nie jest potwierdzone (O3).
- **Zakres min–maks** pojawia się dopiero wtedy, gdy wartości się rozejdą; „0–0 rpm” czy „12,10–12,10 V” nie stoją na stronie (D38).
- **Dyski** nazywają się tak jak model z sysfs, np. `CT2000P3PSSD8`. Przyjazne nazwy z `/etc/sensors.d` pojawiają się po włączeniu opcji etykiet z lm-sensors.
- **Strefy RGB są przykładowe.** Które złącza RGB są na płycie i jak odpowiadają strefom, pozostaje otwarte (O1).
- **Złącze pompy** pokazuje 0 rpm przy wypełnieniu 100 %, bo nic do niego nie jest podłączone. Aplikacja nie odróżni pustego złącza od zatrzymanej pompy, więc Przegląd o tym nie ostrzega. B11 reaguje tylko wtedy, gdy kanał wcześniej miał obroty.
- **Ikony** wentylatora, układu scalonego i innych elementów są narysowane na potrzeby makiety, bo Adwaita ich nie ma. Aplikacja będzie potrzebować własnych ikon symbolicznych.
