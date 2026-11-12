# Makiety interfejsu

Stan na 2026-11-06, po pierwszym przeglądzie (decyzja D6 w `docs/PROJEKT.md`). Przegląd poprawiony 2026-11-10 (krok 8 M1): teksty jak w aplikacji, według D23, D25–D27. System poprawiony 2026-11-11 (krok 10 M1): zakres M1 i teksty jak w aplikacji. Wentylatory przebudowane 2026-11-11 do planu M2 (§7 w `docs/PROJEKT.md`), poprawione według D45 i zaakceptowane przez właściciela tego samego dnia (z dwiema drobnymi poprawkami: jeden komunikat o czasie działania zmian, bez etykiety „Krzywa z BIOS-u”). Strona Wentylatory przerysowana 2026-11-12 w układzie D62 (duża temperatura, lista wentylatorów) razem ze stroną „Szczegóły krzywej”; edytor i arkusz stanów zostają w układzie A do czasu, aż sterowanie z M2b dostanie projekt w układzie D62. To makiety HTML, bez kodu Rust. `renderuj.sh` tworzy z nich obrazy w trybie jasnym i ciemnym w `png/`; obrazy nie są w repozytorium.

| Strona | Plik | Etap | Dane |
|---|---|---|---|
| Przegląd | `przeglad.html` | M1 | prawdziwe odczyty z tej maszyny |
| Wentylatory (D62) | `wentylatory.html`; `?stan=85`, `?stan=reczny`, `?stan=fabryczny` | M2a (0.2.0) | jak w drzewie testowym aplikacji: obroty, wypełnienie i krzywe z MS-7E61; temperatury 50 i 85 °C i tryb ręczny SYS_FAN2 (45 %) przykładowe |
| Szczegóły krzywej (D62) | `wentylatory-szczegoly.html`; `?wentylator=pump1`, `?stan=85` | M2a (0.2.0) | jak wyżej |
| Krzywa wentylatora (podstrona, edytor; układ A) | `wentylatory-krzywa.html`; `?stan=blad`, `?stan=bios-cpu`, `?stan=podglad` | M2b | temperatura prawdziwa; edytowane krzywe przykładowe; krzywa CPU z BIOS-u według §6.3 |
| Wentylatory: wszystkie stany (układ A) | `wentylatory-stany.html` (arkusz bez okna) | M2b | stany błędów i pompa ze sterowaniem przykładowe |
| RGB | `rgb.html` | M3 (zarys) | przykładowe |
| Wydajność | `wydajnosc.html` | M4 (zarys) | prawdziwe |
| System | `system.html` | M1 (bez kontrolera RGB z M3 i restartu do UEFI z M4) | prawdziwe |

**Oglądanie:** plik HTML w przeglądarce. Dopisek `?theme=dark` albo `?theme=light` wymusza tryb, bez niego obowiązuje ustawienie systemu. `width=narrow` daje wąskie okno (400 px, pasek boczny zwinięty jak w aplikacji poniżej 600 sp), `stan=…` wybiera stan strony, a `accent=…` inny kolor akcentu libadwaita (`teal`, `green`, `yellow`, `orange`, `red`, `pink`, `purple`, `slate`), np. `wentylatory.html?stan=85&width=narrow&accent=purple`.

**Pasek boczny** ma tylko strony, które aplikacja ma albo dostaje w M2 (Przegląd, Wentylatory, System); RGB i Wydajność dojdą ze swoimi etapami, choć ich zarysy (`rgb.html`, `wydajnosc.html`) zostają.

**Nowe PNG:** `makiety/renderuj.sh [strona[@stan] …]` (headless Chromium, skala ×2). `SCALE=1` daje mniejsze pliki, `NARROW=1` dodaje wąskie okno, `ACCENT=…` inny akcent, `OUT=katalog` zmienia katalog wyjściowy (domyślnie `png/`).

**Pliki wspólne wentylatorów:** `wykres.js` rysuje wykres krzywej w SVG w rzeczywistym rozmiarze pola (tekst się nie skaluje), `wentylatory.js` ma dane i elementy kart układu A (edytor, arkusz stanów), `wentylatory.css` style. Strona D62 ma własne `wentylatory-lista.js` i `wentylatory-lista.css`, szczegóły krzywej `wentylatory-szczegoly.js` i `.css`. Wykresy rysują się po zbudowaniu okna (`window.afterBuild` w `wspolne.js`). Do zrzutów: `w=1024` daje okno 1024 px, `h=1100` okno o stałej wysokości (jeden ekran), `bare=1` samo okno bez tła.

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
4. **Wentylatory (D62):** u góry wyśrodkowana duża temperatura źródła krzywych z podpisem „Procesor”; pod nią zawsze zarezerwowany wiersz „Wentylatory przyspieszają” (przygaszony, widoczny, gdy któryś wentylator dochodzi do wyższego celu o co najmniej 5 punktów), a na sterowniku bez sterowania zdanie o jądrze z linkiem „Stan w jądrze”. Pod spodem lista szerokości jak w Ustawieniach GNOME (600 px), jeden wiersz na wentylator: nazwa, stan zwykłymi słowami, mały wykres (jedna linia, punkt „teraz”; 192 px, w wąskim oknie 84 px), obroty nad procentem, strzałka. W wąskim oknie obroty i procent schodzą do podtytułu; gdy z nimi zdanie się nie mieści, zostaje samo zdanie. Kliknięcie wiersza otwiera „Szczegóły krzywej” z legendą wykresu i grupą „Złącze” (tryb, minimum, niesprawdzone i niekręcące się złącze). Przełącznik trybu, baner i tryb ręczny ze sterowaniem (M2b) dostaną projekt w tym układzie; punkty 5 i 6 opisują jeszcze układ A.
5. **Baner** „Zmiany działają także po zamknięciu aplikacji, do restartu komputera” z przyciskiem „Przywróć automat BIOS” widać tylko wtedy, gdy któryś kanał jest przejęty. Jest na stronie Wentylatory i na podstronie krzywej; to jedyny komunikat o tym, jak długo działają zmiany. Pasek nagłówka nie ma osobnego przycisku. Karty nie mają etykiety „Krzywa z BIOS-u”: tryb pokazuje przełącznik („Automat BIOS” to krzywa z BIOS-u, D11).
6. **Edytor krzywej:** duży wykres z 7 numerowanymi uchwytami (punkty o tych samych wartościach jako jeden uchwyt „6–7”), punktem pracy („Teraz”, z wypełnieniem po zastosowaniu), krzywą wykonywaną teraz przez kontroler (linia przerywana), strefą poniżej minimum (kreskowanie z opisem) i progiem awaryjnym źródła (linia, kolor i poziomy napis nad wykresem). Legenda tylko dla oznaczeń bez własnego podpisu. Pod wykresem lista punktów; wartości zmienia się przyciskami albo wpisuje z klawiatury. Zmiany zaczynają działać po „Zastosuj” na dolnym pasku, bo zapis wymaga autoryzacji polkit (D3).
7. **Wykres** ma odcinki proste, bo kontroler liczy wypełnienie liniowo między punktami (SYS_FAN1 przy 46 °C: 98/255, prosta między punktami daje 97,6). Poza skrajnymi punktami linia jest przerywana. Kolory: krzywa i uchwyty w akcencie systemu, punkt pracy w zieleni z ikony aplikacji, próg w kolorze błędu libadwaita; każde oznaczenie ma też opis albo wzór linii.
8. **Wydajność:** EPP i tryb sterownika są opisane po polsku, a surowa wartość jest w podtytule.

## Uwagi

- **Nazwa karty graficznej** pochodzi najpierw z `/sys/class/drm/card*/device/product_name`. Na tej maszynie amdgpu tego pliku nie tworzy, więc nazwa pochodzi z bazy PCI (hwdata). Baza podaje „Radeon RX 9050 / 9060 XT”, bo podsystemu PowerColor 148c:2437 nie ma na liście.
- **Moduły pamięci** nazywają się tak jak gniazda z bazy udev (DIMMA2, DIMMB2), bo adresy SPD 0x51 i 0x53 jednoznacznie wskazują obsadzone gniazda (§5.1, D8). Adres SPD stoi w podtytule. Bez takiego dopasowania aplikacja pokazuje „Moduł pamięci 1” i „Moduł pamięci 2”.
- **Opis grupy Procesor** to model z `/proc/cpuinfo` bez końcówki „8-Core Processor”; tak samo strona System (D38).
- **Pompa i wentylator dodatkowy** mają „złącze nietestowane” (na Przeglądzie w podtytule, na stronie „Szczegóły krzywej” w grupie „Złącze”), bo przypisanie złączy PUMP_SYS1 i JAF_1 do kanałów nie jest potwierdzone (O3).
- **Zakres min–maks** pojawia się dopiero wtedy, gdy wartości się rozejdą; „0–0 rpm” czy „12,10–12,10 V” nie stoją na stronie (D38).
- **Dyski** nazywają się tak jak model z sysfs, np. `CT2000P3PSSD8`. Przyjazne nazwy z `/etc/sensors.d` pojawiają się po włączeniu opcji etykiet z lm-sensors.
- **Strefy RGB są przykładowe.** Które złącza RGB są na płycie i jak odpowiadają strefom, pozostaje otwarte (O1).
- **Złącze pompy** pokazuje 0 rpm przy wypełnieniu 100 %, bo nic do niego nie jest podłączone. Aplikacja nie odróżni pustego złącza od zatrzymanej pompy, więc Przegląd o tym nie ostrzega. B11 reaguje tylko wtedy, gdy kanał wcześniej miał obroty.
- **Ikony** wentylatora, układu scalonego i innych elementów są narysowane na potrzeby makiety, bo Adwaita ich nie ma. Aplikacja będzie potrzebować własnych ikon symbolicznych.
