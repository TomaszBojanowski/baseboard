# Projekt: natywna aplikacja GNOME do monitoringu i sterowania płytą główną

Wersja 0.6 · 2026-11-10, plan M2 z 2026-11-11 · status: decyzje D1–D60 podjęte (§12)

Aplikacja nazywa się **Baseboard** (D1): App ID `me.bojanowski.Baseboard`, pliki binarne `baseboard` i `baseboard-helper`, pakiet i domena gettext `baseboard`.

## Zmiany względem 0.5

- **Wentylatory zaraz po M1.** Nowa kolejność: M1 Przegląd i System → M2 Wentylatory → M3 RGB → M4 Wydajność i restart do UEFI → M5 Aktualizacja BIOS-u. Dawne M4 i M5 (pomocnik, krzywe w EC) tworzą M2, a dawne M2, M3 i M3b przesuwają się o jeden numer (§7, D9). Numery etapów w całym dokumencie, także w D1–D8, są już nowe.
- **Warstwa wentylatorów oparta na ABI `nct6683`** z trzech poziomów pewności: przyjęte w `hwmon-next`, w recenzji (v1), nienapisane (§5.5.1).
- **Wykrywanie możliwości wyłącznie po atrybutach sysfs** (istnienie pliku, bit zapisu właściciela), osobno dla każdego kanału; nigdy po wersji jądra (§5.5.2). Co widzi użytkownik przy każdym sterowniku: §5.5.3.
- **Krzywe w EC zamiast krzywej programowej.** Krzywą wykonuje EC, a pomocnik tylko ją zapisuje. Krzywa programowa (pętla w pomocniku) jest odłożona (D10).
- **Zasady bezpieczeństwa dopasowane do sterownika:** kolejność zapisu punktów (B16), błędy zapisu (B17), krzywa pierwotna w `/run` (B18), zmiany w B2, B3, B5–B8, B10–B12 i B14; wymóg 100 % na progu tylko dla procesora i pompy (D19). Krzywa pierwotna wraca bez walidacji (B18) (§4).
- **Kontrakt D-Bus `Fans1`** przepisany pod możliwości kanałów i krzywe w EC (§3.5).
- **Jedna warstwa zależna od sterownika** (`core::fans::driver`, §3.7). Zmiany v1 po recenzji dotykają tylko jej (§7, M2, „Gdy recenzja v1 coś zmieni”).
- **M1 krok 4** obejmuje trzy warianty sterownika w próbkach testowych (§7, M1).
- **Plan przebudowy modułu testowego** v1 pod jądro 7.3.3 (w notatkach lokalnych, §1.4).
- **Przeliczanie wypełnienia jak w BIOS-ie** (D20, §3.5).
- Decyzje z tej wersji: D9–D20 (§12).

---

## 0. W skrócie

- Aplikacja w Rust (GTK4 + libadwaita, meson, gettext en/pl) działa bez roota. Pokazuje czujniki, steruje RGB i profilami wydajności, pokazuje informacje o płycie i firmware. Pomaga też przygotować aktualizację BIOS-u z pendrive'a, ale sama BIOS-u nie flashuje.
- Osobny pomocnik systemowy (D-Bus system bus, polkit) jako **jedyny** pisze do atrybutów wentylatorów w sysfs (`pwmN`, `pwmN_enable`, `pwmN_auto_point*`). Tryb ręczny oddaje automatowi przy wyjściu, awarii i przegrzaniu. Krzywe wykonuje sam EC.
- Wentylatorami sterujemy przez sterownik jądra `nct6683`: tryb ręczny z serii Hamzy i podgląd krzywych EC (oba w `hwmon-next`), zapis krzywych (nasza seria v1, w recenzji). Co jest dostępne, aplikacja wykrywa po atrybutach sysfs.
- Pierwsza płyta to MAG B850M MORTAR WIFI (MS-7E61). Wszystko, co zależy od płyty, jest zapisane w profilu (pliku danych), więc kolejną płytę dodaje się bez zmian w kodzie.
- Pięć kamieni milowych: M1 Przegląd i informacje o systemie → M2 Wentylatory (podgląd, tryb ręczny, krzywe w EC) → M3 RGB → M4 Wydajność i restart do UEFI → M5 Aktualizacja BIOS-u bez flashowania z systemu.

## Słownik

| Pojęcie | Znaczenie |
|---|---|
| EC | wbudowany kontroler Nuvoton NCT6687D: monitor sprzętu i sterowanie wentylatorami. W Linuksie obsługuje go sterownik `nct6683` (urządzenie hwmon o nazwie `nct6687`). |
| automat | `pwmN_enable = 2`: EC wykonuje bieżącą krzywą z `pwmN_auto_point*`. Po starcie komputera to krzywa z BIOS-u. |
| automat BIOS | automat z krzywą pierwotną, czyli niezmienioną przez Baseboard w tym uruchomieniu systemu (B18) |
| tryb ręczny | `pwmN_enable = 1`: stałe wypełnienie zapisane w `pwmN` |
| krzywa programowa | pomocnik co sekundę liczy wypełnienie z dowolnej temperatury i zapisuje je w trybie ręcznym. Odłożona (D10). |
| krzywa w EC | 7 punktów (temperatura, wypełnienie) w EC. EC sam liczy z nich wypełnienie dla temperatury źródła, bez pomocnika (M2c). |
| krzywa pierwotna | migawka krzywej i źródła kanału, którą pomocnik robi przed pierwszą zmianą w danym uruchomieniu systemu (B18) |
| poziom sterownika | co sterownik `nct6683` pozwala zrobić z kanałem, wykryte po atrybutach sysfs (§5.5.2) |
| wypełnienie | sygnał PWM: w sterowniku i w D-Bus 0–255, w interfejsie w procentach (0–100 %) |
| profil płyty | plik danych z mapą złączy, etykietami, rolami i limitami dla jednej płyty (§6) |

---

## 1. Cel i zakres

### 1.1 Cel

Linuksowy odpowiednik tych funkcji oprogramowania producenta, które dotyczą samej płyty: podgląd czujników, wentylatory, oświetlenie RGB, profile wydajności i informacje o firmware. Aplikacja ma być natywna dla GNOME (HIG, libadwaita, tryb ciemny, układ adaptacyjny) i korzystać wyłącznie ze standardowych interfejsów Linuksa: hwmon, libsensors, hidraw i usług D-Bus.

### 1.2 Zakres

| Obszar | Co robimy | Mechanizm |
|---|---|---|
| Monitoring | temperatury, obroty, wypełnienie, napięcia z oceną tolerancji | sysfs hwmon + libsensors |
| RGB | tryby sprzętowe stref, kolory pojedynczych diod (Direct), odczyt wersji firmware kontrolera | hidraw, raporty 0x50 i 0x51, raport odczytu wersji |
| Wydajność | przełączanie profilu energii, podgląd cpufreq | D-Bus `org.freedesktop.UPower.PowerProfiles`; sysfs tylko do odczytu |
| System | DMI, wersja BIOS-u, moduły pamięci, wersje firmware GPU i dysków, stan sterowników, restart do ustawień UEFI | sysfs, baza udev, logind |
| Aktualizacja BIOS-u (M5) | sprawdzenie nowszej wersji (opcja), pobranie z weryfikacją sumy kontrolnej, pendrive dla narzędzia aktualizacji w UEFI, restart do UEFI, lista kontrolna po aktualizacji. Bez flashowania z systemu. | HTTPS tylko na żądanie, UDisks2, logind |
| Wentylatory | podgląd krzywych EC (M2a), tryb ręczny (M2b), edycja krzywych EC (M2c), wybór źródła temperatury, gdy sterownik pozwoli (M2d) | sysfs hwmon `nct6683`; zapis przez pomocnika |

### 1.3 Czego NIE robimy

| Nie robimy | Dlaczego |
|---|---|
| Flashowanie BIOS-u z systemu (narzędziami producenta, przez LVFS, przez zapisanie z systemu polecenia dla narzędzia aktualizacji) | Ryzyko uszkodzenia płyty. Producent nie publikuje firmware w LVFS. Aktualizację wykonuje użytkownik wbudowanym narzędziem w UEFI (M-Flash) z pendrive'a, który aplikacja pomaga przygotować (M5, §5.6). |
| Aktualizację firmware EC, a także sprawdzanie jego wersji przez zapis do EC | Błędny zapis firmware EC odbiera sterowanie wentylatorami i zasilaniem. Wersję EC pokazujemy najwyżej z komunikatu sterownika. |
| OC i PBO: limity PPT/TDC/EDC, krzywe napięć, mnożniki, timingi i profile pamięci | Desktopowy Zen 5 nie ma do tego oficjalnego interfejsu w Linuksie. Ryzyko niestabilności i utraty gwarancji. |
| Raporty HID inne niż 0x50 (odczyt i zapis), 0x51 (zapis) i odczyt wersji firmware (§5.2) | Pozostałe raporty kontrolera RGB są nieudokumentowane. Część może przełączać mikrokontroler w tryb bootloadera. |
| Aktualizacja firmware kontrolera RGB, karty graficznej, kontrolerów USB4 i USB PD oraz innych układów płyty | Błędny zapis firmware uszkadza urządzenie. Wersje firmware pokazujemy tylko do odczytu, gdy daje je standardowy interfejs (sysfs) albo dozwolony raport (§5.2). |
| Dostęp do portów I/O, `/dev/port`, `/dev/mem`, rejestrów EC z przestrzeni użytkownika | Wyścig ze sterownikiem `nct6683` o okno EC. Korzystamy wyłącznie ze standardowego ABI jądra. |
| Zmiany ustawień BIOS-u (NVRAM) z systemu | Brak bezpiecznego, udokumentowanego interfejsu. |
| Odczyt rekordu OEM producenta w SMBIOS (typ 143) | Wymaga roota, a żadna funkcja aplikacji go nie potrzebuje. |
| Urządzenia spoza płyty: peryferia, AIO z własnym USB, sterowanie kartą graficzną | Poza zakresem. GPU i dyski pokazujemy tylko do odczytu: czujniki w Przeglądzie, wersje firmware na stronie System (§5.4). |
| Numery seryjne (płyty, systemu, modułów pamięci, dysków) i adresy MAC, na stronach i w tekście do schowka | Identyfikują komputer i właściciela, a żadna funkcja ich nie potrzebuje. Część jest dostępna bez roota (np. numery modułów pamięci w bazie udev), więc pomijamy je jawnie w kodzie. |
| Lista zainstalowanych programów i poprawek systemu, polecanie aplikacji | Prywatność; poza tematem płyty. |
| Skanowanie wersji sterowników i narzędzi oraz ich aktualizacja | Sterowniki są w jądrze, a programy aktualizuje menedżer pakietów dystrybucji. Skan wersji firmware jak w oprogramowaniu producenta wymagałby jego narzędzi i dostępu do kontrolerów poza ABI jądra. |
| Konto, telemetria, połączenia z siecią bez żądania użytkownika | Domyślnie aplikacja działa offline. Jedyny wyjątek to sprawdzenie i pobranie BIOS-u (M5, §5.6). Wymaga ono włączenia opcji w ustawieniach (domyślnie wyłączonej) i odbywa się tylko po kliknięciu, nigdy w tle ani przy starcie. Poza samym zapytaniem o listę i plik nic nie wysyłamy. Stronę pomocy producenta otwiera przeglądarka. |

### 1.4 Platforma docelowa (sprawdzona 2026-11-06, jądro 2026-11-10)

| Element | Wartość |
|---|---|
| Płyta | `board_name` = „MAG B850M MORTAR WIFI (MS-7E61)”, `board_version` = 1.0, BIOS 1.A68 (09/07/2026) |
| CPU | AMD Ryzen 7 9700X, `amd-pstate-epp` |
| System | Fedora 45, jądro 7.3.3-300.fc45, systemd 262, polkit 127 |
| Biblioteki | GTK 4.24.1, libadwaita 1.10.0, lm_sensors-libs 3.6.0 |
| Narzędzia | Rust 1.99.1, meson 1.12.1, gettext 1.0 |
| Usługi | tuned-ppd 2.28 (udostępnia `org.freedesktop.UPower.PowerProfiles`), logind: `CanRebootToFirmwareSetup` = yes |
| Sterowanie wentylatorami w jądrze | Sterownik z jądra Fedory 7.3.3: `pwm1–4` tylko do odczytu (0444), brak `pwmN_enable` i krzywych. Seria łatek do `nct6683` z `hwmon-next` (gałąź opiekuna hwmon; wersji jądra nie podajemy, dopóki zmiany nie są w drzewie Linusa, D41) daje na tej płycie `pwm1`, `pwm2`, `pwm13–16` z `pwmN_enable`; płyta ma customer ID 0x20D, które ta seria obejmuje. Sprawdzone modułem testowym 2026-11-06, łącznie z zapisem na `pwm16`. |
| Sterownik z naszymi łatkami (moduł testowy) | `hwmon-next` (`e0fdadf44480`) z serią v1 (`81b1e2c64f6d`, `527bbd00913c`): `pwm1`, `pwm2`, `pwm13–16` z `_enable`, `pwmN_auto_point[1-7]_{temp,pwm}` 0644, `pwmN_auto_channels_temp` 0444. Moduł pod 7.3.3 zbudowany i załadowany 2026-11-10. Budowanie, podpisywanie i ładowanie modułu spoza drzewa jądra do testów opisują notatki lokalne właściciela, poza repozytorium. |

---

## 2. Źródła wiedzy i czystość prawna

- Fakty o sprzęcie pochodzą z:
  1. testów na tej płycie,
  2. konfiguracji lm-sensors dla tej płyty (własny PR #582 do lm-sensors),
  3. źródeł publicznych: sterownik `nct6683` i jego dokumentacja, ABI `sysfs-class-hwmon`, publiczny opis protokołu RGB (zgłoszenia w OpenRGB), dokumentacja D-Bus power-profiles i logind,
  4. własnych notatek z analizy interoperacyjności oprogramowania producenta (art. 6 dyrektywy 2009/24/WE). Notatki leżą poza repozytorium,
  5. łatek do `nct6683` (seria Hamzy i nasze) z opisem w `nct6683.rst` oraz własnych notatek z projektu sterownika (poza repozytorium). Do aplikacji trafia z nich tylko ABI sysfs i zachowanie widoczne przez nie; adresów rejestrów EC aplikacja nie używa.
- Do repozytorium trafiają tylko fakty o interfejsach, opisane własnymi słowami, np. „`pwm16` steruje złączem SYS_FAN1”. Nie trafiają: kod, fragmenty dekompilacji, nazwy wewnętrznych klas, funkcji i plików producenta, klucze ani jego pliki danych.
- To samo dotyczy projektów otwartych o innych licencjach (OpenRGB, LibreHardwareMonitor, nct6687d). Korzystamy z opisu protokołu, kodu nie kopiujemy, a źródło podajemy w dokumentacji.
- Aplikacja nie potrzebuje adresów rejestrów EC. Wentylatorami steruje wyłącznie przez sysfs sterownika jądra. Prace nad sterownikiem (łatki do `nct6683`) to osobny projekt.
- Znaki towarowe: nazwa aplikacji, ikona i interfejs nie używają nazw ani oznaczeń producenta. Nazwę modelu płyty pokazujemy tak, jak podaje ją DMI, wyłącznie jako informację o sprzęcie.

---

## 3. Architektura

### 3.1 Komponenty

```
 Sesja użytkownika (bez roota)                         System
 ─────────────────────────────                         ──────
 baseboard (GTK4 + libadwaita)
   Przegląd ──── odczyt hwmon, libsensors ──────────►  nct6683, k10temp, amdgpu, nvme, spd5118
   RGB ───────── /dev/hidrawN (ACL z udev uaccess) ─►  kontroler RGB, USB HID 0DB0:0076
   Wydajność ─── D-Bus: UPower.PowerProfiles ───────►  tuned-ppd / power-profiles-daemon
   System ────── DMI, udev, sysfs; D-Bus: login1 ───►  systemd-logind
   Wentylatory ─ D-Bus: Baseboard.Helper ───────────►  baseboard-helper (root, polkit)
                 (obroty i temperatury czyta sama)       └─ zapis pwmN, pwmN_enable (nct6683)
```

### 3.2 Aplikacja

- **Język i biblioteki:** Rust (edycja 2024), gtk4-rs, libadwaita-rs, zbus (D-Bus), gettext-rs, serde + toml (profile). libsensors przez FFI, ładowany w czasie działania przez `dlopen` (`libsensors.so.5`, potem `.so.4` jak w Fedorze; crate `libc`), żeby brak biblioteki nie blokował startu. Baza udev tak samo przez `dlopen("libudev.so.1")` (D21). Cały kod `unsafe` siedzi w jednym module na bibliotekę (libsensors, libudev, ioctl hidraw) i w module lokalizacji `i18n` (`setlocale`, `nl_langinfo`, D24).
- **Budowanie:** meson wywołuje cargo i instaluje pliki danych: `.desktop`, metainfo, ikony, schemat GSettings, tłumaczenia, profile płyt.
- **Uprawnienia:** brak roota. Aplikacja czyta sysfs, otwiera hidraw dzięki ACL z reguły udev i rozmawia z usługami przez D-Bus.
- **Wątki:** interfejs w wątku głównym GTK. Odczyt czujników i operacje hidraw w wątkach roboczych z przekazywaniem wyników do pętli GLib. `SET_FEATURE` potrafi blokować przez kilka sekund, więc nigdy nie wykonujemy go w wątku interfejsu.
- **Układ:** `AdwApplicationWindow` z `AdwNavigationSplitView` (pasek boczny ze stronami). Strona niedostępna na danym sprzęcie pokazuje `AdwStatusPage` z wyjaśnieniem i tym, co zrobić.
- **Ustawienia (GSettings):** interwał odświeżania, źródło nazw odczytów (nazwy aplikacji albo etykiety z lm-sensors, §5.1), ukryte odczyty i złącza, ostatnia strona, sprawdzanie aktualizacji BIOS-u w sieci (domyślnie wyłączone, M5), ostatnio widziana wersja BIOS-u (do listy kontrolnej, §5.6). Od kroku 11 M1 są klucze `refresh-interval` (1–10 s, domyślnie 1), `sensors-labels` (domyślnie wyłączone) i `hidden-readings` (lista kluczy odczytów `układ/kanał@urządzenie`, np. `nct6687/fan2@devices/platform/nct6683.2592`; ścieżka urządzenia zamiast `hwmonN`, więc klucz przeżywa restart). Od F2 także `shown-readings`: klucze odczytów domyślnie ukrytych, które użytkownik włączył (D42). Okno preferencji (`AdwPreferencesDialog`, Ctrl+,) zapisuje każdą zmianę od razu, a okno aplikacji słucha GSettings i stosuje ją bez restartu: nowy interwał i źródło nazw dostaje wątek odczytu z natychmiastową próbką, ukryte odczyty znikają z bieżącej próbki. Interwał stoi z jednostką („1 s”; wpisać można „5” albo „5 s”). Wiersz „Własne nazwy” ma w jednej linii nazwę pliku (pełna ścieżka w podpowiedzi) i przycisk, który otwiera plik w domyślnej aplikacji, a gdy pliku jeszcze nie ma, jego katalog, tworzony w razie potrzeby (D38). Ostatnia strona dochodzi później.
- **Powiadomienia:** zdarzenia awaryjne pomocnika aplikacja pokazuje jako `GNotification`, o ile działa (pytanie O6, §13).

### 3.3 Pomocnik systemowy

- Osobny program `baseboard-helper` w `/usr/libexec`, uruchamiany przez systemd na żądanie z D-Bus system bus (`Type=dbus`), działa jako root.
- **Jedyny** element z prawem zapisu do `pwmN`, `pwmN_enable`, `pwmN_auto_point*` i `pwmN_auto_channels_temp`. Aplikacja nigdy nie pisze do sysfs.
- **Autoryzacja:** polkit (`CheckAuthorization` przez D-Bus, bez libpolkit).
- **Wejście:** wyłącznie klucze kanałów z profilu płyty (np. `sys1`), nigdy ścieżki. Ścieżki sysfs pomocnik wylicza sam: urządzenie hwmon znajduje po atrybucie `name` i ścieżce urządzenia, a nie po numerze `hwmonN`, który zmienia się między startami.
- **Profile** wczytuje tylko z katalogów systemowych (`/usr/share/baseboard/boards`, `/etc/baseboard/boards`), nigdy z katalogu użytkownika.
- **Czas życia:** startuje na żądanie. Kończy się po 30 s bezczynności, ale tylko wtedy, gdy żaden kanał nie jest w trybie ręcznym ustawionym przez pomocnika. Krzywe w EC nie potrzebują działającego pomocnika. Zamknięcie aplikacji nie zmienia ustawień wentylatorów. Blokada awaryjna (B4) jest zapisana w `/run/baseboard/emergency`, więc trwa po wyjściu i restarcie pomocnika, aż do jawnego zdjęcia albo restartu komputera (D48). Przy aktualizacji pakietu pomocnik jest uruchamiany ponownie (B3 a), a przy odinstalowaniu przywraca krzywe pierwotne i automat (`--restore-all`, D50).
- **Pętla nadzoru:** krok 1 s, tylko gdy któryś kanał jest w trybie ręcznym ustawionym przez pomocnika: progi awaryjne (B4), zatrzymany wentylator (B11), cudze zmiany (B10). Krzywej programowej nie liczy (D10). Sterownik odświeża odczyty mniej więcej raz na sekundę, więc częstszy krok nic nie daje.
- **Telemetria:** pomocnik jej nie przesyła. Aplikacja sama czyta obroty i temperatury z hwmon, a pomocnik wysyła tylko zmiany stanu.
- **Utwardzenie jednostki systemd:** `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateNetwork`, `RestrictAddressFamilies=AF_UNIX`, `SystemCallFilter=@system-service`, `WatchdogSec`. Zapis dozwolony tylko w katalogu urządzenia hwmon i w `/run/baseboard` (dziennik B2, krzywe pierwotne B18, blokada awaryjna B4). Docelowo pusty `CapabilityBoundingSet`, bo pliki `pwm*` mają właściciela root i tryb 0644 (do potwierdzenia w M2b).

### 3.4 Uprawnienia

| Operacja | Kto | Mechanizm | Autoryzacja |
|---|---|---|---|
| odczyt hwmon, DMI, sysfs urządzeń i bazy udev (bez numerów seryjnych i adresów MAC) | aplikacja | sysfs, libudev | brak |
| RGB | aplikacja | `/dev/hidrawN` | reguła udev z `TAG+="uaccess"`: ACL dla aktywnej sesji lokalnej |
| zmiana profilu energii | aplikacja → tuned-ppd | D-Bus | polkit `org.freedesktop.UPower.PowerProfiles.switch-profile` (aktywna sesja: bez hasła) |
| restart do ustawień UEFI | aplikacja → logind | D-Bus | polkit `org.freedesktop.login1.set-reboot-to-firmware-setup` i `…reboot` (aktywna sesja: bez hasła) |
| sprawdzenie i pobranie BIOS-u (M5) | aplikacja | HTTPS | brak; tylko po włączeniu opcji i kliknięciu |
| przygotowanie pendrive'a (M5) | aplikacja → UDisks2 | D-Bus | polkit UDisks2: montowanie nośnika wymiennego; formatowanie tylko po zgodzie użytkownika w aplikacji. Domyślne reguły UDisks2 dla obu akcji sprawdzimy w M5. |
| sterowanie wentylatorami | aplikacja → pomocnik | D-Bus + polkit | `me.bojanowski.Baseboard.fans.control`, `auth_admin_keep`: hasło administratora raz na kilka minut (D3) |
| powrót do automatu BIOS | aplikacja → pomocnik | D-Bus + polkit | `me.bojanowski.Baseboard.fans.restore-auto`, aktywna sesja bez hasła, bo to kierunek bezpieczny (D3) |

Uwaga o uaccess: każdy proces użytkownika aktywnej sesji może wysłać do kontrolera RGB dowolny raport, także te ryzykowne. Biała lista raportów działa więc tylko w naszej aplikacji, a nie w systemie. Tak samo działają reguły udev innych narzędzi RGB. Jeśli to okaże się za mało, RGB można później przenieść do pomocnika.

### 3.5 Kontrakt D-Bus pomocnika (zarys)

- Nazwa na magistrali: `me.bojanowski.Baseboard.Helper`
- Obiekt: `/me/bojanowski/Baseboard/Helper`
- Interfejs: `me.bojanowski.Baseboard.Fans1`. Cyfra w nazwie to wersja: zmiana niezgodna wstecz oznacza nowy interfejs `Fans2` obok starego. Dzięki temu aplikacja (np. z Flatpaka) i pomocnik (z RPM) mogą być w różnych wersjach.
- **Jednostki i przeliczanie** (D15, D20). To jedyne miejsce opisu reguły; w kodzie odpowiada mu jedna para funkcji w `core`.
  - W D-Bus i w sterowniku wypełnienie ma jednostki 0–255 (`y`), temperatura °C (`y`). Punkt krzywej ma typ `(yy)`: temperatura, wypełnienie. Procenty są tylko w interfejsie i w profilu (`min_duty`).
  - **Procent → 0–255:** obcięcie, tak jak w BIOS-ie: ⌊p · 255 / 100⌋. Przykłady: 30 % = 76, 50 % = 127, 20 % = 51, 40 % = 102.
  - **0–255 → procent w interfejsie:** zaokrąglenie do najbliższego pełnego procentu, połówki w górę: ⌊v · 100 / 255 + ½⌋. Przykłady: 76 → 30 %, 127 → 50 %, 124 → 49 %.
  - Każdy pełny procent 0–100 po obcięciu wraca więc w interfejsie do tego samego procentu. Minima (B5) porównujemy w jednostkach 0–255.

**Właściwości** (tylko do odczytu, z `PropertiesChanged`):

| Właściwość | Typ | Znaczenie |
|---|---|---|
| `ApiVersion` | `u` | wersja w obrębie `Fans1`; rośnie przy każdej nowej metodzie lub właściwości (§10.4): 1 = tryb ręczny (`GetChannels`, `GetState`, `SetManual`, `SetAuto`, `SetAllAuto`, `AcknowledgeEmergency`), 2 = krzywe (`GetEcCurve`, `SetEcCurve`, `RestoreOriginalCurve`), 3 = źródło (`SetEcSource`) |
| `HelperVersion` | `s` | wersja pakietu pomocnika, do strony System i zgłoszeń błędów (§10.4) |
| `BoardId` | `s` | klucz profilu (np. `ms-7e61`); pusty, gdy brak profilu |
| `ProfileDigest` | `s` | SHA-256 pliku profilu pomocnika; aplikacja porównuje go ze swoim (§10.4) |
| `DriverLevel` | `s` | najniższy poziom wspólny dla kanałów sterowalnych z profilu: `read-only`, `manual`, `manual+curve-view`, `curves`, `curves+source` (§5.5.2) |
| `Emergency` | `b` | blokada awaryjna aktywna (B4); trwała w `/run/baseboard` (D48) |
| `EmergencyReason` | `s` | czujnik i wartość, która ją wywołała |

**Metody:**

| Metoda | Sygnatura | Działanie | Polkit |
|---|---|---|---|
| `GetChannels` | `() → aa{sv}` | kanały z profilu: klucz, rola, numer, złącze, `fan`/`pwm`, czy sterowalny (profil oznacza kanał jako sterowalny i kanał ma `mode`, §5.5.2; rozstrzyga pomocnik, D33), minimum (0–255), czy przypisanie zweryfikowane, zapisy sprawdzone na sprzęcie (D16), **możliwości** (`as`: `duty-read`, `mode`, `duty-write`, `curve-read`, `curve-write`, `source-read`, `source-write`), liczba punktów krzywej | – |
| `GetState` | `() → aa{sv}` | stan kanałów: tryb (`auto`, `manual`), wypełnienie bieżące, wypełnienie zadane w trybie ręcznym, czy krzywa różni się od pierwotnej, czy kanałem steruje Baseboard | – |
| `SetManual` | `(s key, y duty) → y` | tryb ręczny. Zwraca faktycznie ustawione wypełnienie, po ewentualnym podniesieniu do minimum (B5). | control |
| `SetAuto` | `(s key) → ()` | `pwmN_enable = 2`; krzywa bez zmian | restore-auto |
| `GetEcCurve` | `(s key) → (s source, a(yy) points, a(yy) original, b modified)` | bieżąca krzywa, jej źródło (klucz temperatury z profilu albo `tempX`) i krzywa pierwotna (pusta przed pierwszą zmianą) | – |
| `SetEcCurve` | `(s key, a(yy) points, b activate) → a(yy)` | zapis krzywej w EC przez planer (B16) po walidacji (B7, B14); zwraca krzywą odczytaną po zapisie. `activate` = potem `pwmN_enable = 2` | control |
| `RestoreOriginalCurve` | `(s key) → ()` | zapis krzywej pierwotnej i automat; tylko planer B16, bez walidacji B5, B7 i B14 (B18) | restore-auto |
| `SetAllAuto` | `() → ()` | dla wszystkich kanałów zmienionych przez Baseboard: krzywa pierwotna (bez walidacji, jak wyżej) i automat. Przycisk „Przywróć automat BIOS” | restore-auto |
| `SetEcSource` | `(s key, s source) → s` | źródło temperatury krzywej; tylko przy `source-write` (M2d, D17) | control |
| `AcknowledgeEmergency` | `() → ()` | zdejmuje blokadę awaryjną (dopiero gdy temperatura spadła o histerezę) i ustawia automat na kanałach, które B4 ustawiła na 100 % (D48). Ustawień sprzed blokady nie przywraca: aplikacja wysyła je ponownie. | control |

**Sygnały:**

| Sygnał | Argumenty | Kiedy |
|---|---|---|
| `ChannelChanged` | `(s key, a{sv} state)` | zmiana trybu, wypełnienia zadanego albo krzywej |
| `EmergencyEntered` | `(s sensor, d value_c, d threshold_c)` | przekroczenie progu albo błąd odczytu (B4) |
| `EmergencyCleared` | `()` | po `AcknowledgeEmergency` |
| `ControlLost` | `(s key, s reason)` | Baseboard przestał sterować kanałem: `external-change`, `read-back-failed`, `stall`, `sensor-error`, `resume-failed`, `driver-error`, `driver-reloaded` (B3 g, D49) |

**Błędy:** `me.bojanowski.Baseboard.Error.NotAuthorized`, `.NotSupported` (także brak potrzebnej możliwości kanału), `.UnknownChannel`, `.InvalidArgument`, `.EmergencyActive`, `.HardwareError`, `.DriverRejected` (sterownik zwrócił `-EIO`: EC nie przyjął zapisu albo stan EC zmienił ktoś inny), `.Timeout` (`-ETIMEDOUT`: EC nie odpowiedział na uzgadnianie).

### 3.6 Planowana struktura repozytorium

```
CLAUDE.md                   zasady pracy w repozytorium (zasady lokalne w CLAUDE.local.md, poza repozytorium)
README.md
LICENSE                     GPL-3.0
.gitignore
Cargo.toml, Cargo.lock      workspace i zablokowane wersje zależności
meson.build
meson.options               gdy dojdą opcje
crates/
  core/                     profile płyt, model czujników, typy D-Bus, walidacja (wspólna dla aplikacji i pomocnika)
  app/                      aplikacja (binarka baseboard): build.rs (pakiet zasobów), meson.build,
                            src/ z plikami .ui, style.css, baseboard.gresource.xml i icons/ (ikony wewnętrzne)
  helper/                   pomocnik (binarka baseboard-helper)
data/
  boards/ms-7e61.toml       profil płyty
  me.bojanowski.Baseboard.desktop.in, .metainfo.xml.in, .gschema.xml, icons/
  dbus/                     plik usługi system-services i polityka system.d
  polkit/                   me.bojanowski.Baseboard.policy.in
  systemd/                  baseboard-helper.service
  udev/                     60-baseboard-rgb.rules
po/                         LINGUAS, POTFILES.in, pl.po
build-aux/flatpak/          manifest Flatpaka (me.bojanowski.Baseboard.json), źródła Cargo dla budowania bez sieci (§10.6)
packaging/rpm/              baseboard.spec
tests/fixtures/             sztuczne drzewa sysfs (warianty sterownika z plikiem trybów) i przykładowe raporty
docs/
makiety/                    makiety interfejsu: HTML, OPIS.md, renderuj.sh (PNG w png/, poza repozytorium)
website/                    strona projektu (D41): HTML i CSS bez skryptów, angielska pod /, polska pod /pl/, zrzuty strony w img/,
                            zrzuty metainfo w screenshots/<wersja>/ (stałe adresy, nie zmieniamy ich, D43)
.github/workflows/          website.yml: wdrożenie website/ na GitHub Pages po zmianach na main (D41)
```

Walidacja bezpieczeństwa leży w `core`, więc aplikacja i pomocnik w tej samej wersji liczą to samo. Decyduje zawsze pomocnik; przy różnych wersjach (aplikacja z Flatpaka, pomocnik z pakietu systemowego) wynik aplikacji jest tylko podpowiedzią w interfejsie (§10.4).

### 3.7 Warstwa sterownika wentylatorów

Cała wiedza o ABI `nct6683` siedzi w jednym module `core::fans::driver`, za cechą (trait) `FanDriver`:
- wykrywanie możliwości kanałów z atrybutów (§5.5.2),
- odczyt trybu, wypełnienia, krzywej i źródła,
- zapis trybu, wypełnienia, punktów krzywej i źródła,
- przeliczenia jednostek (milistopnie ↔ °C) i granice sterownika,
- mapowanie błędów (`-EIO`, `-ETIMEDOUT`, `-EBUSY`, `-EINVAL`) na błędy pomocnika,
- planer kolejności zapisu krzywej (B16).

Implementacje:
- `SysfsDriver`: prawdziwy sysfs. Aplikacja używa go tylko do odczytu, pomocnik także do zapisu.
- `ModelDriver`: model zachowania sterownika w pamięci, do testów: przycinanie temperatur i wypełnień do sąsiednich punktów, `-EIO` z przywróceniem poprzedniej krzywej, `-ETIMEDOUT` na żądanie, wyjście w automacie dochodzące do celu stopniowo (tempo to parametr modelu; domyślnie 5 jednostek/s, pomiar z SYS Fan 1), utrata trybu ręcznego po uśpieniu z odtworzeniem albo bez.

Pomocnik, kontrakt D-Bus, zasady §4 i interfejs korzystają tylko z cechy. Zmiana sterownika, np. po recenzji v1, zmienia `SysfsDriver`, `ModelDriver` i próbki w `tests/fixtures/`, a reszta zostaje (§7, M2, „Gdy recenzja v1 coś zmieni”).

---

## 4. Bezpieczeństwo wentylatorów

Zasady obowiązują od M2b (pierwszy zapis przez pomocnika). Testy odwołują się do ich numerów. Złagodzenie którejkolwiek wymaga osobnej decyzji zapisanej w tym dokumencie. Względem 0.5 zmienione są B2, B3, B5–B8, B10–B12 i B14, nowe B16–B18. Złagodzenia wymagające decyzji: usunięcie B3(c) (D13), wymóg 100 % na progu tylko dla procesora i pompy (D19) i niemalejące zamiast rosnących temperatury punktów krzywej (D31). Zaostrzenia z planu M2: B3(g) (D49) i B4 (D48).

**Zachowanie sterownika, na którym opierają się zasady** (poziomy pewności w §5.5.1):
- Sterownik **nie ma minimum** wypełnienia, także dla pompy. Polityka B5 i B6 jest wyłącznie w pomocniku.
- Zapis punktu krzywej przycina temperaturę do 0–127 °C, wypełnienie do 0–255, a obie wartości do sąsiednich punktów. Przycięcie nie jest błędem: zapis się udaje, ale w EC trafia inna wartość.
- Krzywej o malejącym wypełnieniu nie da się zapisać przez sysfs: sterownik przycina wypełnienie do sąsiednich punktów. Gdy zapis się nie uda, kończy się `-EIO` i obowiązuje poprzednia krzywa.
- Zapis jednego punktu to jedna transakcja z EC dla całej krzywej (ok. 30 ms), z weryfikacją odczytem. Gdy EC zapisu nie przyjmie albo odczyt się nie zgadza, sterownik przywraca krzywą sprzed zapisu i zwraca `-EIO`. Zwraca też `-EIO` bez zapisu, gdy stan EC zmienił ktoś inny, a `-ETIMEDOUT`, gdy EC nie odpowiada na uzgadnianie.
- Zapis krzywej w trybie ręcznym zmienia krzywą w EC, ale wentylator zostaje przy wypełnieniu ręcznym. Nowa krzywa działa po `pwmN_enable = 2`.
- W automacie wyjście dochodzi do celu stopniowo. **Pomiar** na SYS Fan 1 (`pwm16`) przy jednym ustawieniu Step Up/Down w BIOS-ie (0,1 s): ok. 5 jednostek na sekundę (ok. 2 %/s) w górę i w dół, czyli pełny zakres w ok. 50 s. Pomiar z 2026-11-11 pod obciążeniem (M2a krok 2): SYS Fan 1 i 2 ok. 5 jednostek/s, CPU Fan 1 (w BIOS-ie 0,3 s) ok. 3,3 jednostki/s, czyli ze 124 do 255 w ok. 40 s; w obu kierunkach tak samo. Pompy (złącze puste) i innych ustawień Step Up/Down nie mierzyliśmy (O15). W trybie ręcznym zmiana jest natychmiastowa.
- Po uśpieniu (S3) **potwierdzone jest tylko to, że BIOS cofa tryb ręczny**: po wznowieniu EC jest w automacie. Czy BIOS nadpisuje też krzywe, na tej płycie nie da się rozróżnić: krzywa zapisana przez sterownik z v1 była po S3 taka jak przed uśpieniem, ale mógł ją zachować EC albo odtworzyć sterownik (O4). Sterownik z `7dc9dff9608d` odtwarza tryb ręczny, a v1 zapisane krzywe, gdy EC się od nich różni; starszy sterownik nie odtwarza niczego. Po restarcie komputera EC ma ustawienia BIOS-u.
- Przy wyładowaniu sterownik niczego nie przywraca: ostatni tryb i krzywa zostają w EC do ponownego załadowania albo restartu (sterownik w `hwmon-next` nie ma `.remove` ani `.shutdown`, które coś przywracają; serii, która by to zmieniła, nie ma w żadnej gałęzi, §7 M2, M2.2). Po ponownym załadowaniu działa B3(g).
- Między punktami krzywej EC liczy wypełnienie liniowo i zaokrągla w górę do pełnego kroku 0–255; poniżej pierwszego i powyżej ostatniego punktu obowiązuje ich wypełnienie. Sprawdzone 2026-11-11 na sześciu kanałach (M2a krok 2, `core::fans::curve`): 939 s odczytów co sekundę przy temp1 42–73 °C, każda ustalona próbka równa modelowi; SYS Fan 1 przechodzi między 98 a 99, a SYS Fan 2 między 108 a 109 dokładnie tam, gdzie wypada zaokrąglenie w górę (zwykłe zaokrąglenie dałoby inne progi, a dla CPU Fan 1 przy 43–56 °C 123 zamiast odczytanego 124). Po zmianie celu o 1 krok odczyt `pwmN` dochodzi do niego w 0–2 s. Pod obciążeniem `stress-ng` (60 s, temp1 do 86 °C) każda ustalona próbka znów równa modelowi. EC liczy z pełnych stopni, tak jak podaje je temp1: na najbardziej stromym odcinku (SYS Fan 1 i 2, 84 → 85 °C, 156 kroków na stopień) przy temp1 = 84 °C w czasie nagrzewania wypełnienie trzymało dokładnie 99 i 109 i ruszyło dopiero przy pierwszym odczycie 85 °C; z dokładniejszej temperatury rosłoby już przy 84 °C. Środka stromego odcinka CPU Fan 1 (75–85 °C) nie dało się porównać w stanie ustalonym, bo temperatura rosła szybciej, niż wentylator dochodzi do celu; wyjście szło jednak stale w stronę wartości modelu i zatrzymało się dokładnie na niej (255 przy 85 °C, 124 po ostygnięciu). Przy odczycie co 1 s wartości zmieniały się co drugą próbkę.

| # | Zasada |
|---|---|
| B1 | **Tylko pomocnik pisze do `pwm*`**, w tym do `pwmN_auto_point*` i `pwmN_auto_channels_temp`. Aplikacja, testy i skrypty developerskie nie piszą do sysfs. |
| B2 | **Dziennik.** Przed pierwszą zmianą kanału pomocnik zapisuje w `/run/baseboard/claimed` klucz kanału i rodzaj zmiany (`manual`, `curve`, `source`), a przed pierwszą zmianą krzywej lub źródła także krzywą pierwotną (B18). Dopiero potem pisze do sysfs. Tryb ręczny: najpierw `pwmN_enable = 1`, potem `pwmN`. Zawsze w tej kolejności, bo `pwmN` przyjmuje zapis tylko w trybie ręcznym. |
| B3 | **Powrót do automatu (`pwmN_enable = 2`)** dla każdego kanału w trybie ręcznym ustawionym przez pomocnika: (a) przy normalnym zakończeniu (SIGTERM; z bezczynności pomocnik w tym stanie się nie kończy); (b) przy awarii (panic z `panic = "abort"`, SIGKILL, OOM, przekroczenie watchdoga). Wtedy przywraca `ExecStopPost=baseboard-helper --restore-auto`, które czyta dziennik B2 i nie potrzebuje D-Bus; (c) *usunięte w 0.6* (przed uśpieniem; zastępuje je B12, D13); (d) przed wyłączeniem i restartem (`PrepareForShutdown`); (e) po przekroczeniu progu awaryjnego (B4, najpierw 100 %, automat po zdjęciu blokady, D48); (f) po błędzie odczytu temperatury lub obrotów; (g) po ponownym pojawieniu się urządzenia hwmon (przeładowanie modułu, D49): kanały z dziennika B2 w trybie ręcznym wracają do automatu, a krzywe pomocnik porównuje z migawką B18 i z tym, co sam zapisał, bez nadpisywania (różne → `ControlLost: external-change`, w każdym razie `ControlLost: driver-reloaded` dla kanałów z dziennika). Aktualizacja pakietu uruchamia pomocnika ponownie, czyli przechodzi przez (a) (D50). **Krzywych w EC B3 nie cofa:** działają same, są bezpieczne z założenia (B14) i do restartu komputera zostają (D4). Po awarii pomocnik **nie** wznawia trybu ręcznego sam. Aplikacja pokazuje komunikat i czeka na decyzję użytkownika. |
| B4 | **Próg awaryjny** (D48). Gdy temperatura z listy progów przekroczy próg w 3 kolejnych odczytach (temperatura procesora: osiągnie go, ≥ 95 °C, bo procesor przy Tjmax dławi się i temp1 może stanąć dokładnie na progu), pomocnik od razu ustawia **wszystkie** kanały w trybie ręcznym ustawionym przez Baseboard na 100 % (tryb ręczny, 255) i włącza blokadę zapisaną w `/run/baseboard/emergency`, która trwa po wyjściu i restarcie pomocnika. Do jej zdjęcia trzeba jawnej akcji (`AcknowledgeEmergency`), a temperatura musi spaść o co najmniej 10 °C poniżej progu; po zdjęciu te kanały przechodzą w automat. Wartość nieodczytywalna lub niewiarygodna (< 0 °C, > 125 °C, brak pliku) przez 3 odczyty działa jak przekroczenie progu. Kanały z krzywą w EC już są w automacie: krzywe procesora i pompy osiągają 100 % najpóźniej na progu (B14), krzywe wentylatorów systemowych nie muszą (D19), a B4 ich nie zmienia. Pełne wypełnienie od razu, a nie automat, bo w automacie wyjście dochodzi do celu stopniowo: na SYS Fan 1 i 2 zmierzono ok. 5 jednostek/s (z 30 % do 100 % w ok. 36 s), na CPU Fan 1 ok. 3,3 jednostki/s (ze 124 do 255 w ok. 40 s), na pompie tempo jest nieznane (O15). Rozstrzyga D14. |
| B5 | **Minimalne wypełnienie** w jednostkach 0–255, przeliczone z procentów z obcięciem jak w BIOS-ie (§3.5, D20): CPU co najmniej 30 % (76), pompa co najmniej 50 % (127) (§6.3). Dotyczy trybu ręcznego i **każdego punktu krzywej**. Za niskie żądanie trybu ręcznego pomocnik podnosi do minimum i zwraca faktycznie ustawioną wartość; krzywą odrzuca (B7). W kodzie są stałe dolne granice, przeliczone tak samo (CPU 20 % = 51, pompa 40 % = 102). Profil może je tylko podnieść, więc nawet błędny profil nie zejdzie niżej. Sterownik minimum nie ma, więc to jedyna ochrona. |
| B6 | **Pompa nigdy nie staje.** Żądanie 0 dla roli `pump`, w trybie ręcznym albo w punkcie krzywej, kończy się błędem `InvalidArgument`, a nie podniesieniem do minimum, żeby interfejs nie sugerował, że pompę można zatrzymać. Interfejs nie ma też przełącznika „zatrzymaj”. Złącze pompy traktujemy jak pompę, także gdy podłączono do niego zwykły wentylator. |
| B7 | **Walidacja wejścia.** Tylko klucze kanałów z profilu, które mają potrzebną możliwość (§5.5.2). Krzywa w EC ma dokładnie tyle punktów, ile podaje sterownik (dziś 7); temperatury niemalejące (D31) w zakresie 0–100 °C, wypełnienie niemalejące, każdy punkt co najmniej minimum B5. Pomocnik **nie polega** na przycinaniu przez sterownik: krzywą spoza reguł odrzuca (`InvalidArgument`). Źródło temperatury tylko z listy temperatur EC w profilu (M2d). **Wyjątek:** krzywa pierwotna (B18) i powrót w B17(b) nie przechodzą tej walidacji. |
| B8 | **Łagodne zmiany.** Tryb ręczny: zapis najwyżej raz na sekundę na kanał i tylko przy zmianie o co najmniej 1 jednostkę; suwak w interfejsie łączy zmiany. Zmiana wypełnienia najwyżej o 10 p.p. na sekundę w dół; w górę bez limitu. Krzywa: jeden zapis planu na żądanie użytkownika, najwyżej raz na 2 s na kanał. W automacie EC sam wygładza wyjście (pomiar wyżej). |
| B9 | **Sterowanie tylko na znanym sprzęcie:** profil pasuje do płyty (łącznie z rewizją PCB), urządzenie hwmon ma oczekiwaną nazwę, a kanał ma potrzebną możliwość z atrybutów (§5.5.2). Bez profilu pomocnik niczym nie steruje (`NotSupported`). Zapis na kanale, którego zapisu nie sprawdzono na sprzęcie, zależy od D16. |
| B10 | **Weryfikacja i cudze zmiany.** Po zapisie trybu pomocnik odczytuje `pwmN_enable`, po zapisie wypełnienia ręcznego `pwmN` (zmiana jest natychmiastowa), po zapisie krzywej wszystkie punkty, które porównuje z planem. Jeśli odczyt trybu albo wypełnienia się nie zgadza, kanał wraca do automatu (`ControlLost: read-back-failed`). Niezgodna krzywa → B17. Pętla nadzoru i każde wywołanie D-Bus porównują stan kanałów zmienionych przez Baseboard z tym, co ustawił. Tryb zmieniony przez inny program albo krzywa inna niż zapisana (bez uśpienia po drodze) → pomocnik przestaje sterować kanałem i go **nie** nadpisuje (`ControlLost: external-change`). |
| B11 | **Zatrzymany wentylator.** Gdy kanał `cpu` lub `pump` pokazywał wcześniej obroty, a przy wypełnieniu ≥ minimum spadnie do 0 rpm na 5 s, wszystkie kanały w trybie ręcznym ustawionym przez Baseboard wracają do automatu (`ControlLost: stall`) i pojawia się ostrzeżenie. „Wypełnienie” to odczyt `pwmN`, a nie cel, bo w automacie wyjście dochodzi do celu stopniowo. |
| B12 | **Po wybudzeniu** pomocnik czeka 5 s i porównuje stan kanałów z tym, co ustawił: tryb ręczny i wypełnienie, krzywą, źródło. Zgodne → nic (sterownik odtworzył stan). Niezgodne → ponowne nałożenie z weryfikacją B10 (sterownik bez odtwarzania); gdy się nie uda, automat i krzywa pierwotna (`resume-failed`). Pomocnik nie zakłada, że sterownik odtwarza stan, bo po atrybutach tego nie widać. Ustawienia nie przeżywają restartu komputera (D4). |
| B13 | **Watchdog systemd.** Pętla nadzoru wysyła `WATCHDOG=1`. Jej zawieszenie kończy się zabiciem procesu i krokiem B3(b). |
| B14 | **Krzywe w EC działają bez nadzoru**, więc każda krzywa zapisywana na żądanie użytkownika musi być bezpieczna sama w sobie. **Role `cpu` i `pump`:** każdy punkt spełnia B5 i B6, a wypełnienie 255 (100 %) jest osiągnięte najpóźniej przy temperaturze progu awaryjnego źródła tej krzywej (B4); źródło bez progu w profilu → zapis krzywej odrzucony (`NotSupported`). **Pozostałe role (`sys`, `aux`):** wystarczy krzywa poprawna według B7 (niemalejąca, w zakresie), bez wymogu 100 % na progu (D19). Na MS-7E61 źródłem wszystkich krzywych BIOS-u jest temp1, czujnik procesora (próg 95 °C). Krzywej pierwotnej B14 nie dotyczy (B18). |
| B15 | **Dziennik.** Każde przejęcie, zmiana trybu, zapis krzywej (plan i wynik), powrót do automatu i zdarzenie awaryjne trafiają do dziennika systemd z kluczem kanału i przyczyną. |
| B16 | **Kolejność zapisu krzywej.** Pomocnik nigdy nie pisze punktów w kolejności, która uruchomiłaby przycinanie w sterowniku. Planer liczy obwiednię: temperatury = minimum ze starej i nowej krzywej, wypełnienia = maksimum. Faza 1: wypełnienia do obwiedni od punktu 7 do 1, temperatury do obwiedni od 1 do 7. Faza 2: temperatury do nowych od 7 do 1, wypełnienia do nowych od 1 do 7. Punkty bez zmian pomijamy. Każdy stan pośredni jest krzywą niemalejącą, a w każdej temperaturze daje wypełnienie nie mniejsze niż stara i nowa krzywa. Dla samego podniesienia wychodzi z tego kolejność 7→1, dla samego obniżenia 1→7. |
| B17 | **Błąd zapisu krzywej.** Przy `-EIO` albo `-ETIMEDOUT` w trakcie planu pomocnik przerywa i czyta krzywą z EC: (a) poprawna i spełnia B14 → zostaje, a pomocnik zwraca błąd z odczytaną krzywą; (b) inaczej powrót do poprzedniej krzywej (zapisanej wcześniej przez użytkownika albo pierwotnej) tym samym planerem, bez ponownej walidacji B5, B7 i B14; (c) gdy i to zawiedzie, automat i `ControlLost: driver-error`. Dzięki B16 i przywracaniu przez sterownik każdy możliwy do zaobserwowania stan krzywej jest bezpieczny. |
| B18 | **Krzywa pierwotna.** Przed pierwszą zmianą krzywej lub źródła kanału w danym uruchomieniu systemu pomocnik zapisuje jego krzywą i źródło w `/run/baseboard/original-curves.json` (tmpfs; po restarcie BIOS i tak wpisuje swoje). „Przywróć automat BIOS”, `RestoreOriginalCurve`, `SetAllAuto` i powrót w B17(b) zapisują ją z powrotem **tylko planerem B16, bez walidacji B5, B7 i B14**, i ustawiają automat. Krzywa pierwotna to stan sprzed Baseboard, zwykle z BIOS-u, i może tych zasad nie spełniać (§6.3): przywrócenie oznacza powrót do ustawień firmware, a nie nową krzywą. Gdy przed migawką krzywą zmienił inny program, „pierwotna” znaczy „sprzed pierwszej zmiany przez Baseboard” (D12). |

**Ryzyko szczątkowe:** jeśli zawiesi się całe jądro, EC dalej wykonuje ostatni tryb, bo sam nie ma watchdoga. W trybie ręcznym oznacza to stałe wypełnienie aż do restartu, w automacie ostatnią krzywą. Tak samo między wyładowaniem a ponownym załadowaniem modułu: pomocnik bez sterownika niczego nie zapisze. Dlatego minimum B5 i zasada B14 są twarde. Sytuacje szczególne (awaria pomocnika, zamknięcie aplikacji, uśpienie, wyłączenie, przeładowanie modułu, aktualizacja i odinstalowanie pakietu) i to, czego nie da się sprawdzić w pełni: §7 M2, M2.5.

**Progi awaryjne** (wartości wstępne, do weryfikacji testami obciążeniowymi w M2b). Administrator może je obniżyć w `/etc/baseboard/`, aplikacja nie może ich zmienić.

| Czujnik | Źródło | Próg |
|---|---|---|
| CPU | `nct6687` temp1, czujnik procesora w układzie płyty (D34); Tctl z `k10temp` tylko do porównania w teście obciążeniowym M2b (O10) | 95 °C (D5). Ryzen 9000 pracuje normalnie do Tjmax 95 °C; pod pełnym obciążeniem z krzywą BIOS zmierzono 89 °C, więc próg 90 °C włączałby się przy zwykłym obciążeniu. |
| MOS (sekcja zasilania) | `nct6687` temp3 | 90 °C |
| System, Chipset A | `nct6687` temp2, temp4 | 80 °C |

---

## 5. Strony aplikacji

### 5.1 Przegląd (M1)

- **Odczyty:** wszystkie układy hwmon przez libsensors, z `ignore` i `compute` z `/etc/sensors.d`. Dla płyty z profilem odczyty są pogrupowane: Procesor, Płyta, Wentylatory, Napięcia. Pozostałe układy grupujemy po sterowniku: `k10temp` → Procesor, `amdgpu` → Karta graficzna, `nvme` → Dyski, `spd5118` → Pamięć, reszta → Inne.
- **Własne nazwy** odczytów płyty z pliku użytkownika (D22, format w §6.2) mają pierwszeństwo przed wszystkim niżej, także przed etykietami z lm-sensors: użytkownik nazwał odczyt wprost.
- **Nazwy odczytów, domyślna kolejność pierwszeństwa:**
  1. profil płyty: nazwa z roli i numeru, tłumaczona przez gettext (§6.2), np. „Wentylator procesora 1”, „Pompa 1”, „+3,3 V”. Nazwa złącza z instrukcji płyty (np. `CPU_FAN1`) stoi w podtytule;
  2. nazwy wbudowane w aplikację, tłumaczone przez gettext, dla znanych etykiet sterowników i typów urządzeń: `k10temp` (Tctl → „Procesor (Tctl)”, Tccd*N* → „Chiplet CCD*N*”), `amdgpu` (edge → „Układ graficzny”, junction → „Najgorętszy punkt”, mem → „Pamięć”, vddgfx → „Napięcie rdzenia”, PPT → „Pobór mocy”, fan1 → „Wentylator”), `nvme` (model dysku z sysfs; numerowane czujniki „Sensor *N*” jako „model · Czujnik temperatury *N*”, D42), `spd5118` (nazwa gniazda z bazy udev, np. „DIMMA2”, gdy adres SPD 0x50+*i* jednoznacznie odpowiada gniazdu *i* z `MEMORY_DEVICE_i_LOCATOR`: wszystkie czujniki na jednej magistrali SMBus, tyle czujników co obsadzonych gniazd, a każdy adres wskazuje inne obsadzone gniazdo z nazwą; w przeciwnym razie wszystkie moduły to „Moduł pamięci *N*”), karta Wi-Fi i karta sieciowa (rozpoznane po urządzeniu nadrzędnym), napięcia pomocnicze `nct6683` (VSB → „Zasilanie czuwania (VSB)”, VBAT → „Bateria podtrzymująca (VBAT)”; tylko tam, gdzie znaczenie jest pewne, więc AVSB, VTT i VREF zostają skrótami, D38);
  3. etykieta sterownika, a bez niej nazwa kanału (np. `temp1`).

  Podtytuł pokazuje źródło (sterownik i kanał albo etykietę sterownika, np. `amdgpu · edge`), żeby odczyt dało się porównać z `sensors`. Podtytuł nie powtarza tytułu (D38): gdy nazwą jest etykieta sterownika albo nazwa ją zawiera, podtytuł podaje sterownik i kanał („VSB” → `nct6687 · in9`); gdy nazwą jest sam kanał, tylko sterownik. Sterownik to nazwa urządzenia hwmon bez końcówek: odpada każda końcowa część po „_”, która zawiera cyfrę albo brzmi `hwmon` (`r8169_0_d00:00` → `r8169`, `ath12k_hwmon` → `ath12k`). To jedna zasada dla wszystkich układów, bez listy wyjątków.
- **Etykiety z lm-sensors** to opcja w ustawieniach, domyślnie wyłączona. Po włączeniu etykieta z konfiguracji lm-sensors ma pierwszeństwo przed punktami 1–3 i aplikacja pokazuje te same nazwy co polecenie `sensors`. `ignore` i `compute` z konfiguracji obowiązują zawsze, niezależnie od tej opcji.
- **Bez powtórzeń:** gdy nazwa urządzenia stoi w opisie grupy (np. nazwa karty graficznej), wiersze jej nie powtarzają: „Układ graficzny”, a nie „RX 9060 XT rdzeń”.
- **Nazwa karty graficznej** w opisie grupy: najpierw `/sys/class/drm/card*/device/product_name` (amdgpu podaje ją tylko dla części kart), a gdy pliku nie ma albo jest pusty, nazwa z bazy sprzętu udev (hwdb, właściwość `ID_MODEL_FROM_DATABASE`, która uwzględnia identyfikatory podsystemu; jej treść pochodzi z `pci.ids`). Własnego parsera `pci.ids` nie piszemy. Z wpisu bazy pokazujemy część z nawiasu kwadratowego („Navi 44 [Radeon RX 9050 / 9060 XT]” → „Radeon RX 9050 / 9060 XT”), a bez nawiasu cały wpis (D23). Tak samo nazywamy karty sieciowe (§5.4).
- **Opis grupy Procesor:** model z `/proc/cpuinfo` (`model name`) bez końcówki „*N*-Core Processor”, np. „AMD Ryzen 7 9700X”. Tę samą krótką nazwę, z tej samej funkcji, pokazuje strona System i tekst do schowka (D38).
- **Dyski** w kolejności numerów urządzeń: `nvme0` przed `nvme1`, a ciągi cyfr porównujemy jako liczby (`nvme2` przed `nvme10`). Tak samo na stronie System (D38).
- **Źródło odczytów:** sysfs, na który libsensors nakłada swoje wartości, etykiety i `ignore`. Wypełnienie `pwmN` i układy, których libsensors nie zgłasza, pochodzą z samego sysfs. Gdy biblioteki nie ma albo `sensors_init` zawiedzie, aplikacja czyta sam sysfs, bez komunikatu dla użytkownika. Tak samo przy podmienionym katalogu sysfs (tylko debug), bo libsensors czyta zawsze `/sys`. Błędną linię w `/etc/sensors.d` libsensors pomija z komunikatem na stderr, a reszta konfiguracji działa.
- **Bez podwójnego przeliczania.** Jeśli wartość z libsensors różni się od surowej wartości z sysfs, konfiguracja ma już `compute` i mnożnika z profilu nie stosujemy. Jeśli są równe, stosujemy mnożnik z profilu. Sprawdzamy to tylko dla szyn napięcia z profilu (tylko do nich stosujemy mnożnik), raz, przy starcie libsensors: w trzech rundach (sysfs, libsensors, sysfs) szyna ma `compute`, gdy w każdej rundzie wartość z libsensors różni się od surowej i przed, i po. Sterownik płyty trzyma odczyt przez sekundę, więc runda widzi najwyżej jedną zmianę. Odczyty innych układów zmieniają się między dowolnymi dwoma odczytami (temperatura chipletu o stopnie, moc karty o waty w ciągu milisekund), więc tam takie porównanie nic nie mówi.
- **Wentylatory:** obroty i wypełnienie (`pwmN`, jeśli czytelne) z nazwą złącza. Kolejność według złączy, a nie numerów `fanN`, bo SYS są w EC numerowane odwrotnie. Złącza bez podłączonego wentylatora można ukryć. Złącze, którego przypisania nie sprawdzono na płycie (`verified = false`), ma w podtytule dopisek „złącze nietestowane”, krótki, żeby podtytuł z wypełnieniem mieścił się w jednej linii (D38).
- **Ukryte odczyty i złącza:** w preferencjach strona „Odczyty” ma przełącznik dla każdego odczytu, w grupach jak na Przeglądzie, z nazwą, podtytułem i wartością z chwili otwarcia (np. 0 rpm przy złączu bez wentylatora). Wyłączony odczyt znika z Przeglądu od razu, a grupa bez odczytów znika cała. Gdy ukryte są wszystkie, strona mówi o tym i odsyła do preferencji. Odczyty ukryte w profilu (`[hidden]`) i przez `ignore` z lm-sensors nie trafiają na listę. Numerowane czujniki temperatury dysków NVMe (`temp2`–`temp9` sterownika `nvme`) są domyślnie ukryte (D42): na liście mają wyłączony przełącznik, włączone trafiają do klucza `shown-readings`, a wyłączenie zawsze wygrywa. Temperatura złożona dysku (`temp1`) jest widoczna jak każdy inny odczyt.
- **Napięcia:** wartość i ocena względem tolerancji z profilu (np. ±5 % dla +12 V, +5 V i +3,3 V według ATX). Ocena to ikona i tekst, np. „W tolerancji ±5 % (+0,8 %)”, a nie sam kolor. Napięcia bez tolerancji w profilu (rdzenie, SoC, VDDIO) nie mają oceny: podtytuł pokazuje źródło, jak przy temperaturach, a miejsce na ikonę zostaje puste, żeby wartości stały w jednej linii (D38).
- **Odświeżanie:** domyślnie co 1 s (w preferencjach 1–10 s), wstrzymane, gdy okno jest niewidoczne. Minimum i maksimum od uruchomienia stoją jako szary zakres obok bieżącej wartości. Wykresy historii przychodzą później (poza M1).
- **Układ:** przy szerokim oknie grupy stoją w dwóch kolumnach, przy wąskim w jednej, w kolejności: Procesor, Płyta, Wentylatory, Napięcia, Karta graficzna, Dyski, Pamięć, Inne.
- **Liczby i jednostki** w całej aplikacji według lokalizacji, w pl z polską typografią: przecinek dziesiętny i spacja przed jednostką i znakiem procentu: „+12 V”, „3,31 V”, „48,0 °C”, „888 rpm”, „45 %”. Zasady w jednym miejscu (`core::units`):
  - separator dziesiętny z `LC_NUMERIC` (`nl_langinfo(RADIXCHAR)`, D24), bez grupowania tysięcy (odczyty mają najwyżej 4 cyfry przed przecinkiem);
  - przed jednostką i znakiem procentu spacja niezłamliwa U+00A0 (D25), w każdym języku tak samo;
  - miejsca po przecinku według rodzaju odczytu: temperatura 1, napięcie 2, obroty 0, moc 1, wypełnienie 0 (procent według §3.5), odchyłka napięcia 1;
  - napięcie z nominałem poniżej 2 V, a bez nominału z odczytem poniżej 2 V (rdzenie, SoC, VDDIO, napięcie rdzenia karty), ma 3 miejsca po przecinku („1,344 V”, „0,215 V”); zakres bez nominału liczy się według większego co do wartości bezwzględnej końca, więc oba końce mają tyle samo miejsc (D27);
  - minus to znak U+2212 („−0,8 %”); odchyłka ma zawsze znak („+0,8 %”), a zero go nie ma; tolerancja ma „±” („±5 %”);
  - zakres min–maks: oba końce z półpauzą bez spacji i jednostka raz, na końcu („39,6–48,4 °C”, „888–890 rpm”). Dopóki oba końce po sformatowaniu są równe, zakresu nie ma (zamiast „12,10–12,10 V” i „0–0 rpm” sama wartość, a podtytuł dostaje więcej miejsca); pojawia się, gdy wartości się rozejdą. Czytnik ekranu dostaje wtedy samą wartość (D38, doprecyzowanie D6);
  - nazwy szyn ATX powstają z napięcia nominalnego tym samym formatowaniem, ze znakiem: „+12 V”, „+5 V”, „+3,3 V” (D26). Idą więc za `LC_NUMERIC`, jak wszystkie liczby, a nie za językiem tłumaczenia.
- **Tylko odczyt:** na stronie nie ma żadnego zapisu.
- **Stany zamiast błędów** (od kroku 12 M1):
  - wartość, której nie da się odczytać (błąd odczytu albo nieliczbowa treść pliku), stoi jako „–”, bez zakresu min–maks i bez oceny napięcia; reszta odczytów działa, a gdy wartość wróci, wiersz znów ją pokazuje;
  - z profilem, ale bez urządzenia hwmon płyty (sterownik niezaładowany albo bez układu): pozostałe układy są na stronie, a nad nimi baner „Brak odczytów płyty: nie znaleziono jej układu nct6687” z przyciskiem „Szczegóły” do strony System;
  - urządzenie, które zniknie (przeładowanie modułu), znika ze strony, a po powrocie jego odczyty wracają same, bez restartu (źródło odczytów powstaje na nowo, gdy zmieni się zestaw urządzeń hwmon);
  - bez żadnego urządzenia hwmon (np. maszyna wirtualna) `AdwStatusPage` „Brak odczytów czujników” z wyjaśnieniem; baneru wtedy nie ma;
  - bez libsensors odczyty idą prosto z sysfs bez żadnego komunikatu na tej stronie; mówi o tym tylko strona System (§5.4).

### 5.2 RGB (M3)

- **Urządzenie:** hidraw z rodzicem USB 0DB0:0076, wykrywane przez udev, także po podłączeniu i zniknięciu. Dodatkowo sprawdzamy, czy profil płyty przewiduje RGB i czy numer seryjny USB zaczyna się od kodu płyty (`7E61`).
- **Reguła udev** `/usr/lib/udev/rules.d/60-baseboard-rgb.rules` dodaje `TAG+="uaccess"` dla `SUBSYSTEM=="hidraw"` z `ATTRS{idVendor}=="0db0"` i `ATTRS{idProduct}=="0076"`. Numer pliku musi być mniejszy niż 73, żeby reguła zadziałała przed `73-seat-late.rules`. Reguła dopasowuje tylko PID 0076: audio płyty (0DB0:CC78) też ma hidraw i nie może dostać ACL. Bez reguły strona pokazuje instrukcję.
- **Protokół** (szczegółowy opis bajtów w `docs/rgb-protokol.md` w M3, ze wskazaniem źródeł publicznych):
  - **Start:** `GET_FEATURE 0x50` odczytuje bieżącą konfigurację wszystkich stref. Bez tego kontroler nie odpowiada na inne odczyty.
  - **Zmiana stref:** `SET_FEATURE 0x50` z pełnym blokiem stref, w którym zmieniamy tylko edytowane strefy, a resztę przepisujemy z odczytu. Ostatni bajt raportu to **store**.
  - **store = 0 (domyślnie):** zmiana tylko w RAM kontrolera.
  - **store = 1:** zapis do flash mikrokontrolera, wyłącznie po przycisku „Zapisz jako ustawienia startowe” z potwierdzeniem. Nigdy automatycznie i nigdy przy przeciąganiu suwaka (zużycie flash).
  - **Direct:** `SET_FEATURE 0x51` ustawia kolory pojedynczych diod złącza ARGB, najwyżej 240 diod w raporcie. Liczbę diod podaje użytkownik i musi odpowiadać rzeczywistości, bo przy złej wartości kontroler blokuje odpowiedź na kilka sekund.
- **Wersja firmware kontrolera:** jeden odczyt po starcie (po `GET_FEATURE 0x50`) i po każdym ponownym pojawieniu się urządzenia. Wynik trafia do sekcji „Kontroler RGB” na stronie System (§5.4). Tylko odczyt; aktualizacji firmware nie ma (§1.3). Gdy kontroler nie odpowiada, strona pokazuje „wersja nieznana” bez błędu. Format odpowiedzi na MS-7E61: O13.
- **Biała lista:** jedna funkcja w kodzie wysyła raporty i przyjmuje tylko typ wyliczeniowy z trzema wariantami: 0x50, 0x51 i odczyt wersji firmware. Dla odczytu wersji funkcja sprawdza identyfikator raportu i bajt polecenia; inne polecenia tego samego raportu są zabronione. Innych raportów nie da się wysłać bez zmiany tego typu.
- **Interfejs:** lista stref z profilu płyty, a dla każdej tryb (podzbiór trybów sprzętowych sprawdzony na sprzęcie), kolory, jasność i prędkość. Osobny widok Direct dla złączy ARGB z liczbą diod i kolorem lub gradientem.
- **Po uśpieniu** kontroler może zniknąć do zimnego startu. Strona mówi to wprost i nie zgłasza błędu jako awarii aplikacji.
- **Poza M3:** animacje liczone przez aplikację, raporty dla urządzeń łączonych szeregowo na złączach ARGB, synchronizacja z innymi urządzeniami.

### 5.3 Wydajność (M4)

- **Profile:** `org.freedesktop.UPower.PowerProfiles` na system bus. Lista profili z właściwości `Profiles`, zmiana przez `ActiveProfile`, reakcja na `PropertiesChanged`, więc zmianę z szybkich ustawień GNOME od razu widać w aplikacji. Pokazujemy `PerformanceDegraded` i `ActiveProfileHolds`: kto i dlaczego trzyma profil. Gdy usługi brak (ani tuned-ppd, ani power-profiles-daemon), strona to wyjaśnia.
- **Tylko do odczytu:** sterownik cpufreq, EPP, boost, zakres i bieżące częstotliwości. EPP nie zapisujemy, bo to zadanie usługi profili; dwa zapisujące programy by się kłóciły.
- **Wartości opisowo:** EPP i tryb sterownika pokazujemy po polsku (np. `balance_performance` → „Zrównoważona, z naciskiem na wydajność”), a surową wartość w podtytule.
- **Poza zakresem:** OC i limity mocy (§1.3).

### 5.4 System (M1: informacje tylko do odczytu; M4: restart do UEFI)

- **Informacje (M1, tylko odczyt, bez D-Bus):**
  - płyta: producent, model, rewizja z DMI; kod płyty i rewizja PCB wyliczone jak w §6.1,
  - BIOS: producent, wersja, data, `bios_release`. Z `class/dmi/id` czytamy poza polami z §6.1 tylko `bios_vendor`, `bios_version`, `bios_date` i `bios_release`,
  - system (`PRETTY_NAME` z `/etc/os-release`, zapasowo z `/usr/lib/os-release`), wersja jądra (`/proc/sys/kernel/osrelease`),
  - CPU: `model name` z `/proc/cpuinfo`, pokazywany krótko jak w Przeglądzie (§5.1); sterownik cpufreq (`scaling_driver` pierwszej polityki),
  - **pamięć:** dla każdego gniazda nazwa gniazda (np. DIMMA2) i stan; dla obsadzonych rozmiar, typ (np. DDR5), prędkość maksymalna i skonfigurowana w MT/s, numer części i producent. Źródło: baza udev, czyli właściwości `MEMORY_DEVICE_*` urządzenia `/devices/virtual/dmi/id`, które wbudowany moduł udev `dmi_memory_id` (systemd) wypełnia z SMBIOS typu 17. Odczyt przez libudev, bez roota; kod nie czyta plików `/run/udev/data`. Pytamy wyłącznie o pola `LOCATOR`, `PRESENT`, `SIZE`, `TYPE`, `SPEED_MTS`, `CONFIGURED_SPEED_MTS`, `PART_NUMBER`, `MANUFACTURER` i `MODULE_MANUFACTURER_ID`. Pola `MEMORY_DEVICE_*_SERIAL_NUMBER` nie czytamy (§1.3). libudev wczytuje do pamięci procesu cały wpis urządzenia, razem z tym polem, ale numer nie wychodzi poza bibliotekę. Strona pokazuje producenta tylko wtedy, gdy SMBIOS podaje jego nazwę. Gdy podaje „Unknown”, surowy identyfikator JEDEC z `MODULE_MANUFACTURER_ID` (np. „Bank 8, Hex 0x5D”) trafia wyłącznie do tekstu „Kopiuj informacje”, bez własnej tablicy nazw producentów (O14, D37 zmienia D8). Wartości zastępcze SMBIOS („Unknown”, „Not Specified” i podobne) traktujemy jak brak wartości. Gdy baza udev nie ma tych właściwości, sekcja pokazuje „brak danych”,
  - **karta graficzna:** nazwa jak w Przeglądzie (§5.1), identyfikatory PCI, sterownik i wersja VBIOS z `/sys/class/drm/card*/device/vbios_version` (amdgpu). Gdy pliku nie ma, wiersz jest pominięty,
  - **dyski:** model, rozmiar i wersja firmware: NVMe z `/sys/class/nvme/nvme*/{model,firmware_rev}`, pozostałe z `/sys/block/*/device/{model,rev}`; rozmiar z `/sys/block/*/size`. Bez urządzeń wirtualnych (zram, loop, device mapper) i bez czytników bez nośnika (rozmiar 0); nośniki wymienne, np. pendrive, są oznaczone,
  - **karty sieciowe:** urządzenia PCI klasy 0x02, nazwa z bazy sprzętu udev (D23) i sterownik. Interfejsów sieciowych i ich adresów nie czytamy,
  - sterownik `nct6683`: załadowany czy nie (jest na liście `/proc/modules` albo jest urządzenie hwmon z profilu), moduł z jądra czy spoza niego (D40: flaga `O` w nawiasie na końcu linii modułu w `/proc/modules`; linia bez flagi `O` to moduł z jądra; brak modułu na liście przy obecnym urządzeniu to sterownik wbudowany w jądro, czyli też z jądra; gdy listy nie da się odczytać, „pochodzenie modułu nieznane”, nigdy zgadywanie; lista jest czytana przy każdym odczycie strony, więc po przeładowaniu modułu pokazuje stan bieżący; tylko informacja, poziomu obsługi z tego nie wyliczamy, §5.5.2), poziom obsługi wentylatorów (`DriverLevel`, §5.5.2). Gdy jądro nie pozwala sterować wentylatorami, tekst mówi, że sterownik w tym jądrze tylko odczytuje wentylatory, a sterowanie pojawi się w jednej z przyszłych wersji Linuksa, bez numeru wersji jądra (D41, D60), jako informacja bez ikony ostrzeżenia,
  - kontroler RGB: obecny czy nie, czy jest reguła udev, wersja firmware (od M3, §5.2),
  - konfiguracja lm-sensors dla płyty: pliki, które czyta libsensors (`/etc/sensors3.conf`, zapasowo `/etc/sensors.conf`, oraz `/etc/sensors.d/*` bez plików ukrytych), z instrukcją `chip` dla układu płyty (wzorzec, którego część przed pierwszym „-” to `[hwmon] name` z profilu, np. `nct6687-isa-*`),
  - działające programy, które mogą konfliktować przy sterowaniu wentylatorami: `fancontrol`, `coolercontrold`, `fan2go` i `thinkfan`, rozpoznawane po nazwie procesu (`/proc/<pid>/comm`, bez linii poleceń).

  Numerów seryjnych nie czytamy ani nie pokazujemy: płyty i systemu (wymagają roota), modułów pamięci (w bazie udev są dostępne bez roota) i dysków. Tak samo adresów MAC (§1.3).
- **Strona (M1):** grupy w kolejności Płyta, BIOS, Procesor i system, Pamięć, Karta graficzna (osobna grupa na kartę, nazwa w opisie grupy), Dyski, Karty sieciowe, Czujniki i wentylatory (sterownik `nct6683`, sterowanie wentylatorami, konfiguracja lm-sensors, inne programy). Wiersze `AdwActionRow`: pola jako para nazwa–wartość (styl `property`), urządzenia z nazwą i szczegółami w podtytule, stany z ikoną i tekstem. Czytnik ekranu dostaje nazwę wiersza i jego wartość. Szczegóły:
  - grupa bez danych ma wiersz „Brak danych”, a pole bez wartości wartość „Brak danych”; puste gniazda pamięci mają „Puste”;
  - kod płyty zawsze z przedrostkiem, „MS-7E61”, także w stopce paska bocznego i w tekście do schowka (D38);
  - wiersz profilu płyty mówi w tytule, co profil pozwala, a w podtytule podaje plik profilu albo przyczynę jego braku (D38): „Sprawdzony dla tej rewizji PCB” (ikona OK) albo „Niesprawdzony dla rewizji PCB 2: tylko odczyt” i „Rewizja PCB nieznana: tylko odczyt” (ostrzeżenie), wszystkie z podtytułem „Profil płyty: ms-7e61.toml”; „Brak profilu dla tego modelu: tryb ogólny” z podtytułem „Profile dla MS-7E56 dotyczą modeli z innymi złączami wentylatorów” (D28); „Brak profilu dla tej płyty: tryb ogólny” z podtytułem „Odczyty bez nazw złączy, bez sterowania wentylatorami”. Stopka paska bocznego zostaje przy swoich krótszych tekstach;
  - sterownik cpufreq nazywa się w interfejsie „Sterownik częstotliwości procesora” (D38);
  - czcionka o stałej szerokości tylko dla ścieżek i poleceń (pliki lm-sensors, polecenia sprawdzenia sterownika); wersja jądra i VBIOS zwykłą czcionką (D38);
  - wersja BIOS-u z datą („1.A68 z 07.09.2026”); data jest zapisana jak w `LC_TIME` (`%x`), tak jak liczby idą za `LC_NUMERIC`, bo format z nazwą miesiąca z tłumaczenia mieszałby języki, gdy `LANGUAGE` i `LC_TIME` się różnią (D35);
  - rozmiary przez `g_format_size` według lokalizacji: pamięć w jednostkach binarnych („16,0 GiB”), dyski w dziesiętnych („2,0 TB”);
  - poziom obsługi wentylatorów słowami (§5.5.3), bez numeru wersji jądra; przy niesprawdzonej rewizji PCB z dopiskiem „tylko odczyt”. Bez profilu stan sterownika jest informacją, nie ostrzeżeniem, bo płyta może nie mieć takiego układu;
  - dane czyta wątek roboczy przy każdym wejściu na stronę, a gdy strona jest widoczna, także po zmianie zestawu urządzeń hwmon (sprawdzanej co 2 s). Przy niewidocznym oknie wątek stoi;
  - przycisk „Kopiuj informacje” w pasku nagłówka, w wąskim oknie (do 500 sp) sama ikona; po skopiowaniu powiadomienie `AdwToast`;
  - z profilem, gdy sterownik `nct6683` nie jest załadowany albo nie znalazł układu płyty: ostrzeżenie i wiersz „Sprawdzenie sterownika” z poleceniami tylko do odczytu `lsmod | grep nct6683` i `journalctl -k -g nct6683`. Bez profilu stan sterownika jest tylko informacją;
  - wiersz „Biblioteka lm-sensors”: wersja, gdy Przegląd czyta przez libsensors; inaczej, bez ostrzeżenia, „Nieużywana: odczyty prosto z sysfs, bez konfiguracji lm-sensors” z powodem (brak biblioteki, błąd `sensors_init`, podmieniony katalog sysfs w buildzie debug);
  - grupa „Pominięte pliki” (tylko gdy są): błędny profil płyty i błędny plik własnych nazw z ścieżką i powodem w jednym wierszu. Aplikacja działa dalej bez nich, przy starcie mówi o nich powiadomienie z przyciskiem „Szczegóły”, a na stderr pełny komunikat.
- **Restart do ustawień UEFI (M4):**
  1. `CanRebootToFirmwareSetup` z logind. Przycisk jest widoczny tylko przy „yes” lub „challenge”.
  2. Dialog z potwierdzeniem.
  3. `SetRebootToFirmwareSetup(true)`.
  4. Restart przez `org.gnome.SessionManager`, żeby inne aplikacje mogły zapisać pracę. Poza GNOME `Reboot(true)` z logind.
  5. Gdy użytkownik anuluje restart, flagę cofamy (`SetRebootToFirmwareSetup(false)`). Inaczej następny zwykły restart nieoczekiwanie wszedłby do UEFI.
- **Aktualizacje BIOS-u:** w M4 tylko wersja i link do strony pomocy producenta. Sprawdzanie w sieci, pobieranie, pendrive i lista kontrolna dochodzą w M5 (§5.6). Flashowania z systemu nie ma.
- **Przycisk „Kopiuj informacje o systemie”** (od M1) kopiuje do schowka tekst do zgłoszeń błędów, także upstream: zwykły tekst po angielsku, nigdy tłumaczony, sekcje z polami „nazwa: wartość”, brakujące wartości jako „unknown”, puste listy jako „none”, brak danych o pamięci jako „no data”. Format ustala `core::report`, a pełny wzorzec jest w teście `report_of_ms7e61`. W M1 tekst nie ma jeszcze profilu energii (dochodzi w M4, wymaga D-Bus) ani wersji firmware kontrolera RGB (dochodzi w M3). Zawartość:
  - wersja Baseboard,
  - system (`PRETTY_NAME` z `/etc/os-release`) i wersja jądra,
  - płyta: producent, model, rewizja, kod płyty i rewizja PCB, dopasowany profil,
  - BIOS: producent, wersja, data, `bios_release`,
  - CPU (krótka nazwa jak w Przeglądzie),
  - pamięć: gniazda, rozmiary, typ, prędkości, numery części i producent (nazwa z SMBIOS albo surowy identyfikator JEDEC, D37),
  - karty graficzne: nazwa, identyfikatory PCI, wersja VBIOS,
  - dyski: model, rozmiar, wersja firmware; bez nośników wymiennych,
  - karty sieciowe: nazwa z bazy sprzętu udev (§5.1) i sterownik,
  - sterownik cpufreq; aktywny profil energii (od M4),
  - nazwy urządzeń hwmon, stan `nct6683` („loaded (in-tree)” albo „loaded (out-of-tree)”) i poziom obsługi wentylatorów; kontroler RGB i wersja jego firmware (od M3).

  Tekst **nie zawiera** numerów seryjnych, adresów MAC, nośników wymiennych ani listy programów, także działających programów sterujących wentylatorami (§1.3).

### 5.5 Wentylatory (M2)

#### 5.5.1 ABI sterownika i poziomy pewności

| Poziom pewności | Zawartość | Atrybuty na MS-7E61 |
|---|---|---|
| **Przyjęte w `hwmon-next`** (jeszcze nie w drzewie Linusa; stan w §7 M2, M2.2) | seria Hamzy: tryb ręczny dla kanałów CPU, pompy i SYS przy customer ID 0x20D; `e0fdadf44480`: odczyt krzywych; `7dc9dff9608d`: odtworzenie trybu ręcznego po wznowieniu | `pwm1`, `pwm2`, `pwm13–16`: `pwmN` i `pwmN_enable` 0644 (1 = ręczny, 2 = automat z bieżącą krzywą); `pwmN_auto_point[1-7]_temp` (milistopnie), `_pwm` (0–255) i `pwmN_auto_channels_temp` (maska `tempX`) 0444 |
| **W recenzji** (v1: `81b1e2c64f6d`, `527bbd00913c`) | zapis punktów krzywej w trybie 1 i 2; przycinanie do 0–127 °C, 0–255 i do sąsiednich punktów; `-EIO` przy odrzuceniu przez EC (wraca krzywa sprzed zapisu); odtwarzanie zapisanych krzywych po wznowieniu | `pwmN_auto_point[1-7]_{temp,pwm}` 0644 dla kanałów, których krzywa przy ładowaniu ma niemalejące temperatury (≤ 127 °C) i wypełnienia; dla pozostałych 0444 |
| **Nieustalone** | łatka 5: wybór źródła temperatury krzywej (projekt: jeden bit, tylko temperatury raportowane przez sterownik; na MS-7E61 temp1–temp4); EZ Conn (`pwm4`, wymaga wentylatora na JAF_1) | `pwmN_auto_channels_temp` 0644 (projekt); brak `pwm4_enable` |

Aplikacja **nie wymaga** żadnego poziomu: korzysta z tego, co znajdzie (§5.5.2). Zachowania z wiersza „W recenzji” mogą się zmienić, ale zależy od nich tylko warstwa sterownika (§3.7).

#### 5.5.2 Wykrywanie możliwości

Wyłącznie po atrybutach sysfs, **osobno dla każdego kanału**. Nigdy po wersji jądra ani po skażeniu modułu. Skażenie (moduł spoza jądra) strona System pokazuje tylko jako informację (§5.4).

| Możliwość | Warunek |
|---|---|
| `duty-read` | jest `pwmN` |
| `mode` | jest `pwmN_enable` z bitem zapisu właściciela |
| `duty-write` | `mode` i `pwmN` z bitem zapisu właściciela |
| `curve-read` | są `pwmN_auto_pointK_temp` i `_pwm` dla K = 1…n; n to liczba tych plików |
| `curve-write` | `curve-read` i wszystkie te pliki z bitem zapisu właściciela |
| `source-read` | jest `pwmN_auto_channels_temp` |
| `source-write` | `source-read` z bitem zapisu właściciela (użycie: D17) |

- Aplikacja działa bez roota, więc sprawdza bity trybu pliku (`stat`), a nie `access(W_OK)`. Pomocnik liczy tak samo.
- Kanał jest **sterowalny**, gdy profil go tak oznacza i ma `mode`. Kanał spoza profilu albo bez `mode` jest tylko do odczytu, nawet przy zapisywalnych plikach. Sterowalność rozstrzyga pomocnik i podaje ją w `GetChannels`; aplikacja pokazuje sterowanie tylko przy kanałach, które pomocnik zgłosił jako sterowalne (D33).
- `DriverLevel` to podsumowanie dla interfejsu: najniższy poziom wspólny dla kanałów sterowalnych z profilu.

| `DriverLevel` | Warunek | Typowy sterownik |
|---|---|---|
| `read-only` | żaden kanał nie ma `mode` | jądro Fedory 7.3.3 |
| `manual` | `mode` i `duty-write`, bez `curve-read` | jądro z częścią łatek z `hwmon-next` (np. przeniesioną przez dystrybucję); w `hwmon-next` tryb ręczny wchodzi razem z odczytem krzywych |
| `manual+curve-view` | dodatkowo `curve-read` | `hwmon-next` |
| `curves` | dodatkowo `curve-write` | `hwmon-next` z serią v1 (moduł testowy) |
| `curves+source` | dodatkowo `source-write` | po łatce 5 |

- Po atrybutach nie widać, czy sterownik odtwarza stan po wznowieniu. B12 działa więc tak samo dla każdego sterownika.

#### 5.5.3 Co widzi użytkownik

Według poziomu sterownika, wydania (D47) i tego, czy jest pomocnik. Flatpak wygląda tak samo jak instalacja natywna, bo odczyt sysfs działa w piaskownicy (§10.1); różnią się tylko komunikaty o uprawnieniu D-Bus i okno polkit od agenta gospodarza (O19).

| | `read-only` (wydane jądra, dziś Fedora 7.3.3) | `manual` | `manual+curve-view` (`hwmon-next`) | `curves` (moduł z v1) |
|---|---|---|---|---|
| Przegląd | obroty; wypełnienie procesora, pompy i wentylatora dodatkowego; SYS 1–4 bez wypełnienia | obroty i wypełnienie wszystkich złączy; tryb ręczny w podtytule, automat bez dopisku (D59) | jak `manual` | jak `manual` |
| Wentylatory, 0.2.0 (sam podgląd, bez pomocnika) | wiersz „Sterowanie wentylatorami niedostępne” z ikoną informacji i przyciskiem „Stan w jądrze” (strona projektu): sterownik w tym jądrze tylko odczytuje wentylatory (tu też: bez wypełnienia wentylatorów systemowych), a sterowanie pojawi się w jednej z przyszłych wersji Linuksa, bez numeru wersji jądra (D60); niskie karty z obrotami i paskiem wypełnienia tam, gdzie jest | wiersz „Ta wersja Baseboard pokazuje ustawienia wentylatorów bez możliwości zmiany.”; karty z trybem jako tekstem z kłódką (D60), bez wykresu | jak `manual`, z miniaturą krzywej i podstroną krzywej w podglądzie | jak `manual+curve-view` |
| Wentylatory od 0.3.0, bez pomocnika | jak w 0.2.0 (przeszkodą jest sterownik) | wiersz „Podgląd bez sterowania” z „Jak zainstalować”; karty jak w 0.2.0 | jak obok | jak obok |
| Wentylatory od 0.3.0, z pomocnikiem | jak w 0.2.0; pomocnik startuje tylko na odczyt właściwości | przełącznik „Automat BIOS / Ręczny” | to samo; krzywe w podglądzie z dopiskiem, że edycja wymaga nowszego sterownika | od 0.4.0 przełącznik „Automat BIOS / Ręczny / Krzywa” i edytor; w 0.3.0 jak `manual+curve-view` |
| Baner „Zmiany działają także po zamknięciu aplikacji, do restartu komputera” | nigdy | gdy któryś kanał jest w trybie ręcznym ustawionym przez Baseboard | jak `manual` | jak `manual` albo gdy któryś kanał ma krzywą inną niż pierwotna |
| Strona System, poziom obsługi | „Sterowanie wentylatorami niedostępne” z wyjaśnieniem jak wyżej (bez zdania o wentylatorach systemowych), bez ikony ostrzeżenia (D60) | „Tryb ręczny” | „Tryb ręczny i podgląd krzywych” | „Krzywe w EC” |

Złącze bez wpisu w `tested_writes` (na MS-7E61 pompa, D52) ma zamiast przełącznika ramkę „Tylko podgląd: złącza nie sprawdzono na tej płycie.”. Nieistniejące `pwm3` jest w każdym przypadku tylko do odczytu. Wentylator dodatkowy (EZ Conn, `pwm4`) jest tylko do odczytu, dopóki sterownik nie obejmie tego kanału (`pwm4_enable`) i zapis nie zostanie sprawdzony na sprzęcie (D32). Oba z wyjaśnieniem.

#### 5.5.4 Strona Wentylatory

Wygląd według zaakceptowanych makiet (D45): szczegóły karty, wykresu, edytora, stanów, ruchu i dostępności oraz realizacja w GTK4 w §7 M2, M2.4. Tu zasady zachowania.

- **Nad kartami** jeden wiersz „Źródło temperatury” z wartością, zakresem min–maks i progiem awaryjnym. Karta podaje źródło tylko wtedy, gdy różni się od wspólnego.
- **Karta złącza:** nazwa z profilu (§6.2) i złącze, obroty, wypełnienie (bieżące, a w automacie także cel z krzywej, dopóki wyjście do niego dochodzi), tryb wybierany przełącznikiem segmentowym (AdwToggleGroup) i miniatura krzywej z punktem pracy. Przy szerokim oknie karty stoją w dwóch kolumnach. Karta nie jest klikalna; podstronę krzywej otwiera przycisk „Edytuj krzywą” (bez zapisu krzywych „Szczegóły krzywej”, D60). Etykiety „Krzywa z BIOS-u” nie ma: tryb pokazuje przełącznik, a w wersji bez sterowania tekst trybu z kłódką (D60).
- **Tryby w interfejsie a sterownik** (D11):

  | Przełącznik | `pwmN_enable` | Krzywa w EC |
  |---|---|---|
  | Automat BIOS | 2 | pierwotna (B18); gdy była zmieniona, wybranie zapisuje ją z powrotem, bez walidacji |
  | Ręczny | 1 | bez zmian |
  | Krzywa | 2 | zmieniona przez użytkownika |

  „Krzywa” jest widoczna tylko przy `curve-write`. Przy samym `curve-read` karta w automacie pokazuje krzywą z EC do podglądu.
- **Ręczny:** zamiast wykresu duża wartość wypełnienia i suwak z dolną granicą równą minimum (B5). Zakres poniżej minimum jest zakreskowany. Zmiana działa od razu.
- **Krzywa:** „Edytuj krzywą” otwiera podstronę (AdwNavigationPage) z edytorem:
  - dokładnie tyle punktów, ile podaje sterownik (7). Ostatnie punkty mogą być kopią poprzedniego, tak jak BIOS wypełnia punkty 5–7 przy czterech poziomach;
  - temperatury i wypełnienia niemalejące. Edytor nie pozwala zapisać krzywej łamiącej B5, B6, B7 i B14 (dla procesora i pompy 100 % najpóźniej na progu awaryjnym źródła; dla wentylatorów systemowych bez tego wymogu, D19);
  - **krzywa z EC, która łamie zasady** (np. pierwotna z BIOS-u, §6.3): edytor otwiera się i pokazuje ją bez zmian. Punkt, który trzeba zmienić, ma na wykresie kolor i znak ostrzeżenia, a w swoim wierszu listy tekst, np. „Ustaw co najmniej 30 %” albo „Przy 95 °C wymagane 100 %”. „Zastosuj” jest nieaktywne, dopóki krzywa nie spełni zasad. Edytor sam niczego nie poprawia;
  - wartości punktów zmienia się przeciąganiem, klawiaturą na wykresie albo w polach listy (przyciski i wpisywanie);
  - źródło temperatury tylko do podglądu (nazwa jak w Przeglądzie); wybór dopiero przy `source-write` (M2c, D17);
  - wykres z bieżącym punktem pracy, wartościami poza skrajnymi punktami i progiem awaryjnym źródła;
  - „Zastosuj” wysyła krzywą do pomocnika (polkit, D3), „Cofnij zmiany” wraca do krzywej z EC, „Przywróć krzywą BIOS” do pierwotnej (B18);
  - po zastosowaniu dopisek, że wentylator dochodzi do nowego wypełnienia stopniowo; bez liczby, dopóki O15 nie da pomiaru dla wszystkich kanałów.
- **Kanał w trybie ręcznym:** zapis krzywej nie zmienia wypełnienia. „Zastosuj” ustawia więc także automat (`activate`), o czym edytor mówi przed zapisem.
- **Baner (AdwBanner)** „Zmiany działają także po zamknięciu aplikacji, do restartu komputera” (D4) z przyciskiem „Przywróć automat BIOS” (`SetAllAuto`: krzywe pierwotne i automat na wszystkich kanałach zmienionych przez Baseboard). To jedyny komunikat o czasie działania zmian. Kiedy jest widoczny: §5.5.3; widać go także na podstronie krzywej. Pasek nagłówka nie ma osobnego przycisku.
- **Pompa ze sterowaniem:** stałe ostrzeżenie w karcie, że pompa nie może się zatrzymać, także w krzywej (B6). Pompa bez sprawdzonego zapisu (D52) ma ramkę „Tylko podgląd” zamiast przełącznika; dopisek o niesprawdzonym złączu (`verified = false`) stoi wtedy w tej ramce, a nie w podtytule.
- **Błędy sterownika:** `DriverRejected` → „Kontroler płyty nie przyjął ustawień” (w edytorze: obowiązuje poprzednia krzywa, zmiany zostają w edytorze), z odczytaną krzywą; `Timeout` → „Kontroler płyty nie odpowiedział.” Bez kodów błędów w interfejsie; szczegóły w dzienniku (B15).
- **Bez powtórzeń:** na jednym ekranie żaden tekst nie powtarza informacji z innego miejsca tego ekranu (D45).

### 5.6 Aktualizacja BIOS-u (M5, na stronie System)

Aplikacja **nie flashuje** BIOS-u (§1.3, D7). Pomaga przygotować aktualizację, którą użytkownik wykonuje sam wbudowanym narzędziem w UEFI (na MS-7E61: M-Flash).

- **Przełącznik w ustawieniach** „Sprawdzaj aktualizacje BIOS-u w sieci”, domyślnie wyłączony.
  - **Wyłączony:** strona System wygląda jak w M4 (wersja i link do strony pomocy). Kod sieciowy jest nieosiągalny.
  - **Włączony:** pojawia się przycisk „Sprawdź nowszą wersję”. Aplikacja łączy się z siecią wyłącznie po kliknięciu, nigdy w tle, przy starcie ani okresowo.
- **Źródło informacji:**
  - **Lista:** lista wydań BIOS-u, z której korzysta oprogramowanie producenta. To plik XML na serwerze aktualizacji producenta (`liveupdate.msi.com`).
  - **To nie jest publiczne API.** Producent nie dokumentuje listy ani nie publikuje warunków korzystania z serwera. Ścieżka i format mogą się zmienić bez ostrzeżenia, więc funkcja działa na zasadzie „najlepszej próby”: każdy błąd kończy się spokojnym komunikatem i linkiem do strony pomocy, nigdy awarią.
  - **Porównanie:** oprogramowanie producenta porównuje wersję z listy z wersją z SMBIOS. My bierzemy bieżącą wersję z DMI (`bios_version`), a płytę z kodu płyty (§6.1).
  - **Warunki dopasowania:** lista podaje przy wydaniu także warunki, od których zależy, czy wolno je zaproponować na danym komputerze:
    - zakres wersji, z których można przejść na to wydanie (np. gdy potrzebna jest wersja pośrednia),
    - wersje pominięte,
    - wykluczenia, m.in. dla procesorów, zainstalowanego sprzętu, sterowników i ustawień systemu Windows.

    Samo porównanie numerów mogłoby więc polecić wydanie, którego producent na tym komputerze nie proponuje. Zakres i wersje pominięte sprawdzamy względem `bios_version`, wykluczenia procesora względem `/proc/cpuinfo`. Warunek, którego nie umiemy sprawdzić w Linuksie (np. dotyczący Windows) albo którego rodzaju nie znamy, daje wynik „Nie rozstrzygam”.
  - **Do ustalenia (O11):** dokładna ścieżka, pola, które czytamy, zapis wersji na liście, zapis i znaczenie warunków dopasowania oraz to, czy lista podaje sumę kontrolną.
  - **W repozytorium** tylko adres i opis odczytywanych pól, własnymi słowami (§2). Przykłady w testach są sztuczne, nie są kopią pliku producenta.
- **Tolerancja błędów i zmian formatu:**
  - **Połączenie:**
    - HTTPS z weryfikacją certyfikatu przez systemowy magazyn i proxy z ustawień GNOME;
    - limit czasu (ok. 15 s) i limit rozmiaru odpowiedzi;
    - bez ciasteczek i identyfikatorów, z ogólnym User-Agent aplikacji.
  - **Parser** pomija nieznane elementy.
  - **Błąd sieci, kod HTTP inny niż 200, uszkodzony XML, brak wymaganych pól albo płyty na liście:** spokojny komunikat „Nie udało się sprawdzić” z linkiem do strony pomocy. Bez awarii i bez ponawiania w tle.
- **Wynik** jest trójstanowy, z czasem ostatniego sprawdzenia:
  1. **„Masz najnowszą wersję”:** na liście nie ma nowszego wydania dla tej płyty.
  2. **„Dostępna wersja X z dnia Y”:** tylko gdy obie wersje dają się jednoznacznie odczytać i porównać, a wszystkie warunki dopasowania tego wydania dało się sprawdzić i są spełnione.
  3. **„Nie rozstrzygam”:** jest nowsze wydanie, ale któregoś warunku nie da się sprawdzić albo wersji nie da się jednoznacznie porównać. Pokazujemy obie wersje, powód i link do strony pomocy.

  Wydanie z niespełnionym warunkiem pomijamy. Błąd sieci lub formatu to osobny komunikat „Nie udało się sprawdzić” (wyżej), a nie czwarty wynik. Przycisk „Pobierz” pojawia się tylko przy wyniku 2.
- **Pobranie i weryfikacja:**
  - przycisk „Pobierz” pokazuje rozmiar przed pobraniem; pasek postępu i anulowanie;
  - plik trafia do `$XDG_CACHE_HOME/baseboard/bios/`;
  - suma SHA-256 (GLib `GChecksum`) jest liczona w trakcie pobierania i porównywana z sumą wzorcową, której źródło ustali O11;
  - **niezgodna suma:** plik usunięty i komunikat;
  - **brak sumy wzorcowej:** aplikacja pokazuje obliczoną sumę i wyraźnie zaznacza brak weryfikacji, a przygotowanie pendrive'a wymaga wtedy dodatkowego potwierdzenia. Nie wiadomo, czy lista w ogóle podaje sumę (O11), więc ten przypadek może być zwykłą ścieżką, a nie wyjątkiem;
  - **archiwum:** wyodrębniamy tylko plik BIOS-u (O12), z limitem rozmiaru i z odrzuceniem ścieżek bezwzględnych i z `..`.
- **Pendrive:**
  - **Lista nośników:** z UDisks2 przez D-Bus, bez roota. Tylko dyski wymienne podłączone przez USB. **Nigdy** dysk, na którym jest zamontowane `/`, `/boot`, `/boot/efi` albo swap.
  - **Wybór:** przy każdym nośniku model, rozmiar, etykieta i system plików. Użytkownik wybiera nośnik i potwierdza w dialogu z nazwą modelu i rozmiarem.
  - **FAT32:**
    1. montowanie przez UDisks2;
    2. kopia pliku do katalogu głównego (istniejący plik o tej samej nazwie tylko po pytaniu);
    3. `fsync` i ponowne obliczenie sumy z nośnika;
    4. odmontowanie i bezpieczne wysunięcie.
  - **Inny system plików albo za mało miejsca:** informacja, nic nie zapisujemy.
  - **Formatowanie** (cały nośnik, FAT32) wyłącznie po osobnej, wyraźnej zgodzie: dialog typu „destrukcyjny” z nazwą nośnika i ostrzeżeniem o utracie danych. Bez tej zgody aplikacja nigdy nie formatuje.
  - **Wymagania narzędzia UEFI** wobec nośnika i pliku: O12.
- **Restart do ustawień UEFI:** jak w §5.4 (M4). Przed restartem krótka instrukcja:
  - podłącz pendrive i nie wyłączaj zasilania w trakcie aktualizacji;
  - w UEFI otwórz narzędzie aktualizacji i wybierz plik;
  - jeśli dysk odblokowuje TPM (np. `systemd-cryptenroll --tpm2-device`), po aktualizacji pomiary się zmienią i będzie potrzebne hasło odzyskiwania. Sprawdź, czy je masz.
- **Lista kontrolna „Po aktualizacji BIOS-u”:**
  - Aplikacja zapamiętuje ostatnio widzianą wersję BIOS-u (DMI). Gdy się zmieni, strona System pokazuje baner z listą do odhaczenia, który można zamknąć. Lista jest dostępna także ręcznie.
  - Punkty:
    1. **Wpisy rozruchowe:** aktualizacja może wyczyścić NVRAM i usunąć wpisy (np. shim, rEFInd). Aplikacja pokazuje obecne wpisy (odczyt `BootOrder` i `Boot####` z efivarfs, bez zapisu). Naprawa ręczna (`efibootmgr`) według instrukcji w dokumentacji.
    2. **Secure Boot i klucze MOK:** sprawdź stan Secure Boot i listę MOK (`mokutil --sb-state`, `mokutil --list-enrolled`). Gdy MOK zniknie, moduły podpisane własnym kluczem przestaną się ładować, więc trzeba go zarejestrować ponownie.
    3. **Wentylatory:** krzywe w ustawieniach BIOS-u wracają do domyślnych, więc ustaw je ponownie. Krzywe ustawione przez Baseboard i tak znikają przy restarcie (D4, B18).
    4. **Pamięć (EXPO) i inne ustawienia** (np. PBO, Resizable BAR) wracają do domyślnych, więc włącz je ponownie w UEFI.

---

## 6. Wykrywanie płyty i profile płyt

### 6.1 Wykrywanie

1. **DMI:** `board_vendor` = „Micro-Star International Co., Ltd.”, kod płyty to 4 znaki szesnastkowe w nawiasie „(MS-xxxx)” w `board_name`. Zapasowo bierzemy cały `product_name` „MS-xxxx”. Dla tej płyty kod to `7E61`. Nazwa modelu to `board_name` bez części „(MS-xxxx)”, u nas „MAG B850M MORTAR WIFI”. Czytamy wyłącznie `board_vendor`, `board_name`, `board_version` i `product_name` z `<sysfs>/class/dmi/id`; plików z numerami seryjnymi i UUID nie otwieramy. Pusty albo brakujący plik oznacza brak wartości. Producent musi się zgadzać dokładnie (po obcięciu białych znaków).
2. **Model:** kod płyty nie wyznacza układu złączy, bo jeden kod mają płyty o różnych złączach (D28). Profil może więc wymienić nazwy modeli (`board_names` w `[match]`); wielkość liter i liczba spacji nie mają znaczenia. Spośród profili dla danego kodu:
   - wygrywa profil, który wymienia nazwę modelu płyty;
   - bez takiego profilu pasuje profil bez listy nazw (każdy model o tym kodzie, jak przed D28);
   - profil z listą nazw nie pasuje do płyty o innej nazwie ani do płyty, której DMI nie podaje nazwy.

   Gdy kod pasuje, ale żaden profil nie pasuje do modelu, aplikacja działa w trybie ogólnym (pkt 5), a stopka paska bocznego mówi, że ten model ma inny układ złączy. Sterowania wtedy nie ma nigdy.
3. **Rewizja PCB:** pierwsza cyfra `board_version`, u nas „1.0” → 1. Rewizje mogą różnić się sprzętem, więc profil wymienia te, które zweryfikowano. Inna albo nieznana rewizja oznacza tryb tylko do odczytu z ostrzeżeniem (B9); ostrzeżenie stoi w stopce paska bocznego. Gdy płycie odpowiada po modelu kilka profili, wygrywa ten, który wymienia jej rewizję. Model rozstrzyga przed rewizją: profil z nazwą modelu, ale bez tej rewizji, daje tryb tylko do odczytu, nawet gdy profil bez listy nazw wymienia rewizję.
4. **Potwierdzenie sprzętu:** urządzenie hwmon o nazwie `nct6687`; do RGB urządzenie USB 0DB0:0076 z numerem seryjnym zaczynającym się od kodu płyty.
5. **Brak pasującego profilu** oznacza tryb ogólny: Przegląd, Wydajność i System działają; RGB i sterowanie wentylatorami są niedostępne.

### 6.2 Profil płyty jako dane

- **Format:** TOML z polem `schema` (wersja formatu).
- **Położenie:**
  - `/usr/share/baseboard/boards/`: profile dostarczane z aplikacją,
  - `/etc/baseboard/boards/`: nadpisania administratora; czytają je aplikacja i pomocnik,
  - `~/.local/share/baseboard/boards/`: tylko własne nazwy odczytów, czyta wyłącznie aplikacja (od kroku 11 M1, D22; format niżej); nie wybiera profilu.

  Profil z późniejszego katalogu zastępuje profil o tym samym `id` z wcześniejszego. Brakujący katalog pomijamy, a błędny plik zgłaszamy i pomijamy: aplikacja działa dalej, strona System wymienia go z powodem (§5.4), a przy starcie mówi o tym powiadomienie. Build debug uruchamiany przez `meson devenv` czyta zamiast tych katalogów `data/boards` ze źródeł (`BASEBOARD_BOARDS_DIR`; `meson devenv` ustawia tę zmienną sam, więc inny katalog trzeba podać w poleceniu uruchamianym przez devenv), a katalog główny sysfs może podmienić `BASEBOARD_SYSFS_ROOT`. Build release i pomocnik tego nie potrafią.
- **Nazwy tłumaczone bez tłumaczenia danych:** profil podaje **rolę** z zamkniętego słownika (`cpu`, `pump`, `sys`, `aux`; temperatury `cpu`, `system`, `vrm`, `chipset`; napięcia `p12v`, `p5v`, `p3v3`, `vcore`, `soc`, `vddio`) oraz numer. Aplikacja buduje z nich nazwę przez gettext, np. „Wentylator systemowy 1”. Nazwa złącza z instrukcji płyty (np. `SYS_FAN1`) jest dodatkiem i nie podlega tłumaczeniu. Opcjonalne pole `label` nadpisuje nazwę bez tłumaczenia.
- **Nazwy ról** (msgid po angielsku, *N* to numer z profilu):

  | Rola | msgid | pl |
  |---|---|---|
  | wentylator `cpu` | CPU fan *N* | Wentylator procesora *N* |
  | wentylator `pump` | Pump *N* | Pompa *N* |
  | wentylator `sys` | System fan *N* | Wentylator systemowy *N* |
  | wentylator `aux` | Auxiliary fan *N* | Wentylator dodatkowy *N* |
  | temperatura `cpu` | CPU (board sensor) | Procesor (czujnik płyty) |
  | temperatura `system` | System temperature | Temperatura systemowa |
  | temperatura `vrm` | CPU power stage | Sekcja zasilania procesora |
  | temperatura `chipset` | Chipset | Chipset |
  | napięcie `p12v`, `p5v`, `p3v3` | bez msgid: napięcie nominalne formatowane jak liczby (§5.1, D26) | +12 V, +5 V, +3,3 V |
  | napięcie `vcore`, `soc`, `vddio` | CPU cores, CPU SoC, CPU VDDIO | Rdzenie procesora, SoC procesora, VDDIO procesora |

**Format `schema = 1`** (od M1 krok 2; pełny przykład: `data/boards/ms-7e61.toml`, parser i walidator: `core::profile`):

| Sekcja | Pola | Uwagi |
|---|---|---|
| główna | `schema`, `id`, `model` | `id` równe nazwie pliku bez `.toml`, małe litery, cyfry i „-”; `model` do wyświetlenia, z DMI |
| `[match]` | `board_vendor`, `board_code`, `pcb_revisions`, `board_names` | kod: 4 wielkie cyfry szesnastkowe; rewizje: cyfry 0–9, co najmniej jedna; `board_names` opcjonalne: nazwy modeli z DMI bez „(MS-xxxx)”, brak albo pusta lista = każdy model o tym kodzie (§6.1, D28) |
| `[hwmon]` | `name` | atrybut `name` urządzenia hwmon |
| `[[fan]]` | `key`, `role`, `number`, `header`, `fan`, `pwm`, `controllable`, `min_duty`, `verified`, `tested_writes`, `label` | `fan` i `pwm` to N z `fanN_input` i `pwmN`; `controllable` domyślnie wyłączone; `min_duty` w procentach, domyślnie stała granica roli (B5); `tested_writes`: `"manual"`, `"curve"` (D16) |
| `[[temp]]` | `key`, `role`, `input`, `emergency_c`, `label` | `input` to N z `tempN_input`; `emergency_c`: próg B4, a dla krzywych z tym źródłem także B14 |
| `[[voltage]]` | `key`, `role`, `input`, `multiplier`, `nominal`, `tolerance_pct`, `label` | `multiplier` domyślnie 1 |
| `[hidden]` | `fan`, `pwm`, `temp`, `in` | numery kanałów, których nie pokazujemy |
| `[rgb]` | `usb_id`, `serial_prefix` | strefy i ich identyfikatory: do ustalenia w M3 (O1) |

**Walidacja** (błąd zamiast cichego pominięcia):
- nieznane pola i nieznane role są błędem; inna wersja `schema` daje osobny błąd przed sprawdzeniem pól;
- klucze: 1–16 małych liter i cyfr, unikalne w sekcji; unikalne też numery kanałów, para rola–numer wentylatora, rola temperatury i napięcia;
- `min_duty` w 0–100 % i nie niżej niż stała granica roli (CPU 20 %, pompa 40 %, B5);
- `controllable` wymaga `pwm`, a `tested_writes` wymaga `controllable` i nie powtarza wpisów;
- `emergency_c` w 1–125 °C; `multiplier` i `nominal` dodatnie; `tolerance_pct` w (0, 50] i tylko z `nominal`;
- kanał z `[hidden]` nie może być użyty przez profil;
- `usb_id` w postaci „vvvv:pppp” małymi literami;
- nazwy w `board_names` niepuste, bez części „(MS-”, bez powtórzeń (porównanie bez wielkości liter i nadmiarowych spacji).

`board_names` doszło po kroku 8 M1 jako pole opcjonalne, więc `schema` zostaje 1: profile bez niego wczytują się i pasują jak dotąd (D28).

**Własne nazwy odczytów** (D22, od kroku 11 M1; parser i walidator: `core::labels`):
- plik `$XDG_DATA_HOME/baseboard/boards/<id>.toml`, zwykle `~/.local/share/baseboard/boards/ms-7e61.toml`, dla profilu o tym `id`; inne profile go nie widzą. Pomocnik go nie czyta;
- format TOML z `schema = 1` i trzema opcjonalnymi tabelami `[fan]`, `[temp]`, `[voltage]`: klucz to `key` wpisu z profilu, wartość to nazwa, np. `[fan]` `sys1 = "Przód, dół"`;
- walidacja: inna wersja `schema` i nieznane sekcje są błędem; klucz musi być kluczem tego rodzaju z profilu; nazwa po obcięciu białych znaków ma 1–64 znaki bez znaków sterujących. Błędny plik aplikacja zgłasza na stderr i pomija w całości;
- nazwa nie jest tłumaczona i ma pierwszeństwo przed etykietami z lm-sensors i nazwami z profilu (§5.1); podtytuł (złącze, źródło) zostaje;
- plik jest czytany przy starcie aplikacji; okno preferencji pokazuje jego ścieżkę i mówi, że zmiany działają po ponownym uruchomieniu. Edycji nazw w interfejsie w M1 nie ma.

### 6.3 Profil MS-7E61 (stan wiedzy 2026-11-10)

**Wentylatory.** Sterowanie według poziomów z §5.5.1. Stan testów na module z `hwmon-next` i v1 (notatki z projektu sterownika).

| Klucz | Złącze | fanN | pwmN | Rola | Tryb ręczny | Zapis krzywej | Zapis sprawdzony na sprzęcie | Minimum | Pewność przypisania |
|---|---|---|---|---|---|---|---|---|---|
| `cpu1` | CPU_FAN1 | fan1 | pwm1 | cpu | tak | tak (v1) | nie (O5) | 30 % | pewne (pomiar) |
| `pump1` | PUMP_SYS1 | fan2 | pwm2 | pump | tak | tak (v1) | nie (O5) | 50 % | wg analizy, niesprawdzone fizycznie |
| – | brak złącza | fan3 | pwm3 | ukryty | nie | nie | – | – | wniosek |
| `ezconn` | JAF_1 (EZ Conn) | fan4 | pwm4 | aux | nie (sterownik nie obejmuje kanału) | nie | – | – | wg analizy, niesprawdzone fizycznie |
| `sys1` | SYS_FAN1 | fan16 | pwm16 | sys | tak | tak (v1) | **tryb ręczny i krzywa, także po S3** (2026-11-06, 2026-11-09) | 0 % | pewne (test fizyczny) |
| `sys2` | SYS_FAN2 | fan15 | pwm15 | sys | tak | tak (v1) | nie | 0 % | pewne |
| `sys3` | SYS_FAN3 | fan14 | pwm14 | sys | tak | tak (v1) | nie | 0 % | pewne |
| `sys4` | SYS_FAN4 | fan13 | pwm13 | sys | tak | tak (v1) | nie | 0 % | pewne |

Nazwy złączy SYS_FAN1–4 trzeba potwierdzić z instrukcją płyty. Wentylatory SYS nie mają twardego minimum (0 % dozwolone, np. dla trybu bez obrotów w spoczynku). Chronią je progi awaryjne MOS i System (B4). Wszystkie krzywe BIOS-u na tej płycie mają źródło temp1 (procesor), a punkty 5–7 są kopią punktu 4 (testy sterownika).

**Krzywe BIOS-u a zasady B5, B7 i B14** (zrzuty EC z 2026-11-09; minimum według B5 w jednostkach 0–255: CPU 76, pompa 127, SYS 0; próg źródła temp1: 95 °C):

| Kanał | Krzywa (punkty 1–4; 5–7 = 4) | B5 | B7 | B14 |
|---|---|---|---|---|
| CPU Fan 1, domyślna BIOS-u (A, B) | 0/45/65/80 °C → 51/89/178/255 (20/35/70/100 %) | **łamie** (punkt 1: 51 < 76) | spełnia | spełnia (255 przy 80 °C) |
| CPU Fan 1, ustawienia właściciela (C, D) | 0/75/80/85 °C → 122/124/191/255 (48/49/75/100 %) | spełnia | spełnia | spełnia (255 przy 85 °C) |
| Pompa, domyślna i właściciela (A–D) | 35/40/70/90 °C → płasko 255 (100 %) | spełnia | spełnia | spełnia |
| SYS Fan 1–4, domyślna BIOS-u (A, B) | 40/55/70/85 °C → płasko 153 (60 %) | spełnia | spełnia | bez wymogu (D19); wcześniej by łamała |
| SYS Fan 1, właściciela (D) | 0/5/84/85 °C → 94/96/99/255 (37/38/39/100 %) | spełnia | spełnia | spełnia także dawny wymóg |
| SYS Fan 2, właściciela (C, D) | 0/5/84/85 °C → 104/107/109/255 (41/42/43/100 %) | spełnia | spełnia | spełnia także dawny wymóg |
| SYS Fan 3, właściciela (C, D) | 40/55/70/85 °C → płasko 76 (30 %) | spełnia | spełnia | bez wymogu (D19); wcześniej by łamała |
| SYS Fan 4, właściciela (C, D) | 40/55/70/85 °C → płasko 102 (40 %) | spełnia | spełnia | bez wymogu (D19); wcześniej by łamała |

Domyślna krzywa CPU z BIOS-u łamie minimum B5. Krzywą pierwotną aplikacja przywraca więc bez walidacji (B18), a edytor otwarty na takiej krzywej wskazuje punkt do zmiany (§5.5.4).

**Temperatury**

| hwmon | Nazwa | Rola | Pewność |
|---|---|---|---|
| temp1 | CPU | cpu | pewne (śledzi Tctl z `k10temp`; nadążanie pod obciążeniem: O10) |
| temp2 | System | system | wniosek |
| temp3 | MOS | vrm | pewne (test obciążeniowy) |
| temp4 | Chipset A | chipset | wniosek |
| temp5 | – (ten sam czujnik co temp3) | ukryty | pewne |

**Napięcia**

| hwmon | Szyna | Mnożnik | Tolerancja | Pewność |
|---|---|---|---|---|
| in0 | +12 V | ×12 | ±5 % | pewne |
| in1 | +5 V | ×5 | ±5 % | pewne |
| in2 | CPU SoC | ×1 | – | pewne |
| in3 | CPU VDDIO | ×2 | – | pewne |
| in4 | CPU Core | ×1 | – | pewne |
| in8 | +3,3 V | ×1 | ±5 % | prawie pewne |
| in5–in7 | nieznane | – | – | ukryte |
| in9–in13 | VSB, AVSB, VTT, VBAT, VREF | ×1 | – | etykiety sterownika, grupa „Inne”; VSB i VBAT z nazwą wbudowaną (§5.1) |

### 6.4 Dodawanie innej płyty

1. Plik `data/boards/<id>.toml` z mapą złączy, rolami i minimami. `[match]` wymienia nazwy modeli (`board_names`, D28), także gdy pod kodem płyty znamy dziś tylko jeden model.
2. **Źródła** (§2; z notatek z analizy interoperacyjności tylko wnioski opisane własnymi słowami, bez danych producenta):
   - instrukcja płyty: nazwa modelu, nazwy i liczba złączy wentylatorów;
   - publiczny schemat kanałów rodziny: dokumentacja i kod sterownika `nct6683` (customer ID, kanały `fanN` i `pwmN`), konfiguracje lm-sensors (mnożniki napięć, etykiety), publiczne zgłoszenia;
   - test właściciela na sprzęcie: przypisanie złączy (wentylator podłączany kolejno do złączy) i zapisy według D16.
3. **Profil bez testu na sprzęcie:** każdy wentylator ma `verified = false` i puste `tested_writes`. Taki profil daje podgląd bez sterowania: odczyty, nazwy i złącza z dopiskiem o niesprawdzonym przypisaniu, bez żadnego zapisu (D16).
4. Testy jednostkowe dopasowania DMI dla nowej płyty: kod, nazwa modelu (także inny model o tym samym kodzie) i rewizja.
5. Sterowanie wentylatorami włącza się samo, gdy sterownik jądra udostępnia `pwmN_enable` dla kanałów z profilu, a test właściciela potwierdził zapis (`tested_writes`, D16). Kod się nie zmienia.

---

## 7. Kamienie milowe

| M | Nazwa | Zakres w skrócie | Root przy instalacji | Zależność zewnętrzna |
|---|---|---|---|---|
| M1 | Przegląd i informacje o systemie | szkielet aplikacji, wykrywanie płyty, profile, odczyt czujników, strona System tylko do odczytu (DMI, pamięć z bazy udev, VBIOS, firmware dysków), „Kopiuj informacje o systemie” | nie (`meson install` do katalogu domowego wystarcza do testów) | udev z `dmi_memory_id` (systemd) |
| M2 | Wentylatory | M2a podgląd krzywych i źródła (bez pomocnika i zapisu, 0.2.0); M2b pomocnik jako osobny pakiet systemowy, tryb ręczny (0.3.0) i edytor krzywych (0.4.0); M2c wybór źródła temperatury (po łatce 5) (D46, D47) | tak od M2b (pakiet `baseboard-helper`) | M2a: brak. M2b: tryb ręczny wymaga `pwmN_enable` (`hwmon-next` albo moduł testowy), edytor zapisu krzywych (v1, moduł testowy, §1.4); wersji jądra nie podajemy, dopóki zmiany nie są w drzewie Linusa (D41) |
| M3 | RGB | hidraw, raporty 0x50/0x51, odczyt wersji firmware, reguła udev w pakiecie RPM | tak, raz (reguła udev) | brak |
| M4 | Wydajność + restart do UEFI | profile energii, podgląd cpufreq, restart do UEFI, profil energii w „Kopiuj informacje” | nie | tuned-ppd lub power-profiles-daemon, logind |
| M5 | Aktualizacja BIOS-u bez flashowania z systemu | sprawdzenie wersji z warunkami dopasowania i wynikiem trójstanowym (opcja, domyślnie wyłączona), pobranie z sumą kontrolną, pendrive FAT32, restart do UEFI, lista kontrolna | nie | sieć tylko na żądanie, UDisks2, logind |

**Stan prac** (jedyne miejsce w dokumencie; aktualizowany w commicie, który kończy krok): M1 ukończony (2026-11-11, kroki 1–13, wynik kryteriów w wierszu kroku 13); M2: plan i makiety zaakceptowane 2026-11-11 (§7, M2; D45–D56), zmiany z M2.11 wprowadzone; kroki 1 (warstwa sterownika do odczytu), 2 (model krzywej: liniowo, z pełnych stopni, zaokrąglenie w górę; zgodny ze sprzętem przy 42–86 °C, także pod obciążeniem `stress-ng`), 3 (widżet wykresu `app::fan_chart` z trybem demonstracyjnym w buildzie debug), 4 (strona Wentylatory w podglądzie z podstroną krzywej; przyjęty przez właściciela po czterech poprawkach wyglądu, D57 przyjęte) i 5 (tryb ręczny wentylatora w podtytule na Przeglądzie, D59 przyjęte; krok czeka na przyjęcie) zrobione 2026-11-11; po przeglądzie właściciela strona Wentylatory dopracowana według D60 (tryb jako tekst z kłódką, legenda wykresów, „Szczegóły krzywej”, opis zakresu, równe ramki w rzędzie, wiersz bez sterowania z ikoną informacji, nowym tekstem i przyciskiem „Stan w jądrze”, ten sam tekst bez ostrzeżenia na stronie System); 2026-11-12 układ kart według jednej reguły zależnej od szerokości (D61: nazwy bez łamania, wartości obok albo pod nazwą, jedna lub dwie kolumny, równe rzędy, stała wysokość karty; legenda „przedłużenie krzywej”), sprawdzony testami i zrzutami przy 1280, 1100 i 960 px w trzech stanach drzewa testowego, przyjęty przez właściciela 2026-11-12; tego dnia w ramce złącza niesprawdzonego ostatnie słowo przestało zostawać samo w wierszu; następny jest krok 6 (domknięcie M2a i wydanie 0.2.0). Flatpak (D39, D40): krok F1 zrobiony 2026-11-11 (manifest w `build-aux/flatpak/`, libsensors, źródła Cargo bez sieci, zmiany dla piaskownicy, porównanie z instalacją natywną w §10.3, `strace` w piaskownicy bez otwarć do zapisu w `/sys`, przeładowanie modułu przy działającej aplikacji z Flatpaka sprawdzone z właścicielem, sekcja README); następny jest F2 przed pierwszym wydaniem (§10.6). Strona projektu (2026-11-11, D41): `website/` po angielsku i po polsku, ze zrzutami aplikacji i sekcją o pracy w jądrze, wdrażana przez GitHub Pages (`.github/workflows/website.yml`); w README sekcja „Kernel work” i adres strony bez dopisku „coming soon”; strona działa pod https://baseboard.bojanowski.me, Pages włączone (2026-11-11: źródło GitHub Actions, domena własna, wymuszone HTTPS). F2, wydanie 0.1.0 (D42–D44): commity gotowe 2026-11-11 (decyzje, przebudowa tłumaczeń, czujniki NVMe, zrzuty pod stałymi adresami, metainfo z kontrolą wersji, dokumentacja instalacji, sekcja README o tym, jak powstaje projekt, procedura §10.7); push `main`, podpisany tag v0.1.0 (na `abe400c`) i wydanie na GitHubie z plikiem `.flatpak` zrobione przez właściciela 2026-11-11 (suma pliku pobranego z wydania równa lokalnej); Baseboard 0.1.0 opublikowany 2026-11-11 w repozytorium Flatpak flatpak.bojanowski.me, podpisany kluczem repozytorium (gałęzie openExpress i Vumi bez zmian, opis repozytorium ogólny: „Apps by Tomasz Bojanowski”); po wdrożeniu pliki `summary*` z serwera równe lokalnym, w pustym katalogu Flatpak widać trzy aplikacje, a instalacja z `me.bojanowski.Baseboard.flatpakref` przechodzi z weryfikacją podpisu. README ma nagłówek jak strona (ikona, opis, linki, zrzut jasny lub ciemny według motywu GitHuba). F3 po wydaniu 0.4.0 (D44, D51). Publikacja (2026-11-11): repozytorium na GitHubie zaczyna się od jednego commita początkowego; historia sprzed publikacji w archiwum lokalnym. Przed publikacją zasady lokalne, ścieżki do notatek i plan budowania modułu testowego przeszły do plików poza repozytorium, rendery makiet (`makiety/png/`, tworzy je `makiety/renderuj.sh`) wypadły z repozytorium, doszły plik LICENSE (GPL-3.0), nagłówki SPDX i adresy: strona https://baseboard.bojanowski.me, zgłoszenia i kod na GitHubie (`config.rs`, metainfo, okno „O programie”, README). Ikona docelowa wstawiona w miejsce roboczej (2026-11-11: `data/icons/hicolor/scalable/apps/me.bojanowski.Baseboard.svg` i `…/symbolic/apps/me.bojanowski.Baseboard-symbolic.svg`; symboliczna przyjmuje kolor tekstu w jasnym i ciemnym motywie; tego samego dnia symboliczna zastąpiona poprawioną, bo poprzednia była wypełnionym kwadratem i w pasku bocznym wyglądała jak klocek, a nowa to ramka z elementami w środku). Poza krokami (2026-11-11): dopasowanie profilu po nazwie modelu (D28), kryterium CPU (D29), odtworzenie treści, które zginęły bez decyzji przy przejściu z 0.5 na 0.6 (tabela progów w §4, B2, B8, B10, §5.5.3, §5.5.4, testy M2), decyzje z tego przeglądu (D30–D34), wybory z kroku 10 (D35, D36), poprawki z przeglądu M1 (D37, D38). Otwarte z M1, przeniesione do M2: test z Orca (robi właściciel); pomiar CPU przy ukrytym oknie w buildzie release (w kroku 8 w buildzie debug 0,00 %; dziś okno nie dało się zminimalizować przez `BASEBOARD_DEBUG_MINIMIZED`, więc pomiar wymaga ręcznej minimalizacji) i na module Fedory (D29). Zrzuty ekranu w metainfo i `<releases>` zrobione w F2 (D43). Zamknięte 2026-11-11: plik `.desktop` i metainfo przebudowują się po zmianie tłumaczeń (`custom_target` z `msgfmt` zależny od `po/LINGUAS` i plików `.po` zamiast `i18n.merge_file`, który zależał tylko od pliku `.in`). Do testu obciążeniowego M2b (O10): po 12 s pełnego obciążenia temp1 doszło do 92 °C przy progu 95 °C. Przed pierwszym wydaniem: sprawdzenie znaku towarowego w bazach (D1; kolizje z oprogramowaniem sprawdzone ponownie 2026-11-11).

Każdy etap kończy się, gdy spełnia swoje kryteria oraz wspólne wymagania z §8.

**Dlaczego wentylatory zaraz po M1** (D9):
- sterownik powstaje teraz, a aplikacja jest jego pierwszym użytkownikiem; problemy z ABI wyjdą, zanim v1 zostanie przyjęta;
- M2a nie potrzebuje roota ani zapisu i działa na każdym jądrze;
- RGB, Wydajność i aktualizacja BIOS-u nie zależą od jądra, więc późniejsza kolejka nic im nie zabiera.

### M1 – Przegląd i informacje o systemie (tylko odczyt)

**Zakres**
- Workspace cargo (`core`, `app`), meson, gettext (en, pl), `.desktop`, metainfo, ikona robocza, schemat GSettings.
- Okno z paskiem bocznym. Widoczne strony Przegląd i System; pozostałe pojawią się w kolejnych etapach.
- Wykrywanie płyty (§6.1), parser i walidator profili, profil MS-7E61.
- Źródło czujników jako cecha (trait) z dwiema implementacjami: libsensors i surowy sysfs (do testów i jako zapas).
- Wszystko z §5.1.
- Z §5.4 część „Informacje” i „Kopiuj informacje o systemie” (bez profilu energii i firmware RGB). Odczyt bazy udev przez libudev, za cechą z atrapą do testów.

**Kryteria ukończenia**
- Na MS-7E61 aplikacja pokazuje te same odczyty co `sensors`, z wartościami zgodnymi z dokładnością do jednego odświeżenia. Po włączeniu etykiet z lm-sensors także te same nazwy. fan3, temp5 i in5–in7 są ukryte, SYS 1–4 w kolejności złączy.
- Działa jako zwykły użytkownik, bez pomocnika i bez sieci. `strace -f -e trace=openat` nie pokazuje otwarcia niczego w `/sys` do zapisu.
- Przy odświeżaniu co 1 s sama aplikacja zużywa poniżej 1 % jednego rdzenia: czas użytkownika wszystkich wątków plus czas jądra wątku GTK (`pidstat -t`, build release, okno na wierzchu). Przy ukrytym oknie zużycie jest praktycznie zerowe (D29).
- Koszt odczytu sprzętu podajemy osobno, jako pomiar, a nie kryterium. Na MS-7E61 to ok. 0,63 % jednego rdzenia dla `nct6683` i ok. 0,2 % dla pozostałych układów. To czas jądra w sterownikach, w `nct6683` dostęp do rejestrów EC przez porty I/O z aktywnym czekaniem: pierwszy odczyt po upływie sekundy odświeża wszystkie rejestry (ok. 6,3 ms), następne biorą je z pamięci podręcznej sterownika. Według kodu sterownik v1 czyta przy odświeżeniu 112 rejestrów, a sterownik Fedory 108 (bez wypełnień `pwm13–16`). Krzywe (`pwmN_auto_point*`) i `pwmN_enable` są poza cyklem odświeżania: każdy odczyt atrybutu czyta rejestr z EC, a aplikacja w M1 nie czyta ich przy każdej próbce (od M2a tryby tak, na Przeglądzie i na stronie Wentylatory; krzywe przy wejściu na stronę Wentylatory i co 10 s, M2.4).
- Na komputerze bez profilu (albo w maszynie wirtualnej bez hwmon) uruchamia się tryb ogólny bez błędów.
- Na każdym z trzech wariantów próbek wykryty poziom sterownika zgodny z §5.5.2; na jądrze Fedory 7.3.3 i na module testowym zgodny z atrybutami w sysfs.
- Cały interfejs po polsku i angielsku (`LANGUAGE=pl` i `LANGUAGE=en`), liczby w formacie lokalnym.
- Dane DMI zgodne z `/sys/class/dmi/id`. Pamięć zgodna z `udevadm info -p /devices/virtual/dmi/id`, VBIOS i firmware dysków zgodne z sysfs.
- Tekst „Kopiuj informacje” zawiera pola z §5.4 dostępne w M1 i nie zawiera numerów seryjnych, adresów MAC ani listy programów.

**Testy**
- Jednostkowe:
  - dopasowanie DMI: ta płyta, inne płyty MSI, inna rewizja PCB, inni producenci, puste pola,
  - parser profili: poprawne, błędne, nieznane role, brak pól, zła wersja `schema`,
  - kolejność nazw (profil, nazwy wbudowane, sterownik; opcja lm-sensors),
  - zasada „bez podwójnego przeliczania”,
  - formatowanie liczb pl/en,
  - przeliczanie wypełnienia (§3.5): każdy pełny procent 0–100 po obcięciu (jak w BIOS-ie) wraca w interfejsie do tego samego procentu; 76 → „30 %”, 127 → „50 %”; stałe granice B5 równe 51 i 102,
  - właściwości pamięci z udev: gniazdo puste, „Unknown”, brak właściwości, brak całej bazy; nazwy gniazd dla czujników `spd5118` i zapasowe „Moduł pamięci *N*”,
  - format tekstu do schowka; test, że tekst nie zawiera numeru seryjnego ani adresu MAC, nawet gdy sztuczne dane je mają.
- Integracyjne na sztucznych drzewach sysfs w `tests/fixtures/sysfs-ms7e61-*/` (trzy warianty sterownika, krok 4), zbudowanych z odczytów z tej maszyny. Katalog główny sysfs można podmienić tylko w aplikacji, nigdy w pomocniku.
- Ręczne na MS-7E61: porównanie z `sensors`, obciążenie CPU (temperatura i CPU Core rosną), przełączenie języka, czytnik ekranu (Orca) odczytuje nazwy i wartości.

**Kroki.** Każdy krok kończy się działającym buildem (`meson compile`), zielonymi sprawdzeniami z §8 i osobnym commitem.

| # | Krok | Wynik |
|---|---|---|
| 1 | **Szkielet.** Workspace cargo (`crates/core`, `crates/app`), meson, gettext (en, pl) z pustym `pl.po`, `.desktop`, metainfo, schemat GSettings, robocza ikona. `meson test` uruchamia `cargo test` i walidatory plików danych. | Zrobione (2026-11-06; historia sprzed publikacji w archiwum lokalnym). Okno z `AdwNavigationSplitView`, paskiem bocznym z pozycją „Przegląd” i pustą `AdwStatusPage`. W buildzie debug okno zapisuje własny zrzut do PNG, zawsze poza drzewem projektu: do katalogu z `BASEBOARD_DEBUG_SCREENSHOT_DIR`, domyślnie `$XDG_CACHE_HOME/baseboard/`. |
| 2 | **Profil płyty.** Typy profilu w `core`, parser TOML i walidator (`schema`, role, klucze, zakresy, minima B5), `data/boards/ms-7e61.toml`, instalacja profili przez meson. | Zrobione (2026-11-10). Testy parsera: poprawne, błędne, nieznane role, brak pól, zła wersja `schema`, minima B5, `tested_writes`, ukryte kanały. |
| 3 | **Wykrywanie płyty.** Odczyt DMI bez numerów seryjnych z podmienialnego katalogu głównego sysfs, kod płyty i rewizja PCB (§6.1), wybór profilu. | Zrobione (2026-11-10). Testy dopasowania DMI: ta płyta, inne płyty MSI, inna i nieznana rewizja PCB, inni producenci, puste i brakujące pola, kolejność katalogów profili. Pasek boczny pokazuje model płyty. |
| 4 | **Odczyt sysfs i poziom sterownika.** Cecha źródła czujników w `core` i implementacja na surowym sysfs hwmon (urządzenie po `name` i ścieżce, nie po numerze `hwmonN`). Wykrycie możliwości kanałów po atrybutach (§5.5.2) w `core::fans::driver`, na razie tylko odczyt. Trzy sztuczne drzewa w `tests/fixtures/` z odczytów tej maszyny i testów sterownika: `sysfs-ms7e61-fedora-7.3.3` (`pwm1–4` 0444), `sysfs-ms7e61-hwmon-next` (`pwm1`, `pwm2`, `pwm13–16` z `_enable` 0644, krzywe 0444), `sysfs-ms7e61-v1` (krzywe 0644). Git nie przechowuje bitów zapisu, więc każde drzewo ma plik `tryby` z trybami plików; test kopiuje drzewo do katalogu tymczasowego i je nakłada. Drzewo v1 ma też `tryby-mieszane`: krzywa `pwm13` 0444, jak dla kanału, którego krzywa nie spełnia warunku przy ładowaniu (§5.5.1). | Zrobione (2026-11-10). Testy integracyjne na trzech drzewach: wykryty `DriverLevel` `read-only`, `manual+curve-view`, `curves`; z `tryby-mieszane` możliwości liczone osobno dla kanału (SYS 4 bez `curve-write`) i `DriverLevel` `manual+curve-view`. Wypełnienie SYS 1–4 jest w odczytach tylko tam, gdzie są `pwm13–16` (test na danych; strona Przegląd w kroku 8). Na jądrze Fedory 7.3.3 wykryty `read-only`, zgodny z atrybutami w `/sys`; `strace` bez otwarć do zapisu w `/sys`. Z modułem v1 (2026-11-10): `curves`, pliki `pwm*` i ich tryby jak w `sysfs-ms7e61-v1`; inne są tylko bieżące wypełnienia i atrybuty spoza `pwm*`, których próbki celowo nie zawierają. |
| 5 | **libsensors.** Moduł FFI w `app` (jedyne `unsafe` tej części, z `// SAFETY:`), `ignore`, `compute` i etykiety z `/etc/sensors.d`, zasada „bez podwójnego przeliczania”. Gdy libsensors zawiedzie, aplikacja wraca do sysfs. | Zrobione (2026-11-10). libsensors przez `dlopen`; połączenie z sysfs i wykrywanie `compute` w `core::sensors`. Testy na drzewie Fedory z konfiguracją płyty (napięcia przeliczone dokładnie raz z libsensors i bez niego, `ignore`, etykiety, szum odczytów), na prawdziwym `/sys` z własną konfiguracją (`compute`, `ignore`, `label`, wartości bez konfiguracji równe sysfs, błędna linia, brak biblioteki, jedna instancja) i wyboru źródła z powrotem do sysfs. Na MS-7E61 z modułem v1 odczyty zgodne z `sensors`; +12 V, +5 V i CPU VDDIO przeliczone raz (przez `compute`, bez mnożnika z profilu); `strace` bez otwarć do zapisu w `/sys`. |
| 6 | **Model Przeglądu.** W `core`, bez tekstów: grupy, kolejność wentylatorów według złączy, ukryte odczyty, ocena tolerancji napięć, wybór źródła nazwy (profil, nazwy wbudowane, sterownik; opcja lm-sensors). Nazwa karty graficznej z `product_name`, potem z hwdb udev (cecha z atrapą w `core`, moduł FFI `app::libudev`, D21). Teksty nazw przez gettext w `app`. | Zrobione (2026-11-10). `core::overview`: grupy w kolejności strony (karta graficzna: osobna grupa na kartę), wentylatory w kolejności złączy z wypełnieniem tylko przy `duty-read`, temperatury i napięcia w kolejności ról, reszta urządzenia płyty w „Inne”, ocena napięć (granica tolerancji należy do tolerancji). Testy na trzech drzewach i bez profilu: grupowanie, kolejność, ukrywanie (profil i `ignore`), tolerancje, nazwy (profil, `label`, nazwy wbudowane, sterownik, opcja lm-sensors), nazwa karty z `product_name` i z bazy udev. Na MS-7E61 model zgodny z makietą Przeglądu; `strace` bez otwarć do zapisu w `/sys`. |
| 7 | **Liczby i jednostki.** Formatowanie według lokalizacji (separator dziesiętny z `LC_NUMERIC`), spacja przed jednostką, zakresy min–maks. Przeliczanie wypełnienia 0–255 ↔ % według §3.5. | Zrobione (2026-11-10). `core::units`: zasady z §5.1 w jednym miejscu i para funkcji przeliczania wypełnienia z §3.5; separator z `nl_langinfo` w `app::i18n` (D24), spacja U+00A0 (D25), nazwy szyn ATX z napięcia nominalnego (D26). Testy pl i en (wartości, zakresy, nominały, odchyłki, tolerancje), test przeliczania (każdy pełny procent 0–100 wraca do siebie; 76 → 30 %, 127 → 50 %, granice B5 51 i 102), separatory prawdziwych lokalizacji `pl_PL` i `en_US`. Na MS-7E61 log modelu po polsku, po angielsku i mieszany (`LANGUAGE=en`, liczby pl). |
| 8 | **Strona Przegląd.** Grupy i wiersze (`AdwPreferencesGroup`, `AdwActionRow`), dwie kolumny przy szerokim oknie (`AdwBreakpoint`), odczyt w wątku roboczym co 1 s, minimum i maksimum, wstrzymanie przy niewidocznym oknie. Źródło odczytów tworzone w wątku roboczym i na nowo, gdy zmieni się zestaw urządzeń hwmon. | Zrobione (2026-11-10). Strona w `app::overview` (`overview.ui`): grupy `AdwPreferencesGroup` z wierszami `AdwActionRow`, dwie kolumny od 760 sp (`AdwBreakpoint`; po prawej Napięcia, Karta graficzna, Dyski i Inne), jedna przy węższym oknie. Odczyt w wątku `app::sampler`: źródło (z wykrywaniem `compute`) powstaje w wątku i od nowa, gdy zmieni się zestaw urządzeń hwmon (wpis, ścieżka, i-węzeł); próbka co 1 s trafia do pętli GLib; odczyt stoi, gdy okno jest niewidoczne albo zawieszone przez kompozytor. Każdy czujnik jest czytany raz na próbkę (libsensors dla swoich układów, sysfs dla pozostałych i dla wypełnień). Minimum i maksimum od uruchomienia (`core::overview::Extremes`), ocena napięć ikoną i tekstem, VSB, AVSB, VTT, VBAT i VREF w rozwijanym wierszu „Napięcia pomocnicze”, w każdym wierszu nazwa i wartość dla czytnika ekranu. Wartości w `GtkInscription`, więc nowa liczba nie przelicza układu okna. Napięcia poniżej 2 V z trzema miejscami po przecinku (D27). Testy: zmiana zestawu urządzeń na sztucznych drzewach (Fedora → v1 → bez urządzenia) odtwarza źródło, wstrzymanie i wznowienie wątku, zakresy min–maks, napięcia pomocnicze, D27. Na MS-7E61 z modułem v1: `strace` bez otwarć do zapisu w `/sys`; CPU (`pidstat`, 60 s, build debug) przy widocznym oknie 1,0–1,35 % jednego rdzenia, przy zminimalizowanym 0,00 %. Z tego wątek GTK ok. 0,1 %, a wątek odczytu ok. 0,9 %, prawie cały w jądrze: pierwszy odczyt `nct6687` po upływie sekundy odświeża wszystkie rejestry przez EC i kosztuje ok. 6,3 ms CPU (następne 0,01 ms), pozostałe układy razem ok. 2 ms. Makieta Przeglądu poprawiona do decyzji (zakresy z jednostką, podtytuły, D23, D27). |
| 9 | **Źródła danych strony System.** Odczyt bazy udev przez libudev za cechą z atrapą: pamięć (bez pól `SERIAL`), nazwy PCI z hwdb, karty sieciowe (bez adresów MAC). VBIOS, dyski z firmware i rozmiarem, `/etc/os-release`, procesor, sterownik cpufreq, stan `nct6683`, konfiguracja lm-sensors, programy mogące konfliktować przy sterowaniu wentylatorami. Nazwy gniazd dla czujników `spd5118` w Przeglądzie. | Zrobione (2026-11-11). `core::system`: dane strony System bez tekstów, z podmienialnymi katalogami `/sys`, `/proc`, `/etc` i `/usr/lib`; baza udev za cechą `core::udev::Udev` (moduł `app::libudev` bez nowych wywołań FFI, wystarcza odczyt właściwości). Pamięć, BIOS, system, procesor, cpufreq, karty graficzne (nazwa, identyfikatory PCI, sterownik, VBIOS), dyski, karty sieciowe, stan `nct6683` i `DriverLevel`, pliki lm-sensors dla układu płyty, programy sterujące wentylatorami (§5.4). W Przeglądzie opis grupy Procesor i nazwy gniazd modułów pamięci (§5.1). Strony System jeszcze nie ma: dane trafiają do dziennika debug w osobnym wątku. Testy na sztucznym systemie z odczytów tej maszyny, z numerami seryjnymi (DMI, NVMe, USB, moduły pamięci) i adresem MAC w sysfs i w atrapie bazy udev: nie ma ich w modelu, a o właściwości z `SERIAL` i `MAC` kod nie pyta; dalej pliki zapasowe, puste gniazda i wartości „Unknown”, nazwy gniazd (jednoznaczne i nie), system bez danych. Na MS-7E61 dane zgodne z `udevadm info` i sysfs, Przegląd pokazuje „AMD Ryzen 7 9700X”, DIMMA2 i DIMMB2; `strace` bez otwarć do zapisu w `/sys` i bez otwarć plików z numerem seryjnym, UUID albo adresem MAC. Makieta Przeglądu poprawiona (gniazda pamięci). |
| 10 | **Strona System i „Kopiuj informacje o systemie”.** Grupy jak w makiecie, bez restartu do UEFI; poziom obsługi wentylatorów z `DriverLevel` (§5.5.3). Tekst do schowka z polami dostępnymi w M1. | Zrobione (2026-11-11). Strona w `app::system` (`system.ui`, `AdwPreferencesPage`) w pasku bocznym obok Przeglądu, treść grup jako czysta funkcja z testami; wątek `app::system_reader` czyta przy wejściu na stronę i po zmianie urządzeń hwmon. Tekst do schowka w `core::report` (po angielsku), akcja `win.copy-system-info` aktywna po pierwszym odczycie, `AdwToast` po skopiowaniu. Testy: pełny tekst dla sztucznego MS-7E61 jako wzorzec (bez pendrive'a, programów, numerów seryjnych i adresu MAC, które sztuczny system ma), tekst bez danych, wiersze profilu (niesprawdzona rewizja, inny model, brak układu płyty), grupy bez danych i pola „Brak danych”, poziom obsługi słowami bez numeru wersji jądra, gniazda pamięci i nośnik wymienny, wątek (odczyt przy wejściu, po zmianie urządzeń, nie bez zmiany ani przy nieaktywnej stronie). Na MS-7E61 strona i tekst do schowka z prawdziwymi danymi, po polsku i po angielsku, przy szerokim i wąskim oknie. Makieta System poprawiona do zakresu M1. |
| 11 | **Ustawienia.** Okno preferencji: interwał odświeżania, etykiety z lm-sensors, ukryte odczyty i złącza. Etykiety użytkownika z `~/.local/share/baseboard/boards/` (D22; format najpierw w §6.2). Okno „O aplikacji”. | Zrobione (2026-11-11). `app::preferences`: `AdwPreferencesDialog` ze stronami „Ogólne” (interwał 1–10 s, nazwy z lm-sensors, ścieżka pliku własnych nazw) i „Odczyty” (przełącznik i wartość dla każdego odczytu, także złączy bez wentylatora); klucze GSettings z §3.2; okno stosuje zmiany od razu (wątek odczytu dostaje nowe opcje i robi próbkę, Przegląd filtruje ostatnią próbkę, `core::overview::visible`). Własne nazwy w `core::labels` według §6.2, pierwszeństwo przed lm-sensors. Menu główne: Preferencje (Ctrl+,), Skróty klawiszowe (`AdwShortcutsDialog` z `shortcuts-dialog.ui`, Ctrl+?), O programie (`AdwAboutDialog`: wersja, GPL-3.0-or-later, autor, podziękowania dla publicznych opisów sprzętu z §2; linki do repozytorium i zgłoszeń dopiero z adresami, stan prac). Tłumaczenia menu i okna „O programie” uzupełnione. Przy okazji (do kroku 10): pochodzenie modułu `nct6683` na stronie System i w tekście do schowka (§5.4). Testy: zapis i odczyt ustawień na prawdziwym schemacie (kompilowanym w teście, ustawienia w pamięci), opcje w działającym wątku bez czekania na interwał, ukrywanie odczytów w modelu, pierwszeństwo nazw przy przełączaniu źródła, parser własnych nazw, pochodzenie modułu z `taint` (z jądra, spoza, wbudowany). Na MS-7E61 zmiana ustawień przez `gsettings` w trakcie działania od razu ukrywa pompę i zmienia podtytuł na „co 2 s”. |
| 12 | **Tryb ogólny i błędy.** Brak profilu, brak hwmon, inna rewizja PCB, brak właściwości pamięci w bazie udev: `AdwStatusPage` albo ostrzeżenie zamiast błędu. | Zrobione (2026-11-11). Stany z §5.1 i §5.4: nieczytelna wartość jako „–” (`core::sensors` zostawia odczyt z NaN, `core::units` pisze „–”, zakres i ocena ją pomijają); wątek odczytu podaje stan próbki (libsensors albo sysfs z powodem, brakujący układ płyty), Przegląd pokazuje baner, a strona System wiersz biblioteki; wskazówka sprawdzenia sterownika; pominięte profile i plik własnych nazw na stronie System, w powiadomieniu i na stderr (powód w jednym wierszu). Brak profilu, inny model (D28) i niesprawdzona rewizja działały już wcześniej; teraz mają testy strony System. Testy: nieczytelny atrybut na drzewie Fedory (folder zamiast pliku i nieliczbowa treść), układ płyty znika i wraca w wątku odczytu, brak urządzeń hwmon, maszyna z DMI bez hwmon (dane systemu i tekst do schowka), błędny profil obok dobrego, wiersze profilu (sprawdzona, niesprawdzona i nieznana rewizja, inny model, brak profilu), wiersz libsensors, brak układu płyty z poleceniami, pominięte pliki, wybór źródła przy braku libsensors. Na MS-7E61 uruchomienia z `BASEBOARD_SYSFS_ROOT` na drzewie bez profilu, bez hwmon i bez układu płyty, oraz z błędnym profilem i błędnym plikiem własnych nazw; przeładowanie modułu na sprzęcie robi właściciel. |
| 13 | **Domknięcie M1.** Pełne tłumaczenie pl, metainfo ze zrzutami ekranu, README, przejście kryteriów M1 (porównanie z `sensors` i `udevadm info`, `strace`, `pidstat`, Orca, `LANGUAGE=en`). | Zrobione (2026-11-11), M1 ukończony. Poprawki interfejsu z przeglądu (D37, D38); pełne tłumaczenie pl, także pliku `.desktop`, metainfo i kluczy okna; opis w metainfo z wiersza o stronie System. Przeładowanie modułu `nct6683` przy otwartej aplikacji sprawdził właściciel. Kryteria na MS-7E61 z modułem v1: odczyty zgodne z `sensors` w granicach jednego odświeżenia, a z etykietami lm-sensors te same nazwy; fan3, temp5 i in5–in7 ukryte; SYS 1–4 w kolejności złączy; `strace` bez otwarć do zapisu w `/sys`, bez plików z numerem seryjnym, UUID albo adresem MAC i bez `/dev/port`, `/dev/mem` i hidraw. CPU w buildzie release przy widocznym oknie (`pidstat -t`, 60 s, odświeżanie co 1 s): sama aplikacja 0,35 % jednego rdzenia (czas użytkownika wszystkich wątków 0,35 %, czas jądra wątku GTK 0,00 %), odczyt sprzętu osobno 0,88 % (czas jądra wątku odczytu), razem 1,27 %. DMI, pamięć, VBIOS, firmware dysków, system i jądro zgodne z sysfs i `udevadm info`; tekst do schowka bez numerów seryjnych i adresów MAC (test i `strace`); interfejs po polsku i po angielsku. Obciążenie wszystkich rdzeni przez 12 s: Tctl do 92,1 °C, temp1 do 92,0 °C, CPU_FAN1 z 887 do 1096 rpm. Bez kroku 13: zrzuty ekranu w metainfo (wymagają publicznych adresów) i test z Orca; przechodzą do M2 (stan prac). |

### Po M1: profile innych płyt (plan, jeszcze bez wykonania)

**Zasada.** Profil innej płyty powstaje tylko z dwóch źródeł:
- instrukcja tej płyty: nazwa modelu, nazwy i liczba złączy wentylatorów;
- publiczny schemat kanałów jej rodziny (§6.4 pkt 2).

Przynależność płyty do rodziny też musi wynikać ze źródła publicznego. Danych z notatek z analizy interoperacyjności (§2) profil nie zawiera. Każdy wentylator ma `verified = false` i puste `tested_writes`, więc profil daje podgląd bez sterowania (§6.4 pkt 3, D16). Gdy źródła się różnią, rozstrzyga instrukcja. Przykład: schemat rodziny ma kanał EZ Conn, a instrukcja nie wymienia takiego złącza, więc profil go nie pokazuje.

**Kandydaci na profile niesprawdzone.** Każdą płytę potwierdzamy w jej własnej instrukcji. `[match]` wymienia nazwę modelu (D28).

| Kod | Model | Stan |
|---|---|---|
| MS-7E56 | B850 GAMING PLUS WIFI | do potwierdzenia w instrukcji |
| MS-7E56 | MAG B850 GAMING PLUS MAX WIFI | do potwierdzenia w instrukcji |
| MS-7E56 | PRO B850-P WIFI | do potwierdzenia w instrukcji; przy D28 bez złącza JAF_1 |
| MS-7E75 | B850 GAMING PLUS WIFI PZ | do potwierdzenia w instrukcji |
| MS-7E80 | B850 GAMING PLUS WIFI6E | do potwierdzenia w instrukcji |
| MS-7E80 | PRO B850-S WIFI6E | do potwierdzenia w instrukcji |

Płyty o tym samym kodzie i tym samym zestawie złączy według instrukcji mogą mieć jeden profil z kilkoma nazwami. Płyta z innym zestawem dostaje osobny profil. Kolejność względem M2 ustalimy po M1.

### M2 – Wentylatory

**Plan z 2026-11-11, zaakceptowany przez właściciela tego samego dnia.** Zastępuje dotychczasowy podział na M2a–M2d. Decyzje P1–P11 to w §12 D45–D55; M2.9 podaje, który numer P to które D (w tej sekcji zostały numery P). Makiety (układ A po poprawkach z D45) są zaakceptowane, a zmiany w reszcie dokumentu z M2.11 wprowadzone.

**Warunki wstępne:** M1. M2a działa na każdym jądrze. Zapis na sprzęcie (M2b) wymaga modułu testowego (§1.4), dopóki łatki nie trafią do jądra Fedory. Każdy test zapisu na sprzęcie wymaga osobnej zgody właściciela (CLAUDE.md).

#### M2.1 Stan wyjściowy

**Zaprojektowane i nadal ważne:**
- kontrakt D-Bus `Fans1` (§3.5) z jednostkami 0–255 i przeliczaniem jak w BIOS-ie (D15, D20);
- zasady B1–B18 i progi awaryjne (§4);
- wykrywanie możliwości po atrybutach i `DriverLevel` (§5.5.2). Zrobione w M1 (krok 4: `core::fans::driver`, trzy drzewa w `tests/fixtures/`, poziom na stronie System);
- profil MS-7E61 z polami `controllable`, `min_duty`, `verified` i `tested_writes` (§6.2, §6.3), też zrobiony w M1;
- tryby w interfejsie (D11), krzywa pierwotna w `/run` (B18, D12), D16, D17, D19, D31–D34;
- podział na aplikację we Flatpaku i pomocnika w pakiecie systemowym, zgodność wersji, brak pomocnika (§10.4, D39);
- tabela „Gdy recenzja v1 coś zmieni” (koniec tej sekcji).

**Nieaktualne albo sprzeczne** w chwili planowania (zostawione jako zapis; poprawione według M2.11 albo przez plan i makiety):
1. `CLAUDE.md` mówi o zasadach B1–B15, a §4 ma B1–B18.
2. Nagłówek dokumentu podaje „decyzje D1–D20”, a są D1–D44.
3. §1.4, §5.5.1 i tabela na początku §7 podają, że `hwmon-next` trafi do jądra 7.5. To prognoza, a README i strona projektu (D41) nie podają wersji jądra przed wejściem zmian do drzewa Linusa. 2026-11-11 najnowszy tag Linusa to v7.4-rc2.
4. §5.5.2 podaje jako typowy sterownik dla `manual` „jądro z samą serią Hamzy”. W `hwmon-next` seria Hamzy wchodzi razem z odczytem krzywych, więc `manual` bez `curve-read` zdarzy się tylko przy częściowym przeniesieniu łatek (np. w dystrybucji). Poziom zostaje, ale nie jest typowy.
5. Tabela na początku §7 i dotychczasowy plan dzielą M2 na M2a–M2d. Ten plan: M2a (podgląd), M2b (pomocnik, tryb ręczny, edytor krzywych), M2c (wybór źródła, dawne M2d). Dawne M2c weszło do M2b.
6. Dawny krok 8 robił pakiet RPM na końcu trybu ręcznego. Według D39 pomocnik to osobny pakiet systemowy, którego potrzebuje także aplikacja z Flatpaka, więc pakiet powstaje na początku M2b.
7. §5.5.3 nie ma przypadku „bez pomocnika”, a od D39 to przypadek typowy (Flatpak bez pakietu `baseboard-helper`). §10.4 zakłada, że strona odsyła do pakietu `baseboard-helper`; w wydaniu z samym M2a tego pakietu nie ma, więc strona nie może do niego odsyłać.
8. §5.5.3 wymaga trybu (automat, ręczny) w podtytule wentylatora na Przeglądzie przy `manual+curve-view`. M1 tego nie robi.
9. §5.5.4: „Kliknięcie karty otwiera podstronę”. Karta z przełącznikiem w środku byłaby elementem aktywnym wewnątrz aktywnego, co psuje obsługę klawiaturą i czytnik ekranu. Makiety mają osobny przycisk „Edytuj krzywą”.
10. Makiety sprzed tego planu (zastąpione 2026-11-11): edytor z 2–8 punktami i przyciskiem „Dodaj punkt” (sterownik ma zawsze 7); źródło „Procesor (Tctl)” z `k10temp`, choć krzywe w EC liczą z temp1 (D34), a Tctl należał do krzywej programowej odłożonej w D10; opis „Procesor (Tctl): 39,8 °C → 25 %” w karcie SYS_FAN1; komentarze „M4 preview” ze starej numeracji; przełącznik trzypozycyjny na każdym poziomie; brak stanów `read-only`, `manual+curve-view`, braku pomocnika i Flatpaka; ostrzeżenie pompy przy złączu, którego zapis blokuje D16.
11. §4 nie mówi, jak EC liczy wypełnienie między punktami. Odczyt z 2026-11-11 wskazuje interpolację liniową: SYS_FAN1 przy temp1 46 °C miał `pwm16` = 98, a prosta między punktami 5 °C → 96 i 84 °C → 99 daje 97,6 (schodek dałby 96). Do sprawdzenia na wszystkich kanałach (krok 2).
12. B3 nie ma reguły dla przeładowania modułu ani dla aktualizacji i odinstalowania pakietu pomocnika, choć §4 mówi, że przy wyładowaniu sterownik niczego nie przywraca.
13. D14 zostawia wybór („automat” albo „ręczny 100 % od razu, potem automat”) bez rozstrzygnięcia. O10 nie zawiera wyniku z M1: temp1 doszło do 92 °C po 12 s pełnego obciążenia przy progu 95 °C (stan prac).
14. B4 wymaga jawnego zdjęcia blokady, ale §3.3 kończy pomocnika po 30 s bezczynności bez kanału w trybie ręcznym, a tak jest zaraz po B4. Blokada w pamięci pomocnika zniknęłaby razem z nim.
15. §3.6 („Planowana struktura”) wymienia `crates/helper`, `packaging/rpm/`, `data/dbus`, `data/polkit` i `data/systemd`. Jeszcze ich nie ma; powstają w M2b.
16. §10.6 umieszcza F3 „po M2”. Przy nowym podziale trzeba ustalić, czy chodzi o koniec M2b (edytor), czy M2c (źródło, zależne od niewysłanej łatki 5).
17. Otwarte z M1 i przeniesione do M2 (stan prac): test z Orca i pomiar CPU przy ukrytym oknie w buildzie release. Dotychczasowe kryteria M2 ich nie zawierały.

#### M2.2 Zależności od jądra

Stan z lokalnego drzewa jądra właściciela 2026-11-11; `hwmon-next` lokalnie i na serwerze to ten sam commit `44c8ceae2dd3`.

| Gdzie | Zmiany w `nct6683` | Poziom na MS-7E61 | Co daje Baseboard |
|---|---|---|---|
| Wydane jądra (7.3; Fedora 7.3.3) i drzewo Linusa do v7.4-rc2 | odczyty; w v7.4-rc2 m.in. `c4a01c2a80b2` (poprawka rzutowania w `show_pwm()` i `store_pwm()`), `f9c5d495e22f` i `04f4079ee61a` (listy płyt) | `read-only`: `pwm1–4` 0444, bez `pwmN_enable` i krzywych | M2a: karty z obrotami i wypełnieniem, wyjaśnienie |
| `hwmon-next` | `5410a1257e26` sterowanie wentylatorami płyt MSI z NCT6687D-R (seria Hamzy), `f62e305a9b72` wypełnienie wentylatorów SYS, `e0fdadf44480` odczyt krzywych, `7dc9dff9608d` tryb ręczny po wznowieniu, `9932e15ac28a`, `2722b11c864a` | `manual+curve-view` | M2a: podgląd krzywych i źródła; M2b: tryb ręczny |
| Seria v1 w recenzji na linux-hwmon (`81b1e2c64f6d`, `527bbd00913c`) | zapis punktów krzywych; `pwmN_auto_channels_temp` dalej 0444 | `curves` | M2b: edytor krzywych |
| Łatka 5 | nienapisana | `curves+source` | M2c: wybór źródła |
| Seria Dahlina (powrót do automatu przy wyładowaniu modułu) | nie ma jej w żadnej gałęzi lokalnego drzewa (szukane po autorze i po treści). Sterownik w `hwmon-next` nie ma funkcji `.remove` ani `.shutdown`, które coś przywracają | – | plan jej nie zakłada |

- **Co działa u użytkowników dziś:** na wydanych jądrach tylko `read-only`. Wyższe poziomy wymagają modułu spoza drzewa (u właściciela `hwmon-next` z v1) albo jądra, do którego trafi `hwmon-next`, a potem dystrybucji z tym jądrem.
- **Gdy v1 zmieni interfejs:** tabela „Gdy recenzja v1 coś zmieni” (koniec sekcji). Edytor powstaje pod v1, ale do wydania trafia dopiero, gdy zapis krzywych jest w `hwmon-next` (P3); wtedy warstwa sterownika i próbki idą za wersją przyjętą.
- **Gdy v1 nie zostanie przyjęta:** M2a i tryb ręczny od niej nie zależą i wychodzą normalnie. Podgląd krzywych zostaje, bo odczyt jest w `hwmon-next`. Edytor zostaje na `main` bez wydania (pomocnik i tak go nie włączy bez `curve-write` w atrybutach) albo go usuwamy. Nowa propozycja zapisu w jądrze to nowy wiersz tabeli zmian.
- **Gdy `hwmon-next` zmieni się przed wejściem do drzewa Linusa** (np. cofnięcie serii): poziom wykrywany po atrybutach spada sam, bez zmian w kodzie; przesuwa się plan wydań, nie kod.
- **Do uzupełnienia, gdy sterowanie wejdzie do wydanego jądra (D41, D60):** numer wersji Linuksa w zdaniu „Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.” (strona Wentylatory i wiersz strony System; wtedy np. „Sterowanie wymaga Linuksa X.Y lub nowszego.”), w README i na stronie projektu (sekcja o jądrze).
- **Seria Dahlina a zasady bezpieczeństwa:** dziś po wyładowaniu modułu w trybie ręcznym zostaje stałe wypełnienie bez nadzoru, do ponownego załadowania albo restartu (§4). Pomocnik nie może temu zapobiec, bo bez sterownika nic nie zapisze. Plan: nowa reguła B3(g) (P5) i opis ryzyka szczątkowego w §4. Gdy seria wejdzie, wyładowanie samo przywróci automat, a B3(g) niczego nie zmieni. Jeśli przy wyładowaniu przywraca też krzywe BIOS-u, migawka B18 dalej się zgadza; pomocnik po powrocie urządzenia porówna krzywe z migawką i z tym, co sam zapisał (P5), bez założeń o zachowaniu sterownika, tak jak przy wznowieniu (B12).
- **Wybór źródła temperatury** dopiero po dowodzie sprzętowym: (a) łatka 5 w kształcie wysłanym na listę (D17); (b) test na MS-7E61, że po zmianie źródła EC liczy wypełnienie z nowej temperatury (np. źródło temp3, sekcja zasilania nagrzana obciążeniem, `pwmN` zgodne z krzywą dla temp3, a nie dla temp1); (c) źródło po S3 i po restarcie. Do tego czasu źródło jest tylko do podglądu.

#### M2.3 Etapy, kroki i to, co widzi użytkownik

Każdy krok kończy się działającym buildem, zielonymi sprawdzeniami z §8 i osobnym commitem. Kolumna „Na sprzęcie” wymienia to, co sprawdza właściciel; odczyt sysfs nie wymaga zgody, zapis i polecenia z `sudo` tak, za każdym razem osobno.

**M2a – podgląd krzywych i źródła, tylko odczyt, bez pomocnika (natywnie i we Flatpaku), wydanie 0.2.0**

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 1 | Warstwa sterownika do odczytu: cecha `FanDriver` (tryb, wypełnienie, krzywa, źródło), `SysfsDriver` i `ModelDriver` do odczytu, milistopnie ↔ °C, maska `pwmN_auto_channels_temp` → klucz temperatury z profilu. Kodu zapisu w tym kroku nie ma. | testy na trzech drzewach z M1 i na `tryby-mieszane`; na MS-7E61 z modułem v1 krzywe i źródła sześciu kanałów równe `cat` (odczyt). **Zrobione (2026-11-11).** W `core::fans::driver`: typy `Mode`, `CurvePoint` (°C i 0–255), `Source` (`Temp(N)`, `Mask` dla mapy bez jednej temperatury, `Unreported` przy ENODATA), `ReadError` (`Missing`, `Os(errno)`, `Invalid` z nazwą atrybutu i treścią; wartość spoza ABI nigdy nie jest zgadywana), cecha `FanDriver`, `SysfsDriver` (tylko odczyt), `read()` (każdy atrybut raz, `None` gdy sterownik go nie ma), `source_temp()` (źródło → temperatura z profilu z progiem); `ModelDriver` w `driver::model` (atrybuty, bity zapisu, wstrzykiwane błędy odczytu). Testy: parsowanie i wartości spoza ABI, mapa źródeł i ENODATA, brakujące i nieczytelne atrybuty, model; na drzewach: Fedora tylko wypełnienia `pwm1`, `pwm2`, `pwm4`, a `hwmon-next`, v1 i v1 z `tryby-mieszane` tryb, wypełnienie, krzywe i źródło równe plikom, znane krzywe CPU_FAN1 i SYS_FAN1 z §6.3. Na MS-7E61 z modułem v1 test `this_machine_reads_like_cat` (uruchamiany na żądanie): sześć kanałów w automacie, krzywe i źródło temp1 równe `cat`, EZ Conn tylko wypełnienie; `strace`: 363 otwarcia w `/sys`, żadne do zapisu | – |
| 2 | Model krzywej w `core`: n punktów z liczby plików, interpolacja liniowa, wartości poza skrajnymi punktami, cel w punkcie pracy, źródło → próg awaryjny, opis krzywej dla czytnika ekranu. | testy jednostkowe; na MS-7E61 cel z modelu równy `pwmN` w stanie ustalonym na sześciu kanałach, w spoczynku i po krótkim obciążeniu procesora (odczyt, bez zapisu do sprzętu); wynik w §4. **Zrobione (2026-11-11)**, z jednym punktem otwartym. `core::fans::curve`: `expected_duty` (liczby całkowite, milistopnie; odcinki liniowe, poza skrajnymi punktami ich wartość, zaokrąglenie w górę; punkty powtórzone i schodki nigdy nie dają odcinka zerowej szerokości; krzywa z malejącą temperaturą → brak wartości), `line_at` (prosta bez zaokrąglenia, do wykresu), `runs` (powtórzone punkty jako jeden uchwyt „6–7”). Opis krzywej dla czytnika ekranu potrzebuje tekstów z gettext, więc przechodzi do kroków 3 i 4 (`app`). Testy: krzywe z drzew testowych i §6.3 (CPU, domyślna CPU z BIOS-u, pompa, SYS 1–3, krzywe płaskie), progi zaokrąglenia, punkty powtórzone i schodek, cały zakres −50…150 °C co 0,1 °C (wartość w zakresie krzywej, niemalejąca, najwyżej jeden krok nad prostą). Na MS-7E61 test `this_machine_follows_the_curve_model` (na żądanie, tylko odczyt): trzy przebiegi, 939 s przy temp1 42–73 °C (spoczynek i dwa obciążenia właściciela po 60 s), 5634 próbki: 5617 z różnicą 0, 14 z +1, 3 z −1 (wszystkie w chwili przejścia, wyrównane w 0–2 s); 5299 próbek ustalonych, wszystkie z różnicą 0. Obciążenie `sha256sum` (instrukcje SHA procesora, mały pobór mocy) dało najwyżej 67 °C, więc strome odcinki sprawdził czwarty przebieg z `stress-ng --cpu 0 --cpu-method matrixprod --timeout 60s` (temp1 do 86 °C): 1501 próbek ustalonych, wszystkie z różnicą 0; EC liczy z pełnych stopni (§4); środek stromego odcinka CPU Fan 1 (75–85 °C) nie ustalił się, więc jest sprawdzony tylko pośrednio (kierunek i punkt zatrzymania). Przy okazji tempo dochodzenia w automacie (O15): SYS Fan 1 i 2 ok. 5 jednostek/s, CPU Fan 1 ok. 3,3 jednostki/s | właściciel: trzy obciążenia po 60 s (zrobione) |
| 3 | Widżet wykresu `app::fan_chart` (M2.4): miniatura i duży wykres, strefy, punkt pracy, przejścia, tryb podglądu bez uchwytów. Geometria, rozmieszczanie etykiet i opis dla czytnika jako czyste funkcje z testami. | testy; zrzut debug wykresu zgodny z makietą w obu stylach; koszt CPU zmierzony. **Zrobione (2026-11-11).** `FanChart` w dwóch rozmiarach (karta 150 px, w wąskim oknie 136 px; duży o proporcjach makiety, 280–390 px), z numerowanymi uchwytami albo kropkami, bez przeciągania. Rysuje siatkę i osie z jednostkami, krzywą z odcinków (poza skrajnymi punktami przerywaną), obszar pod nią, strefę poniżej minimum (kreskowanie, przerywana linia, w dużym wykresie napis), próg awaryjny (linia, pas, w dużym wykresie napis i kółko przy 100 % dla CPU i pompy), punkt pracy z prowadnicą i trójkątem na osi. Gdy wypełnienie ze sterownika różni się od celu z modelu o co najmniej 3 px, widać pierścień celu połączony przerywaną linią, a w dużym wykresie etykietę „Teraz … · …” z „cel …”, jak w stanie „Dochodzenie do celu”. Kolor krzywej z akcentu systemu, punkt pracy zielony. Przejście punktu 250 ms przez `AdwTimedAnimation` (bez animacji, gdy system je wyłączył). Dla czytnika ekranu wykres to obraz z nazwą „Krzywa: …” i opisem z pełnych zdań (źródło, punkty, a powtórzone jako „Punkty 4–7”, teraz, dochodzenie do celu, minimum, próg), zmienianym tylko przy zmianie tekstu. `fan_chart::geometry` to czyste funkcje: mapowanie wartości, siatka i podpisy, kształt krzywej, uchwyty, strefy, punkt pracy, miejsce etykiety. Testy: mapowanie na współrzędne, punkty powtórzone, krzywa płaska i jednopunktowa, 5 punktów, strefy, cel tylko z dala od punktu, etykieta omija przeszkody, opis dla czytnika. Na prośbę właściciela testy warstwy sterownika: krzywa z 5 plików ma 5 punktów, a ciąg 1, 2, 4 nie jest krzywą; ani sterownik, ani model krzywej, ani wykres nie zakładają 7 punktów. Tryb demonstracyjny tylko w buildzie debug: `BASEBOARD_DEBUG_FAN_CHARTS=1` (albo `cards`, `large`) zamiast głównego okna pokazuje karty kanałów z krzywymi z drzewa testowego (`BASEBOARD_SYSFS_ROOT`) albo z `/sys`, kartę SYS_FAN1 dochodzącą z 76 do 98 i dwa duże wykresy; `BASEBOARD_DEBUG_SCREENSHOT_SCALE` i `BASEBOARD_DEBUG_HIDE_AFTER` do zrzutów i pomiarów. Na MS-7E61 z modułem v1: zrzuty kart CPU i SYS_FAN1, stanu „Dochodzenie do celu” i dużego wykresu obok renderów makiet w obu stylach. Wykres zgodny z makietą co do rozmiarów, kolorów i położenia tekstu. Różnice: tekst rysuje Pango, nie przeglądarka; na prawdziwej krzywej CPU uchwyt 1 (0 °C, 48 %) zasłania część podpisu „50 %” (w makiecie punkt 1 leży przy 20 %, sprawa do edytora w M2b). Koszt CPU: 0,70 % przy widocznym oknie, 0,00 % przy ukrytym (M2.4, realizacja); klatki tylko przy zmianie odczytu (`GDK_DEBUG=frames`); `strace` bez otwarć do zapisu w `/sys`; opis dla czytnika sprawdzony przez AT-SPI | – |
| 4 | Strona Wentylatory w podglądzie, w wariancie z P1: karty, stany `read-only`, `manual`, `manual+curve-view` i `curves` bez pomocnika, złącze puste, wentylator stoi, „Tylko odczyt” dla JAF_1, podstrona krzywej w podglądzie, wpis w pasku bocznym. Odczyt trybu przy każdej próbce tylko przy widocznej stronie, krzywych przy wejściu i co 10 s. | zrzuty prawdziwej aplikacji na module v1 (jasny i ciemny styl, EN i PL, szerokie i wąskie okno) obok renderów makiet, z listą różnic; natywnie i we Flatpaku. **Zrobione (2026-11-11).** Model strony w `core::fans::page` (bez tekstów): wiersz u góry („Sterowanie wentylatorami niedostępne” przy `read-only`, inaczej „Ta wersja Baseboard pokazuje ustawienia wentylatorów bez możliwości zmiany.”), źródło wspólne dla większości krzywych z progiem, karty w kolejności złączy (niskie przy `read-only`; inaczej przełącznik dwu- albo trzypozycyjny, nieaktywny, albo ramka „Złącza nie sprawdzono na tej płycie.” przy `verified = false`; wykres, wartość trybu ręcznego, zdanie przy `manual` albo błąd odczytu krzywej), „Tylko odczyt” dla złączy bez sterowania, dochodzenie do celu z modelu krzywej, złącze puste (brak obrotów od startu przy wypełnieniu ≠ 0), karta ze źródłem innym niż wspólne. Odczyt: czytnik czujników przeszedł z Przeglądu do okna i podaje obie strony; tryb przy każdej próbce, krzywe i źródła przy wejściu na stronę i co 10 s, tylko przy widocznej stronie Wentylatory albo jej podstronie, od nowa po przeładowaniu sterownika. Strona `app::fans` (karty, podstrona krzywej z wykresem, legendą tylko wtedy, gdy etykieta punktu pracy się nie mieści, listą punktów, źródłem i „Jak działa krzywa”), wpis „Wentylatory” w pasku bocznym między Przeglądem a Systemem, cztery nowe ikony symboliczne (wentylator, pompa, termometr, krzywa). Bez profilu i bez układu płyty strona statusu z tymi samymi zdaniami co strona System i przyciskiem „Szczegóły” (makiet tych stanów nie było). Klawiatura: Tab zatrzymuje się na karcie (grupa z nazwą i opisem dla czytnika), potem na jej przycisku. Testy: model strony na modelu sterownika (trzy poziomy, `manual`, tryb ręczny, dochodzenie do celu, złącze puste i wentylator stojący, inne źródło, błędy odczytu, złącze wyłączone w preferencjach), próbnik (tryb przy każdej próbce, krzywe co 10 s i przy wejściu, bez strony nic), nazwy źródeł. Na MS-7E61 z modułem v1, natywnie: zrzuty strony, wąskiego okna, podstrony i stanu bez sterowania (drzewo Fedory) obok makiet; stany na kopiach drzew testowych (`manual`, tryb ręczny, stojący wentylator, dochodzenie do celu, bez profilu, bez układu); przeładowanie przy otwartej stronie i podstronie na kopiach drzew (Fedora → v1, układ znika i wraca, podstrona wraca do strony, gdy krzywej już nie ma); Flatpak (`flatpak build` z uprawnieniami manifestu, build debug, bez instalacji) wygląda tak samo; opis dla czytnika sprawdzony przez AT-SPI; `strace`: żadnego otwarcia do zapisu w `/sys`, na Przeglądzie zero odczytów trybów i krzywych, na stronie Wentylatory tryby co próbkę i 252 odczyty punktów krzywych w 25 s (trzy razy). CPU (D29, build release): strona widoczna 0,40–0,87 % po D57 (przed nią 1,9–2,5 %), podstrona krzywej 0,07 % przy stałej temperaturze, okno ukryte 0,00 %; Przegląd 0,08 %. **Poprawki wyglądu po przeglądzie zrzutów (2026-11-11), krok przyjęty po nich:** obroty w nagłówku karty mają szerokość swojego tekstu (cyfry tabelaryczne, więc zmienia się tylko z liczbą cyfr), a nie stałe 7 znaków, więc podtytuł „PUMP_SYS1 · złącze nietestowane” mieści się w jednym wierszu przy szerokości z makiety, a w wąskim oknie obroty i wypełnienie stoją obok siebie na jednej linii bazowej z odstępem 12 px; na podstronie krzywej nagłówki obu kolumn (własne, jak w `AdwPreferencesGroup`, z relacjami nazwy i opisu dla czytnika) leżą w jednym wierszu siatki, więc „Punkty” i „Źródło temperatury” są na jednej linii, a listy zaczynają się na jednej wysokości; ikona karty jest samym kwadratem tła (`GtkImage` rysuje ikonę na środku, a poziomy `GtkBox` przy lewej krawędzi), szarość pustego złącza przez `-gtk-icon-filter`; rysunki ikon wentylatora, termometru i krzywej wyśrodkowane w polu 16×16 (wentylator był przesunięty o −0,49 i −0,62 jednostki, termometr wystawał 0,09 poza dół pola). Zmierzone na zrzutach przy skali 100 % i 125 %, w szerokim i wąskim oknie, dla karty aktywnej i szarej: środek ikony w środku tła z dokładnością do 0,5 px (przed poprawką −10 do −13 px w poziomie i −1 px w pionie). Później na prośbę właściciela pod „Źródło temperatury” opis „Według niej kontroler liczy wypełnienie.” (po angielsku „The controller computes the duty from it.”), bo wiersz pod nagłówkiem był pusty: przy 1200 px w dwóch kolumnach w jednym wierszu po polsku i po angielsku, obie listy nadal zaczynają się na tej samej wysokości | wygląd z modułem v1 i z modułem Fedory (przeładowanie przy otwartej stronie, `sudo`), Orca na stronie i podstronie |
| 5 | Przegląd: tryb w podtytule wentylatora przy `mode` (§5.5.3). | testy modelu; zrzut. **Zrobione (2026-11-11).** Model Przeglądu (`core::overview`): podtytuł złącza ma tryb tylko przy kanale z `mode`, z trybów odczytanych w tej próbce. Próbnik czyta z kanałów tyle, ile pokazuje widoczna strona: System nic, Przegląd same tryby kanałów z `mode`, Wentylatory kanały z krzywymi; na stronie Wentylatory Przegląd bierze tryby z odczytu kanałów, więc każdy `pwmN_enable` jest czytany raz na próbkę. Pierwsza próbka po starcie i po wznowieniu czyta już to, czego potrzebuje strona; krzywe po wznowieniu dopiero 10 s po poprzednim odczycie (wcześniej przy każdym wznowieniu). Tekst (po D59): tylko tryb ręczny, „SYS_FAN1 · wypełnienie 38 % · tryb ręczny”, przed „złącze nietestowane” (po angielsku „manual mode”), bez łamania w środku nazwy; automat BIOS bez dopisku. Testy: model na trzech drzewach (tryb tylko przy `mode`: nie przy sterowniku Fedory, nie dla JAF_1, nie dla nieodczytanych kanałów i kluczy spoza profilu), próbnik (tryby od pierwszej próbki, zmiana trybu od razu, tryby ze strony Wentylatory, przy stronie System żadnych, sterownik Fedory bez trybów), teksty podtytułu we wszystkich połączeniach. Na MS-7E61 z modułem v1, natywnie: zrzuty Przeglądu (jasny i ciemny styl, PL i EN, dwie kolumny, jedna kolumna, wąskie okno), kopia drzewa v1 z SYS_FAN1 w trybie ręcznym i drzewo Fedory obok makiety (makieta pokazuje sterownik Fedory, ten stan bez zmian); `strace`: żadnego otwarcia do zapisu w `/sys`, na Przeglądzie 6 trybów na próbkę (bez `pwm4_enable`) i zero odczytów krzywych, na stronie Wentylatory każdy tryb raz na próbkę, krzywe przy wejściu i co 10 s. CPU (D29, build release, Przegląd): 1,02 % i 0,49 % w dwóch pomiarach po 60 s, w obu z kilkusekundowymi seriami ok. 40 klatek/s od pracy na pulpicie; w spokojnych odcinkach wątek GTK ok. 0,1 % i jedna klatka na próbkę; czas jądra wątku odczytu 0,93–0,95 %. Przy domyślnej szerokości okna (1200 px, dwie kolumny) i z zakresem min–maks podtytuły z „automat BIOS” łamały się na dwa wiersze, stąd D59 po przeglądzie właściciela: w zwykłym stanie każdy podtytuł mieści się w jednym wierszu (zrzuty Przeglądu na module v1, na kopii drzewa v1 z SYS_FAN1 w trybie ręcznym i na drzewie Fedory zrobione ponownie), na dwa łamie się tylko wiersz wentylatora w trybie ręcznym z zakresem | wygląd Przeglądu z modułem Fedory i v1, natywnie i we Flatpaku (w tym kroku Flatpak nie był budowany); pomiar CPU Przeglądu bez pracy na pulpicie |
| 6 | Domknięcie M2a: tłumaczenia, metainfo (opis wydania, zrzuty w `website/screenshots/0.2.0/`), README i strona, kryteria M2a (M2.8), wydanie 0.2.0 według §10.7. | kryteria M2a | push, tag, wydanie, publikacja w repozytorium Flatpak |

**M2b – pomocnik jako osobny pakiet systemowy, potem tryb ręczny, potem edytor krzywych**

*Pomocnik (bez zapisu do sprzętu):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 7 | Crate `helper` (binarka `baseboard-helper`): aktywacja D-Bus, jednostka systemd z utwardzeniem (§3.3), polityka magistrali, akcje polkit tłumaczone przez `msgfmt`, właściwości z §3.5, `GetChannels`, `GetState`; profile tylko z katalogów systemowych; opcja meson do budowania i instalacji samego pomocnika. | testy na prywatnej magistrali z atrapą polkit; `systemd-analyze security` ≤ 5,0 | instalacja pomocnika (`meson install` z `sudo`), odczyt właściwości przez `busctl` |
| 8 | Klient w aplikacji: wykrycie pomocnika (`ServiceUnknown`, `AccessDenied` od pośrednika Flatpaka, brak odpowiedzi), `ApiVersion`, `ProfileDigest`; wiersz „Pomocnik systemowy” na stronie System i wersja w tekście do schowka; `--system-talk-name` w manifeście; stany „brak pomocnika”, „Flatpak bez uprawnienia”, „starszy pomocnik”, „inny profil”. | testy z atrapą pomocnika na prywatnej magistrali; zrzuty stanów | Flatpak z gałęzi `devel` w osobnym katalogu Flatpak: wykrycie pomocnika i jego braku |
| 9 | Pakiet: `packaging/rpm/baseboard.spec` z podpakietami `baseboard-helper` (pomocnik, jednostka, pliki D-Bus i polkit, profile) i `baseboard` (aplikacja natywna), zależności Rust przez `cargo vendor`, skryptlety z P6; budowanie `rpmbuild`, potem COPR. | `rpmlint`; instalacja, aktualizacja i odinstalowanie w maszynie wirtualnej bez śladów | instalacja RPM (`sudo`), projekt w COPR (konto właściciela) |

*Tryb ręczny (wydanie 0.3.0):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 10 | Zapis trybu i wypełnienia w `SysfsDriver` za cechą `write`, którą włącza tylko crate `helper` (B1); `SetManual`, `SetAuto`, `SetAllAuto` bez krzywych, `AcknowledgeEmergency`; dziennik B2, `--restore-auto` w `ExecStopPost` (B3 a, b), B5–B10 (z D16), B15. | testy z `ModelDriver` i na sztucznym sysfs, testy zabijania (`systemd-run`: SIGTERM, SIGKILL, SIGABRT), testy losowe argumentów D-Bus | – (zapis dopiero w kroku 13) |
| 11 | Pętla nadzoru: B4 (z P4, blokada w `/run/baseboard`), B11, B13, B12, B3 d (`PrepareForShutdown` z blokadą opóźniającą logind), B3 f, B3 g (P5), sygnały. | testy wstrzykiwania błędów, atrapa logind na prywatnej magistrali, zawieszenie pętli i watchdog | – |
| 12 | Interfejs trybu ręcznego: przełącznik (2 albo 3 pozycje), suwak z minimum, baner i „Przywróć automat BIOS”, wszystkie stany z `makiety/wentylatory-stany.html`. | zrzuty zgodne z makietą stanów; testy modelu widoku | – |
| 13 | Testy na sprzęcie, w tej kolejności: SYS_FAN1; SYS_FAN2–4 (po każdym wpis w `tested_writes`, D16); CPU_FAN1 z testem O10 (P9); drogi zakończenia B3; S3; próg obniżony do 50 °C; B11 z profilem testowym; zapis innym narzędziem; aktualizacja i odinstalowanie pakietu; przeładowanie modułu; polkit z Flatpaka (O19). | kryteria trybu ręcznego (M2.8) | wszystko w tym wierszu, każde za zgodą |
| 14 | Domknięcie: README (instalacja pomocnika), strona, zrzuty trybu ręcznego, wydanie 0.3.0 (aplikacja i pakiet `baseboard-helper`). | kryteria trybu ręcznego | push, tag, wydanie, COPR, publikacja Flatpaka |

*Edytor krzywych (wydanie 0.4.0, gdy zapis krzywych jest w `hwmon-next`, P3):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 15 | Planer B16 i walidacja B7, B14 w `core`. | testy właściwościowe (M2.8) | – |
| 16 | Pomocnik: `GetEcCurve`, `SetEcCurve`, `RestoreOriginalCurve`, migawka B18, B17, `SetAllAuto` i `--restore-all` z krzywymi, B12 dla krzywych; `ApiVersion` 2. | testy z `ModelDriver`: przycinanie, `-EIO` w połowie planu, `-ETIMEDOUT`, nieudany powrót | – |
| 17 | Edytor (podstrona): wykres z uchwytami, przeciąganie, klawiatura, lista punktów, tryb „Krzywa”, „Przywróć krzywą BIOS”, stany (niezapisane zmiany, błąd zapisu, krzywa łamiąca zasady, podgląd), okno „Odrzucić zmiany krzywej?”. Dwie uwagi do dużego wykresu ze zrzutów kroku 3 M2a (2026-11-11), rozwiązanie do zaprojektowania przy edytorze: uchwyt punktu przy 0 °C zasłania podpis osi wypełnienia (na prawdziwej krzywej CPU uchwyt 1 przy 48 % zakrywa „50 %”); w wąskim oknie uchwyty przy 85–95 °C i kółko progu awaryjnego przy 100 % nachodzą na siebie. | zrzuty zgodne z makietą; testy logiki edycji (ograniczenie do sąsiadów i do minimum, klawiatura, opis dla czytnika) | Orca na edytorze |
| 18 | Testy na sprzęcie z modułem v1: `pwm16`, potem SYS 2–4, CPU według D16; obroty pod obciążeniem idą za krzywą; S3 z krzywą; restart (po starcie krzywa BIOS-u); „Przywróć krzywą BIOS”; O17 opcjonalnie. | kryteria krzywych (M2.8) | wszystko w tym wierszu, każde za zgodą |
| 19 | Domknięcie: zrzuty edytora, README i strona, wydanie 0.4.0; potem F3 według P7. | kryteria krzywych | push, tag, wydanie, COPR, publikacja |

**M2c – wybór źródła temperatury** (po łatce 5 i dowodzie sprzętowym z M2.2)

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 20 | `source-write` według łatki 5 w kształcie wysłanym na listę (D17), `SetEcSource`, `ApiVersion` 3, wybór w edytorze (temperatury EC z profilu), B14 dla nowego źródła. | wybór źródła na module z łatką 5; dowód sprzętowy | test zmiany źródła, za zgodą |

**Co widzi użytkownik** na każdym poziomie sterownika, w każdym wydaniu, z pomocnikiem i bez, natywnie i we Flatpaku: tabela w §5.5.3.

#### M2.4 Wygląd strony Wentylatory (D45, makiety zaakceptowane 2026-11-11)

**Makiety** w `makiety/` (opis w `makiety/OPIS.md`), rendery poza repozytorium (`makiety/renderuj.sh` z `OUT=…`, `NARROW=1`, `ACCENT=…`):
- `wentylatory.html`: bez parametru pełne sterowanie (`curves` z pomocnikiem), `?stan=podglad` (brak pomocnika), `?stan=tylko-odczyt` (jądro Fedory);
- `wentylatory-krzywa.html`: edytor z niezapisanymi zmianami, `?stan=blad`, `?stan=bios-cpu` (domyślna krzywa CPU z §6.3, łamie B5), `?stan=podglad`;
- `wentylatory-stany.html`: 20 stanów karty i strony, każdy z tekstem i uwagą, kiedy występuje.

Wszystkie w jasnym i ciemnym stylu, w szerokim (1200 px) i wąskim oknie (400 px, pasek boczny zwinięty jak poniżej 600 sp). Pasek boczny ma tylko istniejące strony: Przegląd, Wentylatory, System. Dane to odczyty z MS-7E61 z 2026-11-11 (moduł v1, krzywe z ustawień BIOS-u właściciela, temp1 46 °C); własne krzywe, tryb ręczny i stany błędów są przykładowe.

**Układ (D45).** Rozważone trzy warianty: A, karty złączy z miniaturą krzywej; B, lista jak na Przeglądzie z rozwijanym wykresem; C, lista złączy i jeden wentylator z dużym wykresem i edycją na miejscu. Wybrany A: pokazuje naraz wszystkie wentylatory z krzywą i punktem pracy, zgadza się z D6 i daje edytorowi pełną szerokość okna. B chował krzywe w miniaturach, a C pokazywał jeden wentylator, w wąskim oknie dokładał poziom nawigacji (długie nazwy złączy łamały się w liście) i wymagał zmiany D6.

**Strona** (od góry): baner „Zmiany działają także po zamknięciu aplikacji, do restartu komputera” z „Przywróć automat BIOS”, gdy Baseboard coś zmienił (jeden komunikat zamiast banera i osobnego zdania pod nim); grupa „Źródło temperatury” z jednym wierszem (nazwa, `nct6687 · temp1 · próg awaryjny 95 °C`, zakres min–maks i bieżąca wartość, jak wiersze Przeglądu); grupa „Złącza” z kartami; grupa „Tylko odczyt” z wierszem JAF_1. Gdy złącza będą miały różne źródła (M2c), karta podaje źródło tylko wtedy, gdy różni się od wspólnego. W stanach bez sterowania zamiast banera jest jeden wiersz u góry („Sterowanie wentylatorami niedostępne”, „Podgląd bez sterowania”).

**Karta złącza:** ikona złącza (wentylator, kropla dla pompy), nazwa z profilu i podtytuł w jednym wierszu („CPU_FAN1 · minimum 30 %”); obroty dużą cyfrą, pod nimi „wypełnienie 49 %”; przełącznik trybu; miniatura krzywej z punktem pracy, strefą poniżej minimum i progiem jako linią i pasem, bez napisów (minimum mówi podtytuł, próg wiersz źródła); stopka z przyciskiem „Edytuj krzywą”. Etykiety „Krzywa z BIOS-u” nie ma przy żadnym złączu: przełącznik pokazuje tryb („Automat BIOS” to krzywa z BIOS-u, D11), a złącze bez przełącznika ma ramkę „Tylko podgląd”; edytor nazywa tryb w podtytule („CPU_FAN1 · automat BIOS”). Karta nie jest klikalna.
- Tryb ręczny: zamiast wykresu duża wartość wypełnienia („45 %”) i suwak w polu o wysokości wykresu, więc siatka kart zostaje równa; obroty w nagłówku; dla wentylatorów systemowych w stopce „0 % zatrzymuje wentylator”, dla procesora i pompy zakres poniżej minimum zakreskowany na suwaku.
- Złącze bez sprawdzonego zapisu (na MS-7E61 pompa, D52): zamiast przełącznika ramka z kłódką „Tylko podgląd: złącza nie sprawdzono na tej płycie.”; wtedy podtytuł nie powtarza „złącze nietestowane”. Pod spodem w tej samej ramce „Brak obrotów od startu aplikacji: złącze może być puste.”, gdy złącze nie miało obrotów. Łamane zdanie ramki ma w ostatnim wierszu co najmniej trzy słowa, jak opis źródła (D60).
- Układ kart zależy od szerokości, nigdy od chwilowych wartości (D61): nazwa i podtytuł nigdy się nie łamią; obroty i wypełnienie stoją obok nazwy, dopóki mieści się najszersza możliwa treść (obroty czterocyfrowe, „wypełnienie 100 % → 100 %”, najdłuższa nazwa i podtytuł na stronie, w języku i czcionce aplikacji); inaczej schodzą pod nazwę, obok siebie; gdy i to nie mieści się w połowie szerokości, karty stoją w jednej kolumnie. Wszystkie karty strony mają ten sam układ, w rzędzie wykresy i przyciski stoją równo. Karta z wykresem ma stałą wysokość: notka w stopce ma zawsze zarezerwowane miejsce, a „Szczegóły krzywej” stoi w tym samym miejscu. W wąskim oknie wiersz źródła ma wartość nad zakresem.
- Bez pomocnika (podgląd): karty mają ten sam kształt; zamiast przełącznika ramka z trybem jako tekstem („Tryb: Automat BIOS”) i kłódką, w tym samym stylu co ramka pompy („Złącza nie sprawdzono na tej płycie.”, z ikoną informacji); w rzędzie dwóch kart obie ramki mają tę samą wysokość, więc wykresy i stopki stoją równo; przycisk brzmi „Szczegóły krzywej”; nad kartami raz legenda wykresów (strefa poniżej minimum, próg awaryjny, punkt pracy, przedłużenie krzywej), łamana całymi pozycjami (D60, D61).
- Sterownik bez sterowania: karty bez wykresu i przełącznika, podtytuł bez minimum (niczym nie da się sterować); pasek wypełnienia tylko przy kanałach, które je podają; to, że wentylatory systemowe go nie mają, mówi raz wiersz u góry.

**Wykres krzywej** (ten sam w miniaturze i w edytorze):
- Odcinki proste z zaokrąglonymi złączami, bo EC liczy wypełnienie liniowo między punktami (M2.1, pkt 11). Gładka krzywa (spline) pokazywałaby wypełnienia, których wentylator nigdy nie dostaje. Poniżej pierwszego i powyżej ostatniego punktu EC trzyma ich wypełnienie: linia przerywana. Pod krzywą gradient akcentu.
- Osie: temperatura źródła 0–100 °C (zakres B7), siatka co 10 °C, podpisy co 20 °C z jednostką („40 °C”); wypełnienie 0–100 % co 25 % („50 %”). W edytorze tytuły osi: „Wypełnienie” i „Temperatura: Procesor (czujnik płyty)”. W miniaturze mniej linii i podpisów.
- Strefa poniżej minimum B5 (procesor, pompa): kreskowanie i przerywana linia granicy; w edytorze także napis „Poniżej minimum 30 %” pod linią krzywej, więc jej nie zasłania. Próg awaryjny źródła: pas w kolorze błędu i przerywana pionowa linia; w edytorze napis poziomo nad wykresem, przy górze linii („Próg awaryjny 95 °C”, dla procesora i pompy „Próg awaryjny 95 °C: od niego 100 %” i kółko w punkcie progu przy 100 %, B14).
- Punkt pracy na żywo: kropka w zieleni z ikony aplikacji z poświatą, przerywane prowadnice do obu osi i znaczniki na osiach. W edytorze etykieta „Teraz 46,0 °C · 28 %” tam, gdzie nie zasłania uchwytów ani napisów; gdy krzywa jest zmieniona, a niezastosowana, puste kółko w akcencie i druga linia „po zastosowaniu 24 %” (w automacie przy dochodzeniu do celu: „cel 38 %”). Gdy etykieta się nie mieści (wąskie okno), te same wartości stoją w legendzie.
- Uchwyty (edytor): kółka z numerem punktu, takim jak na liście; punkty o tych samych wartościach to jeden uchwyt w kształcie pigułki („6–7”). Wybrany uchwyt jest wypełniony, a przy fokusie klawiatury ma obwódkę fokusu. Punkt, który łamie zasadę, ma kolor ostrzeżenia i znak ostrzeżenia przy uchwycie; powód („Ustaw co najmniej 30 %”) stoi raz, w jego wierszu listy.
- Krzywa, którą EC wykonuje teraz, gdy w edytorze jest inna: przerywana linia w kolorze tekstu.
- Legenda pod dużym wykresem tylko dla oznaczeń, których wykres sam nie podpisuje: „Nowa krzywa” i „W kontrolerze teraz”, a w wąskim oknie punkt pracy. Bez krzywej w kontrolerze i z etykietą punktu pracy na wykresie legendy nie ma. Nic nie opiera się na samym kolorze: strefy mają wzór, linie różny wzór kreskowania, punkt łamiący zasadę znak.
- Kolory (D55): krzywa, obszar i uchwyty w akcencie systemu (`accent_bg_color`, idzie za wyborem użytkownika; makiety z `?accent=…` pokazują inne akcenty libadwaita), numery i tekst akcentu w jego wersji tekstowej; punkt pracy w zieleni z ikony (#26a269 w jasnym stylu, #33d17a w ciemnym); próg w `error_color`, ostrzeżenia w `warning_color`, kreskowanie w kolorze tekstu. Przy zielonym akcencie punkt pracy odróżnia kształt (kropka z poświatą) i etykieta.

**Edytor (podstrona):** tytuł nazwą złącza, podtytuł „SYS_FAN1 · niezapisane zmiany”, „· krzywa z BIOS-u” albo „· podgląd”; duży wykres z legendą; lista 7 punktów, w której każdą wartość zmienia się przyciskami albo wpisuje z klawiatury (`GtkSpinButton`); grupa „Źródło temperatury” (nazwa i kanał, kłódka, bez wartości, którą pokazuje wykres); grupa „Jak działa krzywa”; dolny pasek „Przywróć krzywą BIOS”, „Cofnij zmiany”, „Zastosuj” (w wąskim oknie przywracanie z podpisem w osobnej linii). Ramka u góry tylko tam, gdzie mówi coś, czego nie mówi podtytuł ani legenda: błąd zapisu („Kontroler płyty nie przyjął ustawień”), krzywa łamiąca zasady („Ta krzywa nie spełnia zasad Baseboard”, „Zastosuj” nieaktywne z wyjaśnieniem na dolnym pasku), podgląd („Edycja krzywych i wybór źródła wymagają nowszego sterownika jądra.”).

**Stany** (`wentylatory-stany.html`, każdy z wyglądem i tekstem): sterownik bez sterowania; wersja 0.2.0 (sam podgląd); brak pomocnika; Flatpak bez uprawnienia do pomocnika (z poleceniem i przyciskiem kopiowania); pomocnik w starszej wersji; inna wersja profilu w pomocniku; sterownik bez podglądu krzywych (`manual`); sterownik bez zapisu krzywych (`manual+curve-view`); tryb ręczny z minimum; pompa ze sterowaniem (stałe ostrzeżenie B6); wentylator stoi (0 %, zamierzone, bez ostrzeżenia); wentylator stanął (B11, czerwona obwódka karty, ikona i tekst); złącze puste; krzywa zmieniona i niezapisana (okno „Odrzucić zmiany krzywej?” przy wyjściu z edytora); błąd zapisu w trybie ręcznym; dochodzenie do celu; blokada awaryjna (D48: pełne wypełnienie, „Zdejmij blokadę” aktywne po spadku o 10 °C, wiersz wraca po ponownym uruchomieniu aplikacji); inny program zmienił ustawienia (B10); pomocnik przestał działać; krótkie komunikaty (`AdwToast`: anulowana autoryzacja, zastosowana krzywa, powrót do automatu BIOS). Błędy bez kodów; szczegóły w dzienniku (B15). Na jednym ekranie żaden tekst nie powtarza informacji z innego miejsca tego ekranu.

**Ruch.** `AdwTimedAnimation`, która przy wyłączonych animacjach (ustawienie „Ogranicz animacje”, `gtk-enable-animations = false`) kończy się od razu:
- punkt pracy: przejście 250 ms (ease-out-cubic), tylko gdy przesuwa się o więcej niż 2 px; mniejsze zmiany co sekundę bez animacji; na kartach punkt przeskakuje, płynnie przechodzi tylko na dużym wykresie (D57: koszt CPU);
- po zastosowaniu krzywej linia przerywana przechodzi w nową w 300 ms; po „Cofnij zmiany” odwrotnie;
- krok uchwytu z klawiatury: 120 ms; przeciąganie myszą bez animacji (uchwyt idzie za kursorem);
- liczby zmieniają się od razu, bez animacji; żadnych animacji ciągłych (np. obracającej się ikony), bo kosztowałyby klatkę co 16 ms.

**Dostępność:**
- Klawiatura: przełącznik, suwak (strzałki, PageUp/PageDown), przyciski i pola liczbowe jak w libadwaita; wartości punktów można wpisać w polach listy (liczba z jednostką albo bez, „50” i „50 %”), a pole odrzuca wartość spoza dozwolonego zakresu tak jak przeciąganie. Wykres edytora przyjmuje fokus: Tab i Shift+Tab wybierają kolejny punkt (po ostatnim fokus wychodzi z wykresu), strzałki przesuwają punkt o 1 °C albo 1 %, z Shift o 5, Home i End wybierają pierwszy i ostatni punkt. Ruch jest ograniczony do sąsiednich punktów i do minimum B5, tak jak przy przeciąganiu. Skróty są w oknie skrótów klawiszowych i w wierszu „Klawiatura” edytora.
- Czytnik ekranu: miniatura ma rolę obrazu z opisem (punkty, źródło, temperatura i wypełnienie teraz). W edytorze pełną drogą edycji jest lista punktów (`GtkSpinButton` z nazwami „Punkt 3, temperatura”, „Punkt 3, wypełnienie”); zmiana punktu z klawiatury na wykresie jest ogłaszana (`gtk_accessible_announce`, GTK 4.14). Karty: nazwa złącza, obroty, wypełnienie i tryb jako tekst; źródło i próg w wierszu źródła.
- Kontrast (WCAG, policzony z wartości libadwaita 1.10 i kolorów makiet): podpisy osi 5,2:1 w jasnym i 7,3:1 w ciemnym stylu; linia krzywej 3,8:1 i 3,3:1; punkt pracy 3,3:1 i 6,3:1; próg 6,3:1 i 5,8:1; numer wybranego uchwytu 5,8:1 i 9,4:1; krzywa w kontrolerze 3,9:1 i 5,9:1; teksty błędów i ostrzeżeń w ramkach 4,9–7,9:1. W trakcie projektu poprawione: biały numer na akcencie (3,8:1) i przerywana linia przy kryciu 45 % (2,5:1 w jasnym stylu). Przygaszony tekst libadwaita (krycie 55 %) ma w jasnym stylu 3,2:1, więc wartości i podtytuły złączy na tej stronie mają krycie 72 % (5,2:1), a przygaszone są tylko objaśnienia powtórzone gdzie indziej.

**Spójność z Przeglądem i Systemem:** te same nazwy złączy (rola i numer z profilu) i podtytuły („CPU_FAN1 · wypełnienie 49 % · złącze nietestowane”), ta sama kolejność złączy, podtytuł strony „Odświeżanie co 1 s”, nagłówki i opisy grup jak na Przeglądzie („Złącza”, „W kolejności złączy na płycie”), wiersze `AdwActionRow` dla źródła temperatury (nazwa jak na Przeglądzie, zakres min–maks i wartość), JAF_1 i stanów strony, liczby i jednostki z `core::units`, odstępy grup 24 px i szerokość treści jak na Przeglądzie (`AdwClamp`, 940 px). Tekst o sterowaniu na stronie System i na stronie Wentylatory jest ten sam („Sterowanie wentylatorami niedostępne”).

**Realizacja w GTK4:**
- Własny widżet `app::fan_chart` (podklasa `GtkWidget`) w dwóch trybach: miniatura i edytor. Rysuje w `snapshot()`: krzywa i linie przez `GskPathBuilder` i `append_stroke` (zaokrąglone złącza, przerywanie przez `GskStroke::set_dash`), obszar pod krzywą przez `push_fill` z gradientem, kreskowanie strefy jedną ścieżką ukośnych linii (1 px co 6 px) w przycięciu, tekst przez `PangoLayout` z kontekstu widżetu (Adwaita Sans, cyfry tabelaryczne `tnum`). Antyaliasing i skalę HiDPI zapewnia GSK, więc linie są gładkie przy każdej skali.
- Siatka, osie, strefy, podpisy i krzywa tworzą jeden węzeł renderowania, a punkty krzywej drugi; oba są w pamięci podręcznej do zmiany rozmiaru, stylu (jasny, ciemny, wysoki kontrast, akcent), czcionki albo krzywej. Prowadnice punktu pracy leżą między nimi, a sam punkt i etykieta nad punktami krzywej, jak w makiecie. Nowy odczyt rysuje tylko punkt pracy i etykietę; odczyt bez zmian nie rysuje niczego.
- Kolory: tekst z `widget.color()`, akcent z `AdwStyleManager` (`accent-color-rgba`, wersja tekstowa przez `AdwAccentColor::to_standalone_rgba`), kolory błędu i ostrzeżenia z wartości libadwaita dla stylu jasnego i ciemnego, zieleń z ikony; przerysowanie po `notify::dark`, `notify::high-contrast` i `notify::accent-color-rgba`, a po zmianie czcionki albo wielkości tekstu (`gtk-font-name`, `gtk-xft-dpi`) także podpisów. Ograniczenie: własny CSS użytkownika, który zmienia `error_color`, nie zmieni wykresu.
- Interakcja: `GtkGestureDrag` (uchwyt łapany w promieniu 16 px, wartości całkowite °C i %, ograniczenie do sąsiadów i minimum), `GtkEventControllerMotion` (kursor i podświetlenie), `GtkEventControllerKey` (klawisze wyżej).
- Ograniczenia: elementy wewnątrz własnego widżetu nie są dla czytnika ekranu osobnymi obiektami (GTK pozwala na to od 4.10 przez własną implementację `GtkAccessible`, której w M2 nie planujemy), dlatego lista punktów jest pełną drogą edycji. Etykietę punktu pracy rozmieszcza prosta reguła kandydatów (pozycje wokół punktu, bez zasłaniania uchwytów i napisów); gdy żadna nie pasuje, wartości idą do legendy.
- Koszt CPU przy odświeżaniu co sekundę: sześć miniatur dostaje nowy punkt pracy, a GSK składa węzły z pamięci podręcznej z kilkoma nowymi kształtami. Przejście 250 ms to tyle klatek, ile da odświeżanie ekranu (przy 120 Hz ok. 30), ale tylko przy zmianie o więcej niż 2 px. Zmierzone w kroku 3 (tryb demonstracyjny, sześć kart na MS-7E61 z modułem v1, odczyt Przeglądu co 1 s): 0,70 % jednego rdzenia według D29 przy widocznym oknie (Przegląd z M1: 0,35 %), 0,00 % przy ukrytym. Na prawdziwej stronie w kroku 4 przejścia sześciu kart przy temperaturze zmieniającej się co sekundę kosztowały 1,9–2,5 %, stąd D57; po niej 0,40–0,87 % (zależnie od tego, jak często zmienia się temperatura), bez animacji 0,17 %. Odczyty: `pwmN_enable` przy każdej próbce tylko przy widocznej stronie, która pokazuje tryb: Wentylatory (tryby kanałów z odczytem krzywych) albo Przegląd (od kroku 5 M2a tylko tryby kanałów z `mode`, do podtytułów, §5.5.3); każdy odczyt idzie do EC, poza pamięcią podręczną sterownika (M1), a każdy tryb jest czytany raz na próbkę, bo Przegląd bierze tryby odczytane dla strony Wentylatory; przy stronie System żadnych. Krzywe przy wejściu na stronę Wentylatory, co 10 s i po sygnale pomocnika (ok. 84 odczytów rejestrów). Kryterium jak w D29: cała aplikacja z widoczną stroną Wentylatory poniżej 1 % jednego rdzenia, przy ukrytym oknie praktycznie zero; koszt odczytu sprzętu osobno, jako pomiar.

#### M2.5 Bezpieczeństwo: B1–B18 po kolei

| Zasada | Krok | Test automatyczny | Test na sprzęcie (właściciel, za zgodą) | Czego nie da się sprawdzić w pełni |
|---|---|---|---|---|
| B1 tylko pomocnik pisze | 1, 10 | funkcje zapisu `SysfsDriver` są za cechą `write`, którą włącza tylko crate `helper`, więc aplikacja nie ma kodu zapisu; test zależności crate'ów; `strace` w kryteriach | `strace` aplikacji natywnej i z Flatpaka przy pracy z pomocnikiem (odczyt) | – |
| B2 dziennik i kolejność zapisu | 10 | `ModelDriver`: wpis w `/run/baseboard/claimed` przed pierwszym zapisem, kolejność `pwmN_enable` → `pwmN`; bez katalogu `/run/baseboard` zapis odrzucony | odczyt `/run/baseboard/claimed` po włączeniu trybu ręcznego | – |
| B3 a SIGTERM | 10 | `systemd-run` na sztucznym sysfs: SIGTERM → `pwmN_enable = 2` | `systemctl stop` przy trybie ręcznym, odczyt sysfs | – |
| B3 b awaria | 10 | SIGKILL i SIGABRT (`panic = "abort"`) → `ExecStopPost=… --restore-auto` | `kill -KILL` i `kill -ABRT` pomocnika | OOM: tylko przez SIGKILL, tą samą drogą |
| B3 d wyłączenie i restart | 11 | atrapa logind: `PrepareForShutdown(true)` → automat przed zwolnieniem blokady opóźniającej | wyłączenie i restart przy trybie ręcznym; dowodem jest wpis z poprzedniego uruchomienia (`journalctl -b -1 -u baseboard-helper`) | stanu sysfs po wyłączeniu nie da się odczytać, a po starcie BIOS i tak ustawia automat |
| B3 e, f próg i błąd odczytu | 11 | wstrzykiwanie: przekroczenie progu, znikający plik, wartości niewiarygodne | próg obniżony w `/etc` do 50 °C i krótkie obciążenie | – |
| B3 g przeładowanie modułu (P5, nowa) | 11 | sztuczny sysfs: urządzenie znika i wraca z `pwmN_enable = 1` → automat; krzywe porównane bez nadpisywania | `modprobe -r` i `modprobe` przy trybie ręcznym (`sudo`) | czas między wyładowaniem a załadowaniem jest bez nadzoru (ryzyko szczątkowe) |
| B4 próg awaryjny | 11 | 3 kolejne odczyty, warunek z P4, histereza 10 °C, blokada przeżywa restart pomocnika (`/run/baseboard`), `AcknowledgeEmergency` odrzucone przed spadkiem | próg obniżony do 50 °C; prawdziwy próg 95 °C tylko w teście CPU (P9) | MOS 90 °C i System 80 °C na prawdziwym progu (nie przegrzewamy płyty) |
| B5 minimum | 10, 15 | proptest: dowolne żądanie daje co najmniej 76 (CPU) i 127 (pompa); profil nie zejdzie poniżej stałej granicy; każdy punkt krzywej | `busctl` `SetManual cpu1 0` zwraca 76 (po teście zapisu CPU) | – |
| B6 pompa nie staje | 10, 15 | 0 dla roli `pump` → `InvalidArgument`, także w krzywej | – (na MS-7E61 pompa bez sterowania, P8) | zachowanie prawdziwej pompy: żadna nie jest podłączona |
| B7 walidacja wejścia | 10, 15 | losowe argumenty D-Bus, krzywe z inną liczbą punktów, malejące, poza zakresem | `busctl` ze złymi argumentami (bez skutku w sprzęcie) | – |
| B8 łagodne zmiany | 10, 16 | atrapa zegara: najwyżej 1 zapis/s, spadek najwyżej 10 p.p./s, krzywa najwyżej co 2 s | szybkie przeciąganie suwaka z odczytem `pwmN` co 0,1 s | – |
| B9 znany sprzęt | 7, 10 | drzewa z inną rewizją, innym modelem, bez `name`, bez możliwości, bez `tested_writes` → `NotSupported` | profil testowy w `/etc` z inną rewizją PCB | – |
| B10 weryfikacja i cudze zmiany | 10, 11, 16 | sztuczny sysfs: zmiana trybu albo krzywej z zewnątrz → `ControlLost: external-change` bez nadpisania; niezgodny odczyt → automat | zapis innym narzędziem przy trybie ręcznym (`sudo`) | – |
| B11 zatrzymany wentylator | 11 | zero obrotów przez 5 s przy wypełnieniu ≥ minimum → automat i ostrzeżenie | odłączenie wentylatora SYS, któremu profil testowy nadał rolę `cpu` | zatrzymanie prawdziwego wentylatora procesora i pompy (nie zatrzymujemy) |
| B12 wybudzenie | 11, 16 | `ModelDriver` z odtwarzaniem stanu i bez | S3 z trybem ręcznym i z krzywą | S4 i kexec (O4, O17), opcjonalnie |
| B13 watchdog | 11 | zawieszenie pętli (punkt zaczepienia tylko w buildzie testowym) → zabicie → `--restore-auto` | `kill -STOP` pomocnika przy trybie ręcznym, odczyt po czasie watchdoga | – |
| B14 krzywe bez nadzoru | 15 | proptest: `cpu` i `pump` mają 255 najpóźniej na progu; źródło bez progu → `NotSupported`; `sys` bez wymogu (D19) | edytor nie pozwala zastosować takiej krzywej | czy krzywa spełniająca B14 wystarczy do chłodzenia w każdych warunkach (założenie projektu) |
| B15 dziennik systemd | 10, 11, 16 | przechwycony dziennik: klucz kanału i przyczyna przy każdym zdarzeniu | `journalctl -u baseboard-helper` po testach | – |
| B16 kolejność zapisu krzywej | 15 | proptest: każdy stan pośredni niemalejący i chłodzący co najmniej jak obie krzywe; `ModelDriver` sprawdza, że przycinanie nie zachodzi | odczyt po każdym kroku planu na `pwm16` (dziennik) | – |
| B17 błąd zapisu krzywej | 16 | `ModelDriver`: `-EIO` w połowie planu, `-ETIMEDOUT`, nieudany powrót → automat i `driver-error` | – | prawdziwego `-EIO` ani `-ETIMEDOUT` z EC nie umiemy wywołać na żądanie; tylko model |
| B18 krzywa pierwotna | 16 | migawka przed pierwszą zmianą; przywrócenie krzywej łamiącej B5 (domyślna CPU z §6.3) bez walidacji; restart czyści `/run` | zmiana krzywej SYS_FAN1 i „Przywróć krzywą BIOS” (porównanie odczytu); krzywa domyślna CPU tylko wtedy, gdy właściciel przywróci w BIOS-ie ustawienia domyślne | – |

**Sytuacje szczególne:**
- **92 °C po 12 s przy progu 95 °C (O10).** Przy wentylatorze procesora w trybie ręcznym na minimum temperatura może dojść do progu w kilka sekund, a B4 reaguje po 3 odczytach. Wyjście w automacie dochodzi do celu stopniowo (na SYS_FAN1 ok. 2 %/s, na CPU_FAN1 ok. 3,3 jednostki/s, czyli ze 124 do 255 w ok. 40 s), więc samo przejście na automat chłodzi wolno. Dlatego P4: przy progu od razu 100 % na kanałach sterowanych ręcznie przez Baseboard, a warunek dla procesora „osiągnie” zamiast „przekroczy”, bo procesor dławi przy Tjmax 95 °C i temp1 może stanąć dokładnie na progu. Procesor chroni się też sam (dławienie przy Tjmax). Test: P9.
- **Krzywa z BIOS-u łamiąca B5** (domyślna krzywa CPU, §6.3). Podgląd (M2a) pokazuje ją bez ostrzeżenia, bo to ustawienie firmware. Edytor pokazuje ją bez zmian, wskazuje punkt 1 („Poniżej minimum 30 %”) i blokuje „Zastosuj” (makieta `bios-cpu`). „Przywróć krzywą BIOS”, `SetAllAuto` i powrót w B17(b) zapisują ją bez walidacji (B18). Test automatyczny na tej krzywej; na sprzęcie tylko przy domyślnych ustawieniach BIOS-u.
- **Zapis sprawdzony tylko na SYS_FAN1.** Profil ma dziś `tested_writes` tylko dla `sys1`. SYS_FAN2–4 dostają wpis po teście w kroku 13, CPU_FAN1 po teście P9. Do tego czasu karta pokazuje „Tylko podgląd: zapisu na tym złączu nie sprawdzono na tej płycie”.
- **Puste złącza pompy i wentylatora dodatkowego.** Na PUMP_SYS1 nic nie jest podłączone (0 rpm przy 100 %), więc nie da się sprawdzić ani przypisania (O3), ani zapisu (O5); pompa zostaje bez sterowania (P8). B11 nie reaguje, bo złącze nie miało obrotów. Strona mówi „Brak obrotów od uruchomienia aplikacji. Złącze może być puste.”, a pustego złącza od pompy, która nie ruszyła, nie odróżnia. JAF_1: bez `pwm4_enable` w sterowniku, tylko odczyt (D32).
- **Awaria pomocnika:** B3(b) przywraca automat kanałom w trybie ręcznym; krzywe w EC działają dalej. Aplikacja dostaje zniknięcie nazwy z magistrali, pokazuje „Pomocnik systemowy przestał działać” i nie wznawia trybu ręcznego sama.
- **Zamknięcie aplikacji:** nic się nie zmienia. Pomocnik nadzoruje tryb ręczny dalej (B4, B11, B13), krzywe działają w EC; baner i opis strony o tym mówią.
- **Uśpienie:** bez zmian przed uśpieniem (D13); po wybudzeniu B12 (sterownik z `7dc9dff9608d` zwykle odtwarza tryb ręczny sam). Hibernacja: O4.
- **Wyłączenie i restart:** B3(d) z blokadą opóźniającą logind; po starcie BIOS ustawia automat i swoje krzywe (D4), `/run` jest pusty.
- **Przeładowanie modułu:** B3(g) (P5). Pomocnik widzi zniknięcie urządzenia (udev), wysyła `ControlLost` z nowym powodem `driver-reloaded`, po powrocie urządzenia przywraca automat kanałom z dziennika i porównuje krzywe. Czas bez modułu jest bez nadzoru (§4).
- **Aktualizacja pakietu pomocnika:** restart pomocnika (P6), tryb ręczny wraca do automatu przez B3(a), krzywe zostają; aplikacja pokazuje komunikat jak przy awarii, z inną treścią („Pomocnik został uruchomiony ponownie”).
- **Odinstalowanie:** P6, `baseboard-helper --restore-all` przed usunięciem plików: krzywe pierwotne i automat na kanałach zmienionych przez Baseboard.

**Procedura testu CPU_FAN1 przy progu 95 °C (D53).** Uruchamia ją właściciel, po przeczytaniu, w kroku 13: po testach SYS_FAN1–4, z pomocnikiem, który ma już B4 według D48. Skrypt zapisu odczytów i skrypt straży powstaną w kroku 13; tylko czytają sysfs i wołają pomocnika przez D-Bus.
- **Cel:** (1) zapis na CPU_FAN1 w trybie ręcznym (O5); (2) czy temp1 nadąża za Tctl i czy przy wentylatorze na minimum dochodzi do 95 °C (O10); (3) B4 na prawdziwym progu: pełne wypełnienie najpóźniej po 3 odczytach od osiągnięcia progu.
- **Warunki wstępne:** moduł z serią v1 (jak dziś) i pomocnik z kroku 11; profil testowy w `/etc/baseboard/boards/ms-7e61.toml` (kopia profilu z `tested_writes = ["manual"]` dla `cpu1`, tworzy go właściciel z `sudo`; po teście usunięty, a wynik trafia do profilu w repozytorium); obudowa zamknięta; krzywe i zabezpieczenia temperatury w BIOS-ie bez zmian; `stress-ng` zainstalowany (`sudo dnf install stress-ng`); komputer bez innej pracy; temp1 w spoczynku poniżej 50 °C.
- **Co chroni procesor:** (1) sam procesor: Ryzen 9000 przy Tjmax 95 °C (Tctl) obniża zegar i napięcie, to jego zwykła granica pracy, a wyłączenie awaryjne procesora leży wyraźnie wyżej; (2) B4 w pomocniku (D48): temp1 ≥ 95 °C w 3 kolejnych odczytach → CPU_FAN1 od razu na 100 % i blokada; (3) minimum B5: wentylator cały czas pracuje co najmniej na 30 %; (4) straż: osobny skrypt czyta co 1 s temp1, Tctl, obroty i odpowiedź pomocnika, a przy warunku przerwania kończy obciążenie i woła `SetAllAuto` (akcja `restore-auto`, bez hasła); (5) właściciel: Ctrl+C kończy obciążenie od razu, a samo obciążenie ma limit 60 s.
- **Warunki przerwania** (straż i właściciel): Tctl albo temp1 ≥ 97 °C (B4 nie zadziałała albo procesor nie trzyma Tjmax); obroty CPU_FAN1 poniżej 300 rpm przez 2 s; brak odpowiedzi pomocnika przez 3 s; 60 s obciążenia; cokolwiek nietypowego (dźwięk, zawieszony interfejs).
- **Przebieg:**
  1. Stan przed testem: `cat` krzywej i trybu `pwm1`, temp1, Tctl.
  2. Start zapisu odczytów do pliku CSV poza repozytorium (co 1 s: czas, temp1, Tctl, `pwm1`, `pwm1_enable`, obroty, właściwość `Emergency`) i start straży.
  3. Tryb ręczny na minimum: `busctl call --system me.bojanowski.Baseboard.Helper /me/bojanowski/Baseboard/Helper me.bojanowski.Baseboard.Fans1 SetManual sy cpu1 76` (polkit pyta o hasło). Oczekiwane: odpowiedź 76, `pwm1_enable` = 1, `pwm1` = 76, obroty spadają i się ustalają.
  4. 60 s bez obciążenia: zapis działa (O5), obroty stałe, temp1 stabilne.
  5. Obciążenie wszystkich wątków: `stress-ng --cpu 0 --cpu-method matrixprod --timeout 60s`. Obserwacja: temp1 i Tctl, chwila osiągnięcia 95 °C, przełączenie na 255 najpóźniej po 3 odczytach, `Emergency` = true, wzrost obrotów.
  6. Koniec obciążenia: sam po 60 s albo przez straż lub Ctrl+C.
  7. Powrót do automatu: po blokadzie, gdy temp1 spadnie poniżej 85 °C, `AcknowledgeEmergency` (hasło), a pomocnik ustawia automat (D48); bez blokady `SetAuto s cpu1` (bez hasła). Sprawdzenie: `pwm1_enable` = 2, a krzywa `pwm1_auto_point*` równa odczytowi z punktu 1.
  8. Gdyby pomocnik nie odpowiadał: `sudo systemctl stop baseboard-helper` (B3 a ustawia automat); w ostateczności automat bezpośrednio: `echo 2 | sudo tee /sys/class/hwmon/hwmonN/pwm1_enable` (N tego urządzenia, którego `name` to `nct6687`).
- **Wynik:** plik CSV i opis w wierszu kroku 13: przesunięcie temp1 względem Tctl, najwyższe temp1 i Tctl, czy i kiedy zadziałała B4, czas od progu do 255 i do wzrostu obrotów. Gdy temp1 nie dojdzie do 95 °C w 60 s, B4 na prawdziwym progu zostaje niesprawdzone, a O10 dostaje odpowiedź, na jakiej temperaturze temp1 się zatrzymuje.
- **Czas:** ok. 5 minut, z tego obciążenie najwyżej 60 s.

#### M2.6 Pomocnik i dystrybucja

- **Pakiety.** `baseboard-helper`: pomocnik, jednostka systemd, aktywacja i polityka D-Bus, akcje polkit, profile płyt w `/usr/share/baseboard/boards`, od M3 reguła udev RGB. `baseboard`: aplikacja natywna zależna od `baseboard-helper` (opcjonalnie, dla dystrybucji i testów). Oba z jednego `baseboard.spec`, budowane w COPR na projekcie właściciela. Aplikacja przede wszystkim jako Flatpak (D39).
- **`meson install`.** Opcja do budowania samego pomocnika (do instalacji obok Flatpaka) i samej aplikacji. Pomocnik wymaga prefiksu `/usr`, bo polkit czyta akcje tylko z `/usr/share/polkit-1/actions`. Przy instalacji z mesona nie ma skryptów odinstalowania: README podaje `baseboard-helper --restore-all` przed `ninja uninstall`.
- **Zgodność wersji.** Interfejs `Fans1`, w nim `ApiVersion`: 1 = tryb ręczny (`GetChannels`, `GetState`, `SetManual`, `SetAuto`, `SetAllAuto`, `AcknowledgeEmergency`), 2 = krzywe (`GetEcCurve`, `SetEcCurve`, `RestoreOriginalCurve`), 3 = źródło (`SetEcSource`). Aplikacja pokazuje tylko to, co obsługuje pomocnik, a resztę z dopiskiem „wymaga nowszego pomocnika” (makieta stanów). Nowszy pomocnik niż aplikacja: aplikacja korzysta z tego, co zna. Zmiana niezgodna wstecz to `Fans2` obok `Fans1` przez co najmniej jedno wydanie (§10.4). `HelperVersion` trafia na stronę System i do tekstu do schowka. `ProfileDigest` różny od profilu aplikacji: sterowanie według profilu pomocnika i wiersz o tym na stronie.
- **Wykrywanie braku pomocnika.** M2a (0.2.0) z pomocnikiem w ogóle się nie łączy. Od 0.3.0 przy wejściu na stronę aplikacja czyta `ApiVersion` (to aktywuje pomocnika, który kończy się po 30 s bezczynności): `ServiceUnknown` → brak pakietu („Jak zainstalować” prowadzi do README); `AccessDenied` od pośrednika Flatpaka → odebrane uprawnienie (wiersz z poleceniem `flatpak override`); brak odpowiedzi → „Pomocnik nie odpowiada”. Przy `read-only` strona mówi o sterowniku, nie o pomocniku.
- **Polkit z Flatpaka (O19).** Pomocnik pyta polkit o podmiot `system-bus-name` wywołującego; z Flatpaka połączenie przechodzi przez pośrednika D-Bus, który działa w sesji użytkownika, więc „aktywna sesja” i okno agenta gospodarza powinny działać jak natywnie. Niepewne jest `auth_admin_keep`: zapamiętanie autoryzacji może dotyczyć procesu pośrednika. Test w kroku 13 (natywnie i we Flatpaku: ile razy pyta o hasło w ciągu 5 minut). Gdy nie działa jak natywnie: Flatpak pyta przy każdej zmianie i README to mówi; aplikacja łączy zmiany suwaka (B8), więc pytań jest niewiele.
- **Odinstalowanie bez trybu ręcznego.** `%preun` przy usuwaniu (nie przy aktualizacji): `systemctl stop` (B3 a: automat), potem `baseboard-helper --restore-all` (krzywe pierwotne z `/run/baseboard/original-curves.json` przez planer B16 i automat). Bez migawki (nic nie zmieniano) polecenie nic nie robi. Test w kroku 13 i w maszynie wirtualnej (krok 9).

#### M2.7 Wydania

| Wydanie | Zawartość | Warunek | Strona, README, metainfo |
|---|---|---|---|
| 0.2.0 | M2a: strona Wentylatory w podglądzie, tryb w Przeglądzie | kryteria M2a | zrzuty strony Wentylatory (jasny i ciemny styl, pl i en) w `website/screenshots/0.2.0/` i w metainfo; README „What it does today” i tabela poziomów; karta na stronie projektu; podsumowanie aplikacji bez zmian (sterowania jeszcze nie ma); uprawnienia Flatpaka bez zmian |
| 0.3.0 | pomocnik (`baseboard-helper`, COPR) i tryb ręczny | kryteria trybu ręcznego; `hwmon-next` w wydanym jądrze nie jest warunkiem, ale bez niego sterowanie działa tylko z modułem testowym (README mówi to wprost) | instalacja pomocnika w README i na stronie; zrzuty trybu ręcznego; `--system-talk-name` w manifeście; nowe podsumowanie, jeśli obiecuje sterowanie |
| 0.4.0 | edytor krzywych | kryteria krzywych i zapis krzywych w `hwmon-next` (P3) | zrzuty edytora; tabela „Fans and the kernel” i karta na stronie (stan łatek z drzewa jądra, D41) |
| później | M2c (wybór źródła) | łatka 5 i dowód sprzętowy | jak wyżej |

Przy każdym wydaniu: procedura §10.7, nowy katalog zrzutów (stare zostają), stan łatek na stronie i w README sprawdzony w drzewie jądra (`git merge-base --is-ancestor`), EN i PL.

Przed 0.2.0, osobnym krokiem po kroku 4: na stronie projektu sekcja „Coming next: fan control” ze zrzutami prawdziwej strony Wentylatory z kroku 4 (jasny i ciemny styl, EN i PL), z podpisem, że to widok z łatkami jądra, które są w recenzji.

**Zasady całego M2a:** tylko odczyt, żadnego zapisu do `/sys` (`strace` jak w M1); interfejs wygląda jak zaakceptowane makiety, a krok ze stroną Wentylatory pokazuje zrzuty prawdziwej aplikacji obok renderów makiet z listą różnic; jeden krok, raport (co zrobione, testy, co sprawdzone na sprzęcie, co ma sprawdzić właściciel), stop.

#### M2.8 Kryteria ukończenia i testy

**M2a (0.2.0)**
- Strona Wentylatory na sztucznych drzewach wszystkich poziomów i na sprzęcie z modułem Fedory i modułem v1 wygląda jak makiety przyjęte w P1, natywnie i we Flatpaku, w obu stylach, przy szerokim i wąskim oknie, po polsku i po angielsku.
- Krzywe na podglądzie równe `cat` atrybutów `pwmN_auto_point*`; cel z modelu równy `pwmN` w stanie ustalonym (krok 2).
- Aplikacja niczego nie zapisuje i nie łączy się z pomocnikiem (`strace` jak w M1).
- CPU (D29): cała aplikacja z widoczną stroną Wentylatory poniżej 1 % jednego rdzenia (`pidstat -t`, 60 s, build release); przy ukrytym oknie praktycznie zero (otwarte z M1).
- Orca odczytuje nazwy, wartości, tryby i opisy wykresów na stronie i podstronie (otwarte z M1).
- Przy włączonym „Ogranicz animacje” brak przejść.

**Tryb ręczny (0.3.0)**
- Na module testowym `sys1`–`sys4` i `cpu1` (po P9) działają w trybie ręcznym, a obroty reagują. `pump1` (P8), `ezconn` (D32) i `pwm3` są tylko do odczytu z wyjaśnieniem.
- Każda droga zakończenia z B3 (SIGTERM, SIGKILL, abort, zawieszenie pętli i watchdog, przeładowanie modułu, aktualizacja pakietu) zostawia każdy kanał w trybie ręcznym w `pwmN_enable = 2`, co sprawdzamy odczytem sysfs po zdarzeniu; wyłączenie i restart wpisem w dzienniku z poprzedniego uruchomienia.
- Po S3 tryb ręczny jest taki jak przed uśpieniem: odtworzony przez sterownik albo przez B12.
- Minimum B5 i zakaz B6 nie dają się obejść żadnym wywołaniem D-Bus.
- Próg awaryjny działa na sprzęcie z progiem obniżonym do 50 °C; test CPU według P9 daje wynik O10.
- B11 działa na atrapie i z profilem testowym (wentylator SYS z rolą `cpu`).
- Bez autoryzacji metody `control` są odrzucane, a `restore-auto` przechodzi; natywnie i we Flatpaku (O19).
- Na jądrze Fedory 7.3.3 pomocnik niczego nie zapisuje, a strona pokazuje wyjaśnienie.
- `systemd-analyze security baseboard-helper.service` daje ≤ 5,0, a każde wyjątkowe uprawnienie jest opisane w pliku jednostki.
- Pakiet: instalacja, aktualizacja i odinstalowanie bez kanału w trybie ręcznym i bez plików poza listą z §10.5.

**Krzywe (0.4.0)**
- Obroty idą za krzywą zapisaną w EC pod obciążeniem; krzywa działa po zatrzymaniu pomocnika i zamknięciu aplikacji.
- Odczyt po każdym kroku planu równy planowi (planer nigdy nie wywołuje przycinania).
- „Przywróć krzywą BIOS” daje dokładnie krzywą pierwotną (porównanie odczytu).
- Po S3 krzywa i tryb jak przed uśpieniem; po restarcie krzywa BIOS-u.
- Krzywa łamiąca B5, B6, B7 albo B14 nie trafia do sterownika.
- Edytor obsługiwany w całości klawiaturą; Orca czyta punkty i ich zmiany.

**Testy**
- Jednostkowe i właściwościowe (proptest) dla logiki z `core`: minimum dla `cpu` i `pump` przy dowolnym wejściu (w jednostkach 0–255, z obcięciem: 76 i 127), walidacja krzywych, planer B16 (dowolna para krzywych → każdy stan pośredni poprawny i chłodzący co najmniej jak stara i nowa), B14 dla `cpu` i `pump` oraz brak wymogu 100 % dla `sys` (D19), zapis krzywej pierwotnej łamiącej B5 (np. domyślna krzywa CPU z §6.3) bez walidacji, edytor z nieaktywnym „Zastosuj” dla takiej krzywej, limit tempa spadku wypełnienia w trybie ręcznym (B8).
- `ModelDriver` jako model sterownika: przycinanie temperatur i wypełnień do sąsiadów, `-EIO` z przywróceniem poprzedniej krzywej, `-ETIMEDOUT`, tempo w automacie jako parametr (testy dla kilku wartości, nie tylko zmierzonych 5 jednostek/s), uśpienie z odtworzeniem i bez.
- Integracyjne: pomocnik na prywatnej magistrali sesyjnej, ze sztucznym sysfs z trzech wariantów i atrapą polkit (szablon `polkitd` z python-dbusmock). Ta konfiguracja jest możliwa tylko w buildzie testowym.
- Wstrzykiwanie błędów na sztucznym sysfs: znikający plik temperatury, wartości niewiarygodne, nieudany zapis, zmiana `pwmN_enable` lub krzywej z zewnątrz, zerowe obroty.
- Testy zabijania: SIGKILL i SIGABRT pomocnika uruchomionego przez `systemd-run` ze sztucznym sysfs, potem sprawdzenie, że `--restore-auto` zadziałało.
- Ręczne na sprzęcie, każdorazowo za zgodą: kanały według D16, obciążenie, uśpienie i wybudzenie, wyłączenie, próg awaryjny, równoległy zapis innego narzędzia.
- Proste testy losowe argumentów D-Bus: złe typy, puste i długie tablice, nieznane klucze, krzywe z niewłaściwą liczbą punktów.
- Interfejs: czyste funkcje wykresu (przeliczenia osi, interpolacja, rozmieszczenie etykiet, opis dla czytnika, ograniczenie ruchu uchwytu do sąsiadów i minimum, klawisze) i model widoku stron (stan karty z poziomu sterownika, możliwości, `tested_writes`, odpowiedzi pomocnika); zrzuty debug porównywane z makietami przy przeglądzie, bez automatycznego porównania obrazów.

#### M2.9 Decyzje (podjęte 2026-11-11)

Pełna treść w §12. Numery P z pierwszej wersji planu zostały w tej sekcji.

| P | D | Decyzja |
|---|---|---|
| P1 | D45, D56 | układ A z poprawkami z przeglądu makiet; makiety zaakceptowane z dwiema ostatnimi poprawkami (D56) |
| P2 | D46 | podział M2: M2a podgląd → M2b (pomocnik jako pakiet, tryb ręczny, edytor) → M2c wybór źródła |
| P3 | D47 | wydania 0.2.0, 0.3.0, 0.4.0; edytor wydany dopiero, gdy zapis krzywych jest w `hwmon-next` |
| P4 | D48 | B4: 100 % od razu, „osiągnie próg” dla procesora, blokada trwała w `/run/baseboard` |
| P5 | D49 | B3(g) przy przeładowaniu modułu, powód `driver-reloaded` |
| P6 | D50 | restart pomocnika przy aktualizacji pakietu, `--restore-all` przy odinstalowaniu |
| P7 | D51 | Flathub po 0.4.0, manifest i zgłoszenie robi właściciel |
| P8 | D52 | pompa bez sterowania przy pustym złączu |
| P9 | D53 | test CPU_FAN1 przy progu 95 °C według procedury z M2.5; uruchamia właściciel po przeczytaniu |
| P10 | D54 | bez procesu w tle przy zamkniętej aplikacji |
| P11 | D55 | kolor akcentu systemu dla krzywej i elementów aktywnych, zieleń dla punktu pracy, ruch z poszanowaniem „Ogranicz animacje” |

#### M2.10 Ryzyka

| # | Ryzyko | Skutek | Co je ogranicza |
|---|---|---|---|
| R1 | v1 odrzucona albo zmieniona w recenzji | edytor później albo inaczej | warstwa sterownika (§3.7), tabela zmian niżej, P3 |
| R2 | `hwmon-next` nie wejdzie do najbliższego wydania jądra albo zmieni się po drodze | użytkownicy długo bez sterowania | wykrywanie po atrybutach; M2a działa na każdym jądrze |
| R3 | dystrybucje dostaną jądro z tymi zmianami z opóźnieniem | mało testujących poza właścicielem | README i strona mówią wprost, czego potrzeba |
| R4 | CPU_FAN1 i pompa bez sprawdzonego zapisu (O5, O3; złącze pompy puste) | na MS-7E61 pompa bez sterowania, CPU dopiero po P9 | D16, P8, P9 |
| R5 | temp1 nie nadąża za Tctl albo staje poniżej progu (O10) | B4 spóźnione albo martwe dla procesora | P4, P9; procesor dławi się sam przy Tjmax |
| R6 | wolne dochodzenie w automacie na CPU (zmierzone ok. 3,3 jednostki/s, ze 124 do 255 w ok. 40 s; pompa niezmierzona, O15) | wolne chłodzenie po B4 | P4 (100 % od razu) |
| R7 | wyładowanie modułu w trybie ręcznym (bez serii Dahlina) | stałe wypełnienie bez nadzoru do załadowania albo restartu | minimum B5, P5, ryzyko opisane w §4 |
| R8 | zawieszenie całego jądra | EC trzyma ostatni tryb | minimum B5 i B14 (ryzyko szczątkowe, §4) |
| R9 | polkit przez pośrednika Flatpaka działa inaczej niż natywnie (O19) | pytanie o hasło przy każdej zmianie albo brak sterowania z Flatpaka | test w kroku 13, opis w README, instalacja natywna jako zapas |
| R10 | Flathub nie przyjmie aplikacji zależnej od pakietu gospodarza (D44) | dystrybucja tylko z własnego repozytorium Flatpak i COPR | aplikacja działa bez pomocnika (podgląd) |
| R11 | koszt CPU wykresów ponad D29 | kryterium M2a niespełnione | pamięć podręczna węzłów, odczyt krzywych co 10 s, bez animacji ciągłych, pomiar w krokach 3 i 4 |
| R12 | własny widżet słabo dostępny dla czytnika ekranu | gorsza obsługa z Orca | lista punktów jako pełna droga, ogłaszanie zmian, test z Orca w krokach 4 i 17 |
| R13 | inne programy sterujące wentylatorami (fancontrol, CoolerControl) | wzajemne nadpisywanie | B10 bez nadpisywania; we Flatpaku procesów nie widać (§10.3) |
| R14 | aktualizacja BIOS-u zmienia domyślne krzywe albo zachowanie EC | inne krzywe pierwotne, inne przypisania | zasady nie zależą od wersji BIOS-u, B18 bierze migawkę od nowa; test właściciela po aktualizacji (M5) |
| R15 | czas właściciela na testy na sprzęcie, każdy za zgodą | wydania się przesuwają | testy zebrane w krokach 13 i 18, z gotowymi poleceniami |

#### M2.11 Zmiany w reszcie dokumentu (wprowadzone 2026-11-11, przed kodem)

Decyzje są w §12 (D45–D56), makiety mają tylko układ A. Wprowadzone zmiany:
- `CLAUDE.md`: B1–B18 zamiast B1–B15. Nagłówek dokumentu: aktualny zakres decyzji.
- §0, §1.4, §5.5.1 i tabela na początku §7: bez prognozy „7.5”; podział M2 według P2.
- §3.3: blokada awaryjna w `/run/baseboard` (P4), restart przy aktualizacji (P6). §3.5: numery `ApiVersion` (M2.6) i powód `driver-reloaded` (P5).
- §4: interpolacja liniowa (po kroku 2), B3(g) (P5), B4 (P4), sytuacje szczególne z M2.5, D14 rozstrzygnięte.
- §5.5.3: przypadek „bez pomocnika” i 0.2.0 (tabela z M2.3). §5.5.4: wariant z P1, osobny przycisk zamiast klikalnej karty, wykres i stany z M2.4.
- §10.4: M2a bez połączenia z pomocnikiem, wykrywanie z M2.6. §10.5: prefiks `/usr`, odinstalowanie. §10.6: F3 według P7.
- §13: O10 z wynikiem z M1 (92 °C po 12 s) i procedurą z M2.5; O19 z planem testu z M2.6.

**Gdy recenzja v1 coś zmieni.** Zmiany dotyczą tylko `SysfsDriver`, `ModelDriver` i próbek w `tests/fixtures/` (§3.7):

| Możliwa zmiana w v2 | Co zmieniamy w warstwie sterownika |
|---|---|
| `-EINVAL` zamiast przycinania przy złamaniu kolejności | mapowanie błędu; planer B16 i tak omija przycinanie |
| zapis tylko w jednym trybie albo krzywa działa od razu w trybie ręcznym | kolejność kroków zapisu (np. najpierw automat); tekst w edytorze |
| cała krzywa jednym atrybutem albo inny zestaw plików | ścieżka zapisu i warunek `curve-write`; planer nieużywany |
| inny warunek 0644 (np. zawsze albo parametr modułu) | nic: wykrywanie po bitach trybu |
| inne granice (np. 0–100 °C) albo jednostki | stałe i przeliczenia |
| inna liczba punktów | nic: liczba z liczby plików, edytor się dostosowuje |
| bez odtwarzania krzywych po wznowieniu | nic: B12 nakłada ponownie |
| kanały CPU i pompy bez zapisu krzywej | nic: możliwości z bitów trybu |
| inne kody błędów | mapowanie błędów |
| zapis bez przywracania przy błędzie | B17 sprawdza stan po każdym kroku planu (już tak jest) |

Kontrakt D-Bus, zasady §4 i interfejs zostają bez zmian.

### M3 – RGB

**Zakres**
- Reguła udev, wykrywanie urządzenia i zmian (udev monitor).
- Kodowanie i dekodowanie raportów 0x50/0x51 oraz odczytu wersji firmware, warstwa transportu jako cecha (prawdziwy hidraw i atrapa do testów).
- Wszystko z §5.2. Opis protokołu w `docs/rgb-protokol.md`.
- Reguła udev w pakiecie RPM z M2b (§10).

**Kryteria ukończenia**
- Na MS-7E61 z urządzeniem ARGB działa każdy obsługiwany tryb, a `GET_FEATURE 0x50` po zapisie zwraca to, co wysłano.
- Direct ustawia kolory diod. Zmiana z store = 0 nie zmienia ustawień startowych; z store = 1 przeżywa zimny start.
- Aplikacja nie wysyła raportów innych niż 0x50, 0x51 i odczyt wersji firmware (z jednym dozwolonym bajtem polecenia), co potwierdza test, przegląd kodu i (za zgodą) zapis ruchu `usbmon`.
- Wersja firmware kontrolera jest widoczna na stronie System; brak odpowiedzi daje „wersja nieznana”, a nie błąd.
- Interfejs nie zamarza przy blokującym `SET_FEATURE` ani przy zniknięciu urządzenia.
- Bez reguły udev strona pokazuje jasną instrukcję.
- Reguła daje ACL tylko dla 0DB0:0076: `getfacl` dla hidraw kontrolera RGB pokazuje użytkownika, a hidraw audio (0DB0:CC78) nie.

**Testy**
- Jednostkowe:
  - wzorcowe ciągi bajtów przygotowane z publicznego opisu: rozmiary 290 i 727 B, układ wiersza strefy,
  - store = 0 jako wartość domyślna,
  - ograniczenia jasności, prędkości i liczby diod (≤ 240),
  - `decode(encode(x)) == x`,
  - zachowanie nieedytowanych stref z odczytu,
  - pakiet odczytu wersji firmware ma wyłącznie dozwolony bajt polecenia; dekodowanie odpowiedzi krótkiej, pustej i z nieznanym formatem.
- Na atrapie transportu: kolejność „najpierw GET 0x50”, limit czasu, zniknięcie urządzenia.
- Udev: `udevadm test` i `udevadm info`, `getfacl /dev/hidrawN`.
- Ręczne: wszystkie tryby, Direct, uśpienie i wybudzenie, zimny start po store = 1.

### M4 – Wydajność i restart do UEFI

**Zakres**
- Wszystko z §5.3 oraz z §5.4 restart do ustawień UEFI i link do strony pomocy. Informacje o systemie i „Kopiuj informacje” są od M1.
- Profil energii w tekście „Kopiuj informacje”.

**Kryteria ukończenia**
- Zmianę profilu w aplikacji widać w szybkich ustawieniach GNOME i w `tuned-adm active`, a zmiana z zewnątrz pojawia się w aplikacji w ciągu 1 s.
- Bez usługi profili lub logind strona wyjaśnia, czego brakuje, i nic się nie wywraca.
- Restart do UEFI na MS-7E61 wchodzi do ustawień firmware. Anulowanie w oknie sesji cofa flagę: `bootctl reboot-to-firmware` pokazuje „not set”.
- Tekst „Kopiuj informacje” zawiera aktywny profil energii.

**Testy**
- Integracyjne z python-dbusmock (szablony `power_profiles_daemon` i `logind`) na prywatnej magistrali. Pakiet `python3-dbusmock` jako zależność developerska (instalacja za zgodą).
- Jednostkowe: obsługa `challenge` i `na` z `CanRebootToFirmwareSetup`, profil energii w tekście do schowka.
- Ręczne: przełączanie profili, prawdziwy restart do UEFI, anulowanie restartu.

### M5 – Aktualizacja BIOS-u bez flashowania z systemu

**Warunek wstępny:** M4 (restart do UEFI). Etap nie zależy od jądra.

**Poza zakresem:** flashowanie z systemu w jakiejkolwiek formie: narzędziami producenta, przez LVFS, przez zapis do pamięci flash albo przez zapisanie z systemu polecenia, które po restarcie samo uruchamia narzędzie aktualizacji (§1.3, D7). Do UEFI wchodzimy wyłącznie standardowym `OsIndications` przez logind.

**Zakres**
- Wszystko z §5.6.
- Kod sieciowy w jednym module `app`, za cechą (trait) z atrapą do testów. Przy wyłączonej opcji jest nieosiągalny.
- Propozycje bibliotek (do potwierdzenia `cargo deny`):
  - libsoup 3 przez soup3-rs: proxy i TLS z ustawień GNOME;
  - parser XML, np. quick-xml;
  - SHA-256 z GLib;
  - UDisks2 przez zbus.

**Kryteria ukończenia**
- **Opcja wyłączona:** aplikacja nie otwiera żadnych połączeń sieciowych. `strace -f -e trace=connect` pokazuje tylko gniazda AF_UNIX.
- **Opcja włączona:** połączenie wyłącznie po kliknięciu. Przy dłuższej pracy aplikacji nic nie łączy się w tle.
- **Wynik sprawdzenia:**
  - na MS-7E61 poprawne porównanie bieżącej wersji z listą;
  - wydanie z niespełnionym albo niesprawdzalnym warunkiem dopasowania nigdy nie daje wyniku „Dostępna”;
  - błędy sieci, kody HTTP i zmieniony format dają komunikat, a nie awarię.
- **Weryfikacja pliku:** plik z niezgodną sumą jest odrzucany i usuwany.
- **Pendrive:**
  - lista nośników nigdy nie pokazuje dysków wewnętrznych ani systemowych;
  - bez potwierdzenia nic nie trafia na nośnik, a formatowanie wymaga osobnej zgody;
  - suma pliku na nośniku jest zgodna, a nośnik zostaje odmontowany.
- **Narzędzie w UEFI:** na MS-7E61 wbudowane narzędzie widzi plik na przygotowanym pendrivie. Sprawdzamy to bez zatwierdzania flashowania; samą aktualizację robimy tylko za osobną decyzją właściciela.
- **Lista kontrolna:** baner pojawia się po zmianie wersji BIOS-u i można go zamknąć.

**Testy**
- **Jednostkowe:**
  - parser listy na sztucznych próbkach w `tests/fixtures/bios-list/`: poprawna, inna kolejność elementów, nieznane pola, brak pól, uszkodzony XML, pusta, za duża;
  - porównanie wersji, wybór wpisu dla płyty;
  - warunki dopasowania i wynik trójstanowy: wersja w zakresie i poza nim, wersja pominięta, wykluczony procesor, warunek niesprawdzalny w Linuksie, nieznany rodzaj warunku, wersje nieporównywalne;
  - bezpieczne wyodrębnianie z archiwum (`..`, ścieżki bezwzględne, za duże pliki);
  - weryfikacja sumy.
- **Integracyjne:**
  - lokalny serwer HTTPS na `localhost` z certyfikatem testowym: 200, 404, 500, zerwane połączenie, przekroczony czas, przekierowanie;
  - atrapa UDisks2 na prywatnej magistrali (python-dbusmock): nośnik wymienny, dysk wewnętrzny, dysk systemowy, nośnik bez FAT32, brak miejsca.
- **Ręczne na MS-7E61:**
  - sprawdzenie i pobranie;
  - przygotowanie prawdziwego pendrive'a;
  - restart do UEFI i odnalezienie pliku w narzędziu aktualizacji.

---

## 8. Jakość – wymagania wspólne dla wszystkich etapów

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `meson test`.
- `cargo deny check licenses`: zależności zgodne z licencją projektu.
- `desktop-file-validate`, `appstreamcli validate`, `glib-compile-schemas --strict`, `msgfmt --check`. Tłumaczenie pl kompletne (`msgfmt --statistics` bez nieprzetłumaczonych).
- Każdy blok `unsafe` z komentarzem `// SAFETY:`, wyłącznie w modułach FFI.
- Dostępność: każdy element interaktywny i każda wartość ma nazwę dla czytnika ekranu; nic nie opiera się wyłącznie na kolorze (np. przekroczenie tolerancji ma też ikonę i tekst).
- Dokumentacja (`docs/`) aktualizowana w tym samym commicie co zmiana zachowania.

---

## 9. Internacjonalizacja

- gettext od pierwszego commitu z kodem: msgid po angielsku, pierwsze tłumaczenie `pl`.
- Tłumaczone są także: plik `.desktop`, metainfo, opisy akcji polkit (`msgfmt` w meson, `custom_target` zależny od `po/LINGUAS` i plików `.po`; `i18n.merge_file` nie przebudowuje pliku po zmianie tłumaczeń) i nazwy z ról profilu płyty (§6.2).
- Zdania nie są składane z kawałków. Liczba mnoga przez `ngettext`, kontekst przez `pgettext`.
- Komunikaty pomocnika w dzienniku są po angielsku. Użytkownik widzi tłumaczenia przygotowane przez aplikację na podstawie nazwy błędu D-Bus.

---

## 10. Dystrybucja

**Zasada (D39).** Aplikację instaluje się przede wszystkim jako Flatpak, docelowo z Flathub, z App ID `me.bojanowski.Baseboard`. Pomocnik systemowy (M2) i reguła udev (M3) nie mogą być w Flatpaku, więc instaluje je osobny pakiet systemowy. Instalacja natywna (`meson install`, RPM) zostaje: do rozwoju i testów na sprzęcie oraz dla dystrybucji.

### 10.1 Co widzi piaskownica Flatpak

Sprawdzone 2026-11-11 na MS-7E61 (Flatpak 1.18.4, runtime GNOME 50 i 51) próbką tylko do odczytu uruchomioną w runtime z różnymi uprawnieniami.

| Zasób | Bez uprawnień | Z uprawnieniem | W manifeście |
|---|---|---|---|
| `/sys/class/hwmon` i urządzenia pod nim: odczyty, `pwm*`, punkty krzywych, tryby plików | działa; cały `/sys` jest tylko do odczytu, próba zapisu kończy się błędem | – | – |
| `/sys/class/dmi/id` (pola bez numerów seryjnych), `/sys/class/drm` (VBIOS), `/sys/class/nvme`, `/sys/block`, cpufreq, `/proc/cpuinfo`, wersja jądra | działa | – | – |
| `/proc/modules` (moduł z jądra czy spoza, D40) | działa: lista modułów gospodarza z flagami skażenia, zawsze bieżąca | – | – |
| `/sys/module/nct6683` | niewidoczne | `--filesystem=/sys/module/nct6683:ro` podpina katalog tylko wtedy, gdy istnieje przy starcie aplikacji | nie (D40): bez modułu przy starcie Flatpak po cichu pomija podpięcie i stary kod uznałby sterownik za wbudowany |
| `/sys` jako całość | to tmpfs z podpiętymi `/sys/class`, `/sys/devices`, `/sys/bus` i innymi; libsensors 3.6.2 bez `SENSORS_SYSFS_ROOT` uznaje to za brak sysfs (`sensors_init` −4) | `--env=SENSORS_SYSFS_ROOT=/sys`: libsensors tylko ostrzega na stderr („/sys: Not a sysfs mount.”) i działa | tak |
| baza udev `/run/udev` (pamięć, nazwy PCI z hwdb) | niewidoczna | `--filesystem=/run/udev:ro` | tak |
| konfiguracja lm-sensors gospodarza (`/etc/sensors3.conf`, `/etc/sensors.d`), profile administratora (`/etc/baseboard/boards`) | niewidoczna; `/etc` w piaskownicy należy do runtime'u | `--filesystem=host-etc:ro`: cały `/etc` gospodarza w `/run/host/etc`, tylko do odczytu. Pojedynczych ścieżek z `/etc` Flatpak nie udostępnia („ścieżka zastrzeżona”) | nie: za szerokie. Użytkownik może je dodać sam: `flatpak override --user --filesystem=host-etc:ro me.bojanowski.Baseboard` |
| `os-release` | `/etc/os-release` to plik runtime'u („GNOME 51 (Flatpak runtime)”); system gospodarza jest w `/run/host/os-release` | – | – |
| procesy gospodarza (`/proc/<pid>/comm`, programy sterujące wentylatorami, §5.4) | niewidoczne: piaskownica ma własną przestrzeń PID | brak uprawnienia | – |
| `/dev/hidraw*` (RGB, M3) | niewidoczne | tylko `--device=all`, które odsłania też m.in. `/dev/port` | otwarte (O18) |

**Uprawnienia w M1** (`finish-args`): `--share=ipc`, `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--filesystem=/run/udev:ro` oraz zmienna `--env=SENSORS_SYSFS_ROOT=/sys`. Bez sieci, bez `host-etc`, bez `--device=all`. Od M2 dochodzi `--system-talk-name=me.bojanowski.Baseboard.Helper` (§10.4), w M5 sieć dla sprawdzania BIOS-u.

### 10.2 Runtime i biblioteki

- **Runtime:** GNOME 51 (GTK 4.24). Minimum budowania natywnego zostaje GNOME 50 (GTK 4.22, libadwaita 1.9). Rust z rozszerzenia `org.freedesktop.Sdk.Extension.rust-stable` (dziś 1.98, wymagamy 1.92).
- **libudev** jest w runtime GNOME 50 i 51 (`libudev.so.1`); `dlopen` działa bez zmian w kodzie.
- **libsensors** nie ma ani w Platform, ani w SDK GNOME 50 i 51. Dołączamy go jako moduł manifestu: sama biblioteka z lm-sensors 3.6.2 (archiwum z GitHuba z sumą SHA-256 w manifeście), zbudowana z `ETCDIR=/run/host/etc`, bez programów i bez biblioteki statycznej. W piaskownicy wymaga `SENSORS_SYSFS_ROOT=/sys` (§10.1). Bez `host-etc` libsensors nie widzi żadnej konfiguracji i daje wartości z sysfs; mnożniki napięć liczy wtedy aplikacja z profilu (§5.1), a kanały ukryte przez `ignore` w konfiguracji płyty ukrywa `[hidden]` profilu. Z `host-etc` działa konfiguracja gospodarza, tak jak w instalacji natywnej.

### 10.3 Zmiany w aplikacji dla piaskownicy (M1)

Aplikacja rozpoznaje piaskownicę po pliku `/.flatpak-info` i wtedy:
- system czyta z `/run/host/os-release`;
- pliki lm-sensors na stronie System i profile administratora czyta z `/run/host/etc`, jeśli jest widoczny. Inaczej wiersz „Konfiguracja lm-sensors” mówi, że w piaskownicy nie jest dostępna, i podaje polecenie `flatpak override` z §10.1, a przełącznik „Nazwy z lm-sensors” jest nieaktywny z tym samym wyjaśnieniem;
- wiersz o innych programach sterujących wentylatorami mówi „Nie można sprawdzić w Flatpaku”, bez ikony stanu, nigdy z zielonym znaczkiem;
- wiersz biblioteki lm-sensors mówi, że działa bez konfiguracji systemu, której Flatpak nie udostępnia;
- tekst „Kopiuj informacje” dostaje pole ze sposobem instalacji (`Flatpak` z nazwą runtime'u albo `native`), także w instalacji natywnej.

Podsumowanie w metainfo, w pliku `.desktop` i w oknie „O programie” nie obiecuje sterowania przed M2: „Monitor your motherboard sensors” (32 znaki), po polsku „Podgląd czujników płyty głównej”.

**Różnice wobec instalacji natywnej** (porównanie 2026-11-11 na MS-7E61, moduł v1, ten sam kod w obu wersjach):
- bez `host-etc`: konfiguracja lm-sensors gospodarza nie działa. Odczyty płyty są takie same (mnożniki i ukryte kanały z profilu), ale kanały ukryte tylko przez `ignore` w konfiguracji użytkownika wracają: na tej maszynie były to numerowane czujniki dysków NVMe (Sensor 1, 2 i 8 pierwszego dysku, 1 i 2 drugiego, razem pięć wierszy); od D42 aplikacja ukrywa je sama, tak samo natywnie i we Flatpaku. Przełącznik etykiet z lm-sensors jest nieaktywny;
- z `host-etc:ro`: etykiety, `ignore` i lista plików konfiguracji jak w instalacji natywnej;
- strona System: system gospodarza, pamięć, nazwy PCI, VBIOS, dyski i stan sterownika z pochodzeniem modułu jak w instalacji natywnej; inne programy sterujące wentylatorami nie do sprawdzenia;
- tekst do schowka różni się tylko polem `Installation`;
- libsensors pisze na stderr jedno ostrzeżenie przy każdym starcie źródła odczytów (§10.1).

Bez zmian w kodzie: własne nazwy (D22) leżą w `$XDG_DATA_HOME` piaskownicy, czyli `~/.var/app/me.bojanowski.Baseboard/data/baseboard/boards/`, a przycisk w preferencjach otwiera je przez portal. Ustawienia GSettings Flatpak trzyma osobno od instalacji natywnej. Aplikacja nie pisze do sysfs ani w piaskownicy, ani poza nią (B1); w piaskownicy `/sys` jest dodatkowo tylko do odczytu.

### 10.4 Aplikacja i pomocnik (od M2)

| Pakiet | Zawartość | Skąd |
|---|---|---|
| `me.bojanowski.Baseboard` (Flatpak) | aplikacja, profile płyt do nazw i odczytów | Flathub, lokalnie `flatpak-builder` |
| `baseboard-helper` (pakiet systemowy) | pomocnik, jednostka systemd, aktywacja i polityka D-Bus, akcje polkit, profile płyt w `/usr/share/baseboard/boards`; od M3 reguła udev dla RGB | `meson install` z opcją budowania samego pomocnika, docelowo RPM w COPR |
| `baseboard` (RPM, opcjonalnie) | aplikacja natywna, zależna od `baseboard-helper` | dla dystrybucji i testów |

- **Uprawnienie D-Bus:** Flatpak dostaje tylko `--system-talk-name=me.bojanowski.Baseboard.Helper`. Okna polkit pokazuje agent gospodarza, a autoryzacja działa jak przy aplikacji natywnej (do potwierdzenia w M2b, O19).
- **Brak pomocnika:** podgląd M2a działa bez pomocnika, bo czyta sysfs sam; wydanie 0.2.0 z pomocnikiem w ogóle się nie łączy i nie odsyła do pakietu, którego jeszcze nie ma. Od 0.3.0 przy wejściu na stronę Wentylatory aplikacja odczytuje właściwość `ApiVersion`, co aktywuje pomocnika przez D-Bus (pomocnik kończy się po 30 s bezczynności). Brak odpowiedzi oznacza „Pomocnik nie odpowiada”; przy sterowniku `read-only` strona mówi o sterowniku, nie o pomocniku. Szczegóły: §7 M2, M2.6. Błąd `ServiceUnknown` oznacza, że pomocnik nie jest zainstalowany: strona zostaje w podglądzie i mówi, że sterowanie wymaga pakietu `baseboard-helper`, z odnośnikiem do instrukcji. `AccessDenied` od pośrednika D-Bus Flatpaka oznacza odebrane uprawnienie: strona podaje polecenie, które je przywraca. Strona System ma wiersz „Pomocnik systemowy” (zainstalowany z wersją, brak, niezgodny), a tekst do schowka zawiera wersję pomocnika.
- **Zgodność wersji:** nazwa interfejsu niesie wersję główną (`Fans1`, §3.5). W jej obrębie zmiany są tylko zgodne wstecz, a `ApiVersion` rośnie przy każdej nowej metodzie lub właściwości (1 tryb ręczny, 2 krzywe, 3 źródło; §3.5). Aplikacja korzysta tylko z tego, co zgłasza `ApiVersion`, a resztę ukrywa z dopiskiem, że wymaga nowszego pomocnika. Zmiana niezgodna wstecz to `Fans2` obok `Fans1` przez co najmniej jedno wydanie. Nowa właściwość `HelperVersion` (`s`) daje wersję pakietu do strony System i zgłoszeń błędów.
- **Profile:** o sterowaniu rozstrzyga profil pomocnika (B9, D33): aplikacja pokazuje sterowanie tylko przy kanałach, które pomocnik zgłosił w `GetChannels` jako sterowalne. Profil aplikacji z Flatpaka służy do nazw i odczytów. Nowa właściwość `ProfileDigest` (`s`, SHA-256 pliku profilu) pozwala wykryć, że oba profile się różnią. Wtedy aplikacja trzyma się danych pomocnika i pokazuje dopisek o różnych wersjach profilu.
- **Zasady B1–B18:** wszystkie działają po stronie pomocnika i żadna nie wymaga, żeby aplikacja była z tego samego pakietu. Wspólną instalację zakładało tylko zdanie z §3.6, że aplikacja i pomocnik „liczą to samo” z tego samego `core`. Przy różnych wersjach wynik walidacji w aplikacji (B5, B7, B14) jest tylko podpowiedzią w interfejsie. Pomocnik i tak decyduje, a jego `InvalidArgument` lub `NotSupported` aplikacja pokazuje jako zwykły komunikat. B1 piaskownica dodatkowo wymusza, bo `/sys` jest w niej tylko do odczytu.

### 10.5 Instalacja natywna

1. M1: `meson setup build --prefix=…` i `meson install`. Do testów wystarczy prefiks w katalogu domowym.
2. Od M2b: `packaging/rpm/baseboard.spec` budowany lokalnie (`rpmbuild`, potem COPR), z podpakietem `baseboard-helper` (pomocnik, jednostka systemd, pliki D-Bus, akcje polkit). Zależności Rust dołączone przez `cargo vendor`, żeby budowanie działało bez sieci. Przy aktualizacji pakiet uruchamia pomocnika ponownie, a przy odinstalowaniu (nie przy aktualizacji) zatrzymuje go i woła `baseboard-helper --restore-all`: krzywe pierwotne i automat na kanałach zmienionych przez Baseboard (D50). Pomocnik instalowany przez `meson install` wymaga prefiksu `/usr` (polkit czyta akcje tylko z `/usr/share/polkit-1/actions`); meson nie ma skryptów odinstalowania, więc README podaje `baseboard-helper --restore-all` przed `ninja uninstall`.
3. Od M3: reguła udev dla RGB w pakiecie `baseboard-helper`, żeby działała też z aplikacją z Flatpaka.

| Ścieżka | Zawartość |
|---|---|
| `/usr/bin/baseboard` | aplikacja |
| `/usr/libexec/baseboard-helper` | pomocnik |
| `/usr/lib/systemd/system/baseboard-helper.service` | jednostka |
| `/usr/share/dbus-1/system-services/me.bojanowski.Baseboard.Helper.service` | aktywacja przez D-Bus |
| `/usr/share/dbus-1/system.d/me.bojanowski.Baseboard.Helper.conf` | kto może zająć nazwę i wywoływać metody |
| `/usr/share/polkit-1/actions/me.bojanowski.Baseboard.policy` | akcje polkit |
| `/usr/lib/udev/rules.d/60-baseboard-rgb.rules` | ACL dla kontrolera RGB |
| `/usr/share/baseboard/boards/*.toml` | profile płyt |

### 10.6 Kroki Flatpaka i wymagania Flathub

| Krok | Zakres |
|---|---|
| F1 (zrobione 2026-11-11) | manifest `build-aux/flatpak/me.bojanowski.Baseboard.json` (miejsce przyjęte w aplikacjach GNOME), moduł libsensors, źródła Cargo `build-aux/flatpak/cargo-sources.json`, budowanie lokalne `flatpak-builder --user` bez sieci dla Cargo, zmiany z §10.3, porównanie z wersją natywną, `strace`, sekcja README |
| F2 (przed pierwszym wydaniem) | zamknięcie braków z tabeli niżej, wydanie 0.1.0 (D43, procedura w §10.7) |
| F3 (zgłoszenie na Flathub po wydaniu 0.4.0, bez czekania na M2c: D44, D51) | manifest Flathub i zgłoszenie przygotowuje i prowadzi właściciel sam (zasady Flathub wobec AI), repozytorium zgłoszenia, uzasadnienie uprawnień z §10.1, domena bojanowski.me i weryfikacja ID; tylko za zgodą właściciela |

Wymagania Flathub, których dziś nie spełniamy:

| Wymaganie | Dziś | Krok |
|---|---|---|
| Budowanie bez sieci: zależności Cargo jako źródła manifestu (`cargo-sources.json` z `flatpak-cargo-generator`) | zrobione w F1: `flatpak-cargo-generator.py` z oficjalnego `flatpak/flatpak-builder-tools`, commit `9d86d336a39b9af38b86f114b6ce5820fe260f91`, SHA-256 skryptu `0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380`, uruchomiony poza repozytorium; plik trzeba wygenerować od nowa po każdej zmianie `Cargo.lock` | F1 |
| `<releases>` w metainfo z wersją i datą | zrobione w F2: 0.1.0 z opisem po angielsku, tłumaczonym | F2 |
| Wersja wydania i podpisany tag, zgodne w `meson.build`, `Cargo.toml` i metainfo | 0.1.0 we wszystkich trzech, zgodność sprawdza meson przy konfiguracji; tag podpisuje właściciel przy wydaniu (§10.7) | F2 |
| Zrzuty ekranu w metainfo pod stałymi publicznymi adresami | zrobione w F2: Przegląd, System i Przegląd w ciemnym stylu, okno 1000×700, każdy z wariantem polskim, pod https://baseboard.bojanowski.me/screenshots/0.1.0/ (D43); zrzuty strony w `website/img/` mają okno 1500×1000, więcej niż wytyczne Flathub (najwyżej 1000×700) | F2 |
| Działająca strona domowa (walidator Flathub sprawdza adresy) | zrobione 2026-11-11: https://baseboard.bojanowski.me (D41, GitHub Pages) | F2 |
| Opis zgodny z tym, co aplikacja umie, `summary` najwyżej 35 znaków | zrobione w F1 (§10.3) | F1 |
| Kolory marki w metainfo (`<branding>`), zalecane | zrobione w F2: #deddda i #3d3846 (D43) | F2 |
| Domena z App ID, bojanowski.me: linter Flathub wymaga odpowiedzi https://bojanowski.me (`appid-url-not-reachable`), weryfikacja ID plikiem `https://bojanowski.me/.well-known/org.flathub.VerifiedApps.txt` (przekierowania dozwolone, HTTPS obowiązkowe) albo rekordem DNS TXT `_flathub.bojanowski.me` | domena główna odrzuca połączenie (sprawdzone 2026-11-11); strona Baseboard jest pod baseboard.bojanowski.me | F3 (D44) |
| Uzasadnienie uprawnienia `/run/udev:ro` i zmiennej `SENSORS_SYSFS_ROOT` | tylko w tym projekcie; linter ich nie zgłasza, oceniają je moderatorzy | F3 |
| Manifest Flathub bez treści generowanych przez AI, zgłoszenie otwierane i prowadzone przez właściciela, ujawnienie AI (zasady Flathub od 2026-09); źródło git z `tag` i `commit` (typ `dir` jest w Flathub błędem) | manifest lokalny (`dir`) służy tylko do budowania własnego; w repozytorium nie ma plików pod Flathub | F3, właściciel |

### 10.7 Wydanie (procedura)

Ta sama procedura co dla openExpress i Vumi, które korzystają z tego samego repozytorium Flatpak (D43). Wydanie robimy tylko na prośbę właściciela, z notatkami wydania przez niego zaakceptowanymi. Push `main`, tag, wydanie na GitHubie i publikacja w repozytorium Flatpak tylko na jego wyraźne polecenie. Instalacji Flatpaka właściciela nie zmieniamy: żadnego `flatpak install`, `update`, `uninstall` ani `remote-add` na jego koncie; własne próby tylko na gałęzi `devel` w osobnym katalogu Flatpak (`FLATPAK_USER_DIR`, `FLATPAK_SYSTEM_DIR`, `FLATPAK_CONFIG_DIR` w `~/.cache`). W poleceniach `$HOME`, nie `~`, bo po „=” bash tyldy nie rozwija.

1. **Wersja.** `meson.build`, `Cargo.toml` (workspace) i nowy `<release>` na początku listy w metainfo: data dnia tagowania, krótki opis po angielsku (co działa, czego jeszcze nie ma), przetłumaczony w `po/pl.po`. Zgodność trzech miejsc sprawdza meson przy konfiguracji. Zrzuty metainfo nowego wydania idą do nowego katalogu `website/screenshots/<wersja>/` (okno 1000×700, ustawienia domyślne, jasny styl i jeden ciemny, angielski i polski), stare zostają. Sprawdzenia: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `meson test`, `appstreamcli validate --pedantic --no-net` na zbudowanym metainfo (dopuszczalne tylko `cid-contains-uppercase-letter`), linter Flathub `appstream`, audyt drzewa (ścieżki domowe, notatki, dane prywatne); commit.
2. **Push `main`** (właściciel). Strona wystawia nowe zrzuty; potem `appstreamcli validate --pedantic` z siecią bez ostrzeżeń o zrzutach.
3. **Tag** (właściciel, jego kluczem do tagów): `git tag -s vX.Y.Z -m "Baseboard X.Y.Z"`, `git tag -v vX.Y.Z`, push tagu. Po każdym pushu: `git status -sb` bez „do przodu”, `git ls-remote origin <gałąź|tag>` wskazuje lokalny commit.
4. **Pakiet `.flatpak`** z czystego klonu commita tagu, `B=$HOME/.cache/baseboard-flatpak`: klon z `git describe --tags --exact-match` = `vX.Y.Z` i pustym `git status --short`; `flatpak-builder --force-clean --state-dir=$B/state --repo=$B/bundle-repo $B/bundle-build $B/src/build-aux/flatpak/me.bojanowski.Baseboard.json` (bez `--install`); `flatpak build-bundle $B/bundle-repo $B/me.bojanowski.Baseboard.flatpak me.bojanowski.Baseboard --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo`; SHA-256 i rozmiar do opisu wydania. Tłumaczenia są w samej aplikacji (`"separate-locales": false` w manifeście): plik `.flatpak` zawiera tylko gałąź aplikacji, bez rozszerzenia `.Locale`, więc z osobnym rozszerzeniem aplikacja zainstalowana z pliku byłaby tylko po angielsku (sprawdzone 2026-11-11).
5. **Wydanie na GitHubie** (właściciel): `gh release create vX.Y.Z $B/me.bojanowski.Baseboard.flatpak --title "Baseboard X.Y.Z" --notes-file …`; suma SHA-256 pliku pobranego z GitHuba równa lokalnej.
6. **Repozytorium Flatpak** https://flatpak.bojanowski.me (repozytorium GitHub `TomaszBojanowski/flatpak-repo`, GitHub Pages; repozytorium OSTree w katalogu `repo/`, gałąź `master` każdej aplikacji, podpis kluczem GPG `5A170A422BEAC494`, który podpisuje właściciel, hasło przez pinentry):
   - przed: `git pull --ff-only` w klonie (jedno wydanie naraz); zapisać `ostree --repo=$R refs | sort` i `ostree --repo=$R rev-parse` gałęzi `master` pozostałych aplikacji;
   - `flatpak-builder --force-clean --state-dir=$B/state --repo=$R --gpg-sign=5A170A422BEAC494 $B/build $B/src/build-aux/flatpak/me.bojanowski.Baseboard.json` z tego samego czystego klonu tagu (bez `--install` i bez `--default-branch`: gałąź `master`);
   - `flatpak build-update-repo` z tymi samymi tytułem, komentarzem, opisem i stroną co dotąd (ich zmiana zmienia źródło u każdego użytkownika, więc tylko za zgodą właściciela), `--generate-static-deltas --prune --gpg-sign=5A170A422BEAC494 $R`;
   - po: lista gałęzi bez ubytków (przybywają tylko gałęzie Baseboard: aplikacja, `.Locale`, `.Debug`), commity pozostałych aplikacji bez zmian, `ostree show` mówi „Found 1 signature”;
   - zasady: nigdy nie budować repozytorium od zera, nie usuwać gałęzi innych aplikacji, `--prune` bez `--prune-depth`;
   - commit „Publish Baseboard X.Y.Z” (podpisany), push (właściciel); po wdrożeniu Pages `summary`, `summary.sig`, `summary.idx` i `summary.idx.sig` z https://flatpak.bojanowski.me/repo/ odpowiadają 200 i są równe lokalnym; w pustym katalogu Flatpak `flatpak remote-add --user bojanowski https://flatpak.bojanowski.me/bojanowski.flatpakrepo` i `flatpak remote-ls --user --app bojanowski --columns=application,version,commit` pokazują wszystkie aplikacje z oczekiwanymi commitami; katalog testowy usunąć.
7. **Po wydaniu:** stan prac w §7; strona i README, jeśli coś zależało od wydania.

---

## 11. Licencja

**GPL-3.0-or-later** (D2) dla aplikacji, pomocnika i tłumaczeń. Metainfo na CC0-1.0, jak wymaga AppStream i Flathub.

- Jest zgodna ze wszystkimi planowanymi zależnościami: GTK i libadwaita (LGPL-2.1-or-later), libsensors (LGPL-2.1-or-later), gtk-rs i zbus (MIT), `libc` (MIT lub Apache-2.0).
- To najczęstsza licencja aplikacji z GNOME Circle.
- Wymusza otwartość pochodnych, co ma znaczenie przy narzędziu, które steruje sprzętem.
- Kod jądra (łatki do `nct6683`, GPL-2.0) powstaje osobno i nie trafia do tego repozytorium, więc nie ma konfliktu.
- Ograniczenie: kodu na licencji GPL-2.0-only nie da się włączyć. Nie planujemy tego, bo z innych projektów bierzemy tylko opis protokołów.

Rozważone i odrzucone: GPL-2.0-or-later (szersza zgodność, słabsza ochrona) oraz MIT/Apache-2.0 (bez copyleftu).

---

## 12. Decyzje

Decyzje D1–D7 podjęte 2026-11-06, D8–D27 2026-11-10, D28–D56 2026-11-11. Numery etapów w D1–D8 są według numeracji z 0.6.

| # | Data | Decyzja | Skutek |
|---|---|---|---|
| D1 | 2026-11-06 | Nazwa **Baseboard** | App ID `me.bojanowski.Baseboard`, pliki binarne `baseboard` i `baseboard-helper`, domena gettext `baseboard`, nazwa D-Bus `me.bojanowski.Baseboard.Helper`, akcje polkit `me.bojanowski.Baseboard.*`, katalog repozytorium `baseboard` |
| D2 | 2026-11-06 | Licencja GPL-3.0-or-later | §11 |
| D3 | 2026-11-06 | Polkit: `auth_admin_keep` dla `fans.control`, bez hasła w aktywnej sesji dla `fans.restore-auto` | §3.4 |
| D4 | 2026-11-06 | Ustawienia wentylatorów działają do restartu komputera. Po starcie zawsze rządzi automat BIOS. | B12, §5.5. Opcję „przywracaj po starcie” rozważymy po M2b; krzywe w EC (M2c) częściowo rozwiązują ten problem. |
| D5 | 2026-11-06 | Próg awaryjny CPU 95 °C zamiast 90 °C. Źródło: temp1 układu płyty (D34; pierwotnie `k10temp` Tctl) | §4, profil MS-7E61. Ryzen 9000 pracuje normalnie do 95 °C, a przy 90 °C alarm włączałby się przy zwykłym obciążeniu. To złagodzenie B4 zatwierdzone przez właściciela projektu. |
| D6 | 2026-11-06 | Ustalenia z przeglądu makiet (`makiety/`) | §5.1, §5.3–§5.5, §6.2; lista poniżej |
| D7 | 2026-11-06 | Aktualizacja BIOS-u bez flashowania z systemu (M5): sprawdzenie wersji, pobranie z sumą kontrolną, pendrive dla narzędzia w UEFI, restart do UEFI, lista kontrolna. Sieć tylko na żądanie użytkownika, po włączeniu opcji domyślnie wyłączonej. | §1.2, §1.3, §3.4, §5.6, M5. Jedyne połączenia z siecią w aplikacji. |
| D8 | 2026-11-10 | Zakres M1 rozszerzony o informacje o systemie (2026-11-10) | §5.1, §5.4, §7 M1 i M4, O14; lista poniżej |
| D9 | 2026-11-10 | Kolejność etapów: Wentylatory jako M2 zaraz po M1; RGB → M3, Wydajność i restart do UEFI → M4, aktualizacja BIOS-u → M5 (§7) | §7 |
| D10 | 2026-11-10 | Krzywa programowa (pętla w pomocniku, dowolne źródło, np. karta graficzna albo dysk) odłożona poza M2; wentylatorami sterują tryb ręczny i krzywe w EC | §3.3, §5.5 |
| D11 | 2026-11-10 | Przełącznik w interfejsie: Automat BIOS = `pwmN_enable = 2` z krzywą pierwotną, Ręczny = 1, Krzywa = 2 z krzywą zmienioną (§5.5.4) | §5.5.4 |
| D12 | 2026-11-10 | Krzywa pierwotna w `/run` na jedno uruchomienie systemu (B18), zamiast pliku w `/var/lib` z 0.5 | B18 |
| D13 | 2026-11-10 | Usunięcie B3(c): bez przełączania na automat przed uśpieniem; zamiast tego B12 (porównanie po wybudzeniu i ponowne nałożenie). Złagodzenie twardej zasady. | B3, B12 |
| D14 | 2026-11-10 | B4 przy wyjściu dochodzącym do celu stopniowo (na SYS Fan 1 zmierzono ok. 5 jednostek/s, z 30 % do 100 % w ok. 36 s): zostaje „automat” albo „ręczny 100 % od razu, potem automat” | B4, O15 |
| D15 | 2026-11-10 | Wypełnienie w D-Bus w jednostkach 0–255 zamiast procentów (§3.5) | §3.5 |
| D16 | 2026-11-10 | Zapis na kanałach bez testu na sprzęcie (CPU, pompa; O5): blokuje go pole `tested_writes` w profilu, dopóki test za zgodą nie wypadnie dobrze | B9, §6.2, §6.3 |
| D17 | 2026-11-10 | Wybór źródła: samo wykrycie `source-write` nie włącza funkcji; warstwa sterownika obsłuży tylko semantykę z łatki 5 w kształcie wysłanym na listę (M2d) | §5.5.2, M2d |
| D18 | 2026-11-10 | Przebudowa modułu testowego v1 pod jądro 7.3.3 według planu w notatkach lokalnych; kroki z `sudo` wykonuje właściciel | §1.4 |
| D19 | 2026-11-10 | B14 dla roli `sys` (i `aux`): bez wymogu 100 % na progu awaryjnym, wystarczy krzywa poprawna według B7. Twardy wymóg zostaje dla `cpu` i `pump`. Uzasadnienie: BIOS też na to pozwala (domyślnie SYS Fan 1–4 mają płaskie 60 %, a ustawienia właściciela płaskie 30 % i 40 %, §6.3); wentylatory obudowy nie są jedyną ochroną procesora, który ma własny wentylator z twardym wymogiem i sam dławi przy Tjmax. Złagodzenie B14 z 0.6. | B4, B14, §5.5.4 |
| D20 | 2026-11-10 | Przeliczanie wypełnienia jak w BIOS-ie: procent → 0–255 z obcięciem (30 % = 76, 50 % = 127; stałe granice 20 % = 51, 40 % = 102), 0–255 → procent w interfejsie z zaokrągleniem do najbliższego pełnego procentu, więc każdy procent z BIOS-u wraca do tej samej wartości (§3.5) | §3.5, B5, §6.2 |
| D21 | 2026-11-10 | Baza udev przez własny moduł FFI `app::libudev`, ładowany przez `dlopen("libudev.so.1")` jak libsensors; bez crate'ów z wiązaniami. Moduł rośnie w kroku 9 M1 | §3.2, M1 krok 6 i 9, CLAUDE.md |
| D22 | 2026-11-10 | Etykiety użytkownika z `~/.local/share/baseboard/boards/` od kroku 11 M1 (Ustawienia), nie od kroku 6; format do opisania w projekcie przed kodem | §6.2, M1 krok 11 |
| D23 | 2026-11-10 | Nazwa karty graficznej z bazy (`ID_MODEL_FROM_DATABASE`, np. „Navi 44 [Radeon RX 9050 / 9060 XT]”): aplikacja pokazuje część z nawiasu kwadratowego, a bez nawiasu cały wpis; model w `core` zwraca surowy wpis | §5.1 |
| D24 | 2026-11-10 | Separator dziesiętny z `nl_langinfo(RADIXCHAR)` w module `app::i18n`, który już wywołuje `setlocale`; to moduł FFI na liście w CLAUDE.md. Bez crate'ów lokalizacji | §3.2, §5.1, CLAUDE.md |
| D25 | 2026-11-10 | Między liczbą a jednostką lub znakiem procentu spacja niezłamliwa U+00A0, w każdym języku | §5.1 |
| D26 | 2026-11-10 | Nazwy szyn ATX („+12 V”, „+5 V”, „+3,3 V”) z napięcia nominalnego przez formatowanie liczb według `LC_NUMERIC`, nie przez gettext; zastępuje dawne msgid „+3.3 V” → „+3,3 V” | §5.1, §6.2 |
| D27 | 2026-11-10 | Napięcia poniżej 2 V z trzema miejscami po przecinku, pozostałe z dwoma. O progu decyduje nominał, a bez nominału odczyt (w zakresie: większy co do wartości bezwzględnej koniec). Rdzenie, SoC, VDDIO i napięcie rdzenia karty zmieniają się o kilka miliwoltów, a dwa miejsca by to ukryły | §5.1 |
| D28 | 2026-11-11 | Profil dopasowany także po nazwie modelu z DMI (`[match] board_names`, `board_name` bez „(MS-xxxx)”). Kod płyty i customer ID sterownika nie wyznaczają układu złączy. Jeden kod mają płyty o różnych złączach: według instrukcji producenta kod MS-7E56 mają trzy płyty B850, z których jedna nie ma złącza JAF_1. Jeden customer ID obejmuje całe rodziny: dokumentacja `nct6683` wymienia dla NCT6687D-R płyty B850M, B850-P i X870E, a MS-7D75 i MS-7D76 (0x207) mają według instrukcji 8 i 5 złączy. Profil bez listy nazw pasuje jak dotąd, profil z listą tylko do wymienionych modeli; kod bez pasującego modelu → tryb ogólny z informacją, nigdy sterowanie. Pole jest opcjonalne, więc `schema` zostaje 1. Wniosek z analizy interoperacyjności (§2), potwierdzony źródłami publicznymi | §6.1, §6.2, §6.4 |
| D29 | 2026-11-11 | Kryterium CPU M1 dotyczy samej aplikacji: czas użytkownika wszystkich wątków plus czas jądra wątku GTK poniżej 1 % jednego rdzenia przy odświeżaniu co 1 s. Koszt odczytu sprzętu (czas jądra w sterownikach) podajemy osobno jako pomiar. Uzasadnienie: odświeżenie `nct6683` to ok. 6,3 ms aktywnego czekania na EC w jądrze, taki sam koszt ma każdy program czytający czujniki co sekundę, a aplikacja nie ma na niego wpływu poza częstością odczytu. Pomiar i analiza kodu z 2026-11-11 | §7 (kryteria M1) |
| D30 | 2026-11-11 | Zatwierdzone odtworzenie treści, które zginęły bez decyzji przy przejściu z 0.5 na 0.6 (commit w historii sprzed publikacji, w archiwum lokalnym): tabela progów awaryjnych, powód kolejności zapisu w B2, w B8 limit spadku wypełnienia 10 p.p. na sekundę w trybie ręcznym (twarda zasada z testem w M2), skutek niezgodnego odczytu w B10 (`read-back-failed`), brak numeru wersji jądra w §5.5.3, baner także na podstronie krzywej, obciążenie w kryterium M2c i w testach ręcznych M2 | §4, §5.5.3, §5.5.4, M2 |
| D31 | 2026-11-11 | B7: temperatury punktów krzywej niemalejące, a nie rosnące jak w 0.5. Uzasadnienie: sterownik przycina temperaturę punktu do sąsiednich punktów, więc przyjmuje równe temperatury, a v1 udostępnia zapis krzywej tylko kanałom, których krzywa przy ładowaniu ma niemalejące temperatury (§5.5.1). BIOS przy czterech poziomach wypełnia punkty 5–7 kopią punktu 4 (§6.3), więc wymóg rosnących temperatur odrzucałby krzywe z BIOS-u i każdą krzywą o mniej niż 7 różnych punktach. Złagodzenie B7 z 0.6 | B7, §5.5.4, M2 krok 9 |
| D32 | 2026-11-11 | EZ Conn (`ezconn`, `pwm4`) tylko do odczytu, dopóki sterownik nie obejmie kanału (`pwm4_enable`) i zapis nie zostanie sprawdzony na sprzęcie. Zastępuje „w każdym przypadku” z 0.6 i „jeśli sterownik obejmie” z 0.5. Odblokowanie bez zmian w kodzie: `controllable = true` w profilu, gdy pojawi się `pwm4_enable`, i wpis w `tested_writes` po teście za zgodą (D16) | §5.5.3, §6.3, M2b |
| D33 | 2026-11-11 | `GetChannels` znowu podaje, czy kanał jest sterowalny: profil oznacza go jako sterowalny i kanał ma `mode` (§5.5.2). Rozstrzyga pomocnik; aplikacja pokazuje sterowanie tylko przy kanałach, które pomocnik zgłosił jako sterowalne. Pole zniknęło w 0.6 bez decyzji, przy przebudowie kontraktu | §3.5, §5.5.2 |
| D34 | 2026-11-11 | Źródłem progu awaryjnego CPU jest temp1 układu płyty (`nct6687`, rola `cpu` w profilu), a nie `k10temp` Tctl; format profilu bez zmian (próg należy do temperatury urządzenia z `[hwmon]`, §6.2). Krzywe BIOS-u też mają źródło temp1, więc B4 i B14 liczą z tego samego czujnika. Tctl z `k10temp` służy tylko do porównania w teście obciążeniowym M2b: czy temp1 nadąża za Tctl i czy osiąga próg (O10) | §4 (progi), D5, §6.3, O10 |
| D35 | 2026-11-11 | Data BIOS-u na stronie System według `LC_TIME` (`%x`, np. „1.A68 z 07.09.2026”), a nie z nazwą miesiąca w formacie z tłumaczenia. Uzasadnienie: przy `LANGUAGE=en` i polskim `LC_TIME` format z tłumaczenia dawał mieszankę „1.A68 of września 7, 2026”; data idzie za `LC_TIME` tak jak liczby za `LC_NUMERIC` (D24) | §5.4 |
| D36 | 2026-11-11 | Zatwierdzone wybory z kroku 10 M1: w wąskim oknie (do 500 sp) przycisk „Kopiuj informacje” pokazuje samą ikonę, z nazwą dla czytnika ekranu; tekst do schowka zaczyna się od wersji Baseboard; grupy strony System nazywają się „Płyta” (ten sam tekst co na Przeglądzie) i „Czujniki i wentylatory” (zamiast „Sterowniki i dostęp” z makiety, bo wiersz RGB przyjdzie w M3) | §5.4 |
| D37 | 2026-11-11 | Producent modułu pamięci na stronie System tylko wtedy, gdy SMBIOS podaje nazwę. Surowy identyfikator JEDEC (np. „Bank 8, Hex 0x5D”) trafia wyłącznie do tekstu „Kopiuj informacje” (`maker JEDEC …`). Zmienia D8 (identyfikator obok numeru części na stronie). Uzasadnienie: bez tablicy nazw kod nic nie mówi użytkownikowi, a w zgłoszeniu błędu się przydaje | §5.4, O14 |
| D38 | 2026-11-11 | Poprawki interfejsu z przeglądu przed zamknięciem M1 (krok 13); lista poniżej | §3.2, §5.1, §5.4, §6.3 |
| D39 | 2026-11-11 | Flatpak jako podstawowy sposób instalacji aplikacji, docelowo Flathub, App ID `me.bojanowski.Baseboard`. Uprawnienia minimalne i tylko do odczytu, sprawdzone na sprzęcie (§10.1); libsensors jako moduł manifestu. Pomocnik i reguła udev w osobnym pakiecie systemowym `baseboard-helper`, zgodność wersji przez `Fans1`/`ApiVersion`, rozstrzyga profil pomocnika. Zmienia plan „Flatpak później” z 0.6. Odpowiedzi właściciela przy akceptacji: `host-etc` domyślnie nie, tylko przez `flatpak override`; `flatpak-cargo-generator` z przypiętego commita oficjalnego repozytorium; zmiany kodu z §10.3 w F1; runtime GNOME 51 bez podnoszenia minimum natywnego; zgoda na katalog `build-aux/`; podsumowanie bez obietnicy sterowania już w F1 | §3.5, §3.6, §10 |
| D40 | 2026-11-11 | Pochodzenie modułu `nct6683` (z jądra czy spoza) z `/proc/modules` zamiast `/sys/module/nct6683/taint`, natywnie i w Flatpaku; gdy listy nie da się odczytać, „pochodzenie modułu nieznane”. Zastępuje wąskie uprawnienie `--filesystem=/sys/module/nct6683:ro` z projektu D39. Sprawdzone przeładowaniem modułu przy działającej aplikacji z Flatpaka (v1 → moduł Fedory → brak modułu → v1): `/proc/modules` w piaskownicy pokazuje każdą zmianę w ciągu sekundy, aplikacja pokazała prawdę w każdym stanie. Podpięcie `/sys/module/nct6683` nie wystarcza: gdy modułu nie ma przy starcie, Flatpak go nie podpina i nie widać modułu załadowanego później; po przeładowaniu podpięty katalog znika na stałe i nowego modułu nie widać. Stara zasada „brak `taint` = wbudowany” pokazałaby wtedy fałszywe „z jądra” | §5.4, §10.1 |
| D41 | 2026-11-11 | Strona projektu https://baseboard.bojanowski.me: statyczna, w katalogu `website/` (HTML i CSS bez skryptów, analityki i ciasteczek; angielska pod `/`, polska pod `/pl/`; jasny i ciemny motyw według systemu; zrzuty prawdziwej aplikacji), wdrażana przez GitHub Actions na GitHub Pages (`.github/workflows/website.yml`, akcje przypięte do commitów, domena własna ustawiana w repozytorium). Zgoda właściciela na katalogi `website/` i `.github/`. Sekcja o pracy w jądrze (strona i README) wymienia zmiany właściciela dotyczące tej płyty, ze stanem ustalonym w drzewie jądra: „w wydanym jądrze” tylko dla commitów w tagu wydania; hash i link do git.kernel.org tylko dla drzewa Linusa; dla `hwmon-next` i recenzji sam tytuł i stały link (patch.msgid.link, lore, patchwork). Bez wersji jądra, dat, nazwisk innych autorów i ich adresów e-mail w linkach | §3.6, §10.6 |
| D42 | 2026-11-11 | Numerowane czujniki temperatury dysków NVMe („Sensor 1”–„Sensor 8”, `temp2`–`temp9` sterownika `nvme`) domyślnie ukryte; temperatura złożona („Composite”, `temp1`) zawsze widoczna. Reguła w `core`, ta sama natywnie i we Flatpaku, bez udziału konfiguracji lm-sensors. W Preferencjach → Odczyty te czujniki mają przełączniki, na początku wyłączone; włączone zapisuje klucz `shown-readings`. Nazwa po włączeniu: „model · Czujnik temperatury N”, bez zgadywania położenia. Uzasadnienie: sterownik czyta je z logu SMART dysku jako surowe pola, bez opisu położenia, a dyski nie ustawiają dla nich progów (0 K i 0xFFFF K); temperatura złożona to ta, którą dysk porównuje ze swoimi progami (`crit`, alarm). Na tej maszynie natywnie ukrywał je `ignore` w lokalnej konfiguracji, a we Flatpaku dodawały pięć wierszy | §3.2, §5.1, §10.3 |
| D43 | 2026-11-11 | Wydanie 0.1.0 (F2). Metainfo: `<releases>` z datą dnia tagowania i opisem po angielsku, tłumaczonym; nowe zrzuty w oknie 1000×700 (wytyczne Flathub), w domyślnym akcencie, pod stałym adresem https://baseboard.bojanowski.me/screenshots/0.1.0/: Przegląd (domyślny), System i Przegląd w ciemnym stylu, każdy z wariantem polskim (`xml:lang="pl"`); adres donation (Buy Me a Coffee) i `<translation type="gettext">`; podsumowanie bez zmian; kolory marki #deddda i #3d3846 (wybór właściciela po podglądzie banera; wytyczne Flathub zalecają kolory nasycone); zgodność wersji `meson.build`, `Cargo.toml` i najnowszego `<release>` sprawdzana przy konfiguracji meson. Dystrybucja: zalecana instalacja ze wspólnego repozytorium Flatpak właściciela https://flatpak.bojanowski.me (źródło „bojanowski.me apps”, gałąź `master`, ten sam klucz co openExpress i Vumi), plik `.flatpak` w wydaniu na GitHubie jako druga droga, budowanie ze źródeł jako trzecia; procedura w §10.7. README i strona: Flathub „rozważany po sterowaniu wentylatorami”; README mówi, że projekt powstaje z pomocą asystenta AI | §10.6, §10.7 |
| D44 | 2026-11-11 | Zgłoszenie na Flathub (F3) dopiero po M2. Powody: projekt jest młody, a Flathub zwykle nie przyjmuje aplikacji istniejących bardzo krótko; zasady Flathub wobec AI (manifest bez treści generowanych przez AI, zgłoszenie otwiera i prowadzi właściciel, obowiązkowe ujawnienie), więc manifest Flathub i ujawnienie przygotuje właściciel sam, a w repozytorium nie ma plików pod Flathub; sterowanie z M2 wymaga pomocnika systemowego, a aplikacje zależne od składników gospodarza Flathub przyjmuje tylko wyjątkowo, więc zgłoszenie ma pokazać, jak aplikacja działa z pomocnikiem i bez niego. Domena bojanowski.me (odpowiedź HTTPS dla lintera, weryfikacja ID) też w F3 | §10.6 |
| D45 | 2026-11-11 | Strona Wentylatory w układzie A (karty złączy z miniaturą krzywej i punktem pracy, edytor na podstronie), z poprawkami z przeglądu makiet: w pasku bocznym tylko istniejące strony; jeden wiersz „Źródło temperatury” nad kartami (wartość, zakres, próg awaryjny), a w karcie źródło tylko wtedy, gdy różni się od wspólnego; w karcie próg jako linia i strefa bez napisu, w edytorze napis poziomo przy górze linii; „złącze nietestowane” w karcie raz; w wąskim oknie nazwa złącza w całej linii, obroty pod nią, przycisk przywracania krzywej BIOS z podpisem; przy sterowniku bez sterowania zdanie o wypełnieniu tylko w wierszu u góry; tryb ręczny bez wykresu (duża wartość wypełnienia, suwak, obroty) przy równej siatce kart; wartości punktów w edytorze także wpisywane z klawiatury; żaden tekst nie powtarza się na jednym ekranie. Warianty B (lista) i C (jeden wentylator) odrzucone. Makiety po poprawkach czekają na akceptację renderów; do niej bez kodu interfejsu | §7 M2 (M2.4), §5.5.4 |
| D46 | 2026-11-11 | Podział M2: M2a podgląd bez pomocnika; M2b pomocnik jako osobny pakiet systemowy, potem tryb ręczny, potem edytor krzywych (dawne M2c); M2c wybór źródła temperatury (dawne M2d) | §7 M2 (M2.3) |
| D47 | 2026-11-11 | Wydania: 0.2.0 = M2a, 0.3.0 = pomocnik i tryb ręczny, 0.4.0 = edytor krzywych, wydany dopiero, gdy zapis krzywych jest w `hwmon-next`; do tego czasu kod edytora na `main` i testy z modułem v1 | §7 M2 (M2.7) |
| D48 | 2026-11-11 | B4: przy progu od razu 100 % (tryb ręczny 255) na kanałach sterowanych ręcznie przez Baseboard, automat po zdjęciu blokady; dla procesora warunek „osiągnie próg” (≥ 95 °C w 3 kolejnych odczytach) zamiast „przekroczy”; blokada zapisana w `/run/baseboard`, więc trwa po wyjściu i restarcie pomocnika. Rozstrzyga D14. Zaostrzenie B4 | §4 (B4), §3.3 |
| D49 | 2026-11-11 | Nowa reguła B3(g): po ponownym pojawieniu się urządzenia hwmon (przeładowanie modułu) kanały z dziennika B2 w trybie ręcznym wracają do automatu; krzywe porównane z migawką B18 i z zapisem pomocnika, bez nadpisywania; nowy powód `driver-reloaded` w `ControlLost` | §4 (B3), §3.5 |
| D50 | 2026-11-11 | Pakiet pomocnika: przy aktualizacji restart pomocnika (tryb ręczny wraca do automatu przez B3 a, krzywe zostają); przy odinstalowaniu `baseboard-helper --restore-all` (krzywe pierwotne i automat) | §10.5, §7 M2 (M2.6) |
| D51 | 2026-11-11 | Zgłoszenie na Flathub (F3) po wydaniu 0.4.0, bez czekania na M2c; manifest i zgłoszenie przygotowuje właściciel sam (D44) | §10.6 |
| D52 | 2026-11-11 | Pompa na MS-7E61 bez sterowania (bez `tested_writes`), dopóki do PUMP_SYS1 nie zostanie podłączony wentylator albo pompa do testu przypisania (O3) i zapisu (O5) | §6.3, §7 M2 |
| D53 | 2026-11-11 | Test CPU_FAN1 przy prawdziwym progu 95 °C (O5, O10): zgoda co do zasady. Przed wykonaniem właściciel czyta procedurę (§7 M2, M2.5, „Procedura testu CPU_FAN1”) i uruchamia test sam | §7 M2 (M2.5), O5, O10 |
| D54 | 2026-11-11 | Bez procesu w tle przy zamkniętej aplikacji (O6 zostaje otwarte): zasady egzekwuje pomocnik, zdarzenia są w dzienniku systemd, a aplikacja po uruchomieniu pokazuje trwałą blokadę awaryjną (D48) | §3.2, O6 |
| D55 | 2026-11-11 | Krzywa i elementy aktywne wykresu w kolorze akcentu systemu z libadwaita (`accent_bg_color`, za wyborem użytkownika), nie w stałym niebieskim; punkt pracy („teraz”) w zieleni z ikony aplikacji; ruch z planu (przejścia tylko przy zmianach, bez animacji ciągłych) z poszanowaniem ustawienia „Ogranicz animacje”; wartości i podtytuły złączy z kryciem 72 % zamiast przygaszonych 55 % | §7 M2 (M2.4) |
| D56 | 2026-11-11 | Makiety M2 (układ A po poprawkach z D45) i plan M2 zaakceptowane, z dwiema poprawkami bez nowej rundy: baner i zdanie pod nim to jeden komunikat „Zmiany działają także po zamknięciu aplikacji, do restartu komputera”; etykiety „Krzywa z BIOS-u” nie ma przy żadnym złączu (wybór z pary „przy każdym albo nigdzie”), bo przełącznik i tak pokazuje „Automat BIOS”, czyli krzywą z BIOS-u (D11), a złącze bez przełącznika ma ramkę „Tylko podgląd”; edytor nazywa tryb w podtytule („automat BIOS”) | §5.5.3, §5.5.4, §7 M2 (M2.4) |
| D57 | 2026-11-11 | Zaakceptowane przez właściciela 2026-11-11 (propozycja z kroku 4 M2a). Punkt pracy przechodzi płynnie tylko na dużym wykresie; na kartach przeskakuje. Pomiar: strona Wentylatory z sześcioma kartami przy temperaturze zmieniającej się co sekundę o 1 °C (zwykłe przy pracy na komputerze) zużywała 1,9–2,5 % jednego rdzenia, bo każda zmiana dawała 30 klatek całego okna przy 120 Hz; bez przejść na kartach 0,40–0,87 %, kryterium D29 to poniżej 1 %. Na karcie 1 °C to 3–4 px, więc przejście jest tam ledwo widoczne; na dużym wykresie 1 °C to ok. 9 px i ruch jednego wykresu jest czytelny | §7 M2 (M2.4, Ruch) |
| D58 | 2026-11-11 | Opis projektu od pierwszego zdania mówi, że to aplikacja dla płyt głównych MSI: hasło „A native GNOME app for the sensors and fans of MSI motherboards on Linux.” (strona EN i PL, README, metainfo, opis repozytorium na GitHubie), zaraz pod nim dwa krótkie zdania „Sensor readings work on any board. Fan header names and, later, fan control need a board profile; one is verified today: MAG B850M MORTAR WIFI.”, bez obietnic sterowania, którego jeszcze nie ma; podsumowanie w metainfo, pliku `.desktop` i oknie „O programie”: „MSI motherboard sensors and fans”. Strona ma ten sam tytuł i opis w znacznikach `meta` i Open Graph (`og:title`, `og:description`, `og:url`, `og:type`, `og:image` ze zrzutem Przeglądu z własnej domeny), osobno dla wersji polskiej. Nazwa producenta tylko opisowo, nigdy w nazwie aplikacji ani ikonie; zastrzeżenie o znakach towarowych w każdym opisie, który ma na nie miejsce (strona, README, metainfo). Zmienia zasadę z `CLAUDE.md`, która wykluczała nazwę producenta z tekstów interfejsu. Wchodzi do wydania 0.2.0 | `CLAUDE.md`, README, `website/`, `data/` |
| D59 | 2026-11-11 | Na Przeglądzie podtytuł wentylatora wymienia tylko tryb ręczny („SYS_FAN1 · wypełnienie 38 % · tryb ręczny”, po angielsku „manual mode”); automat BIOS, zwykły stan, zostaje bez dopisku, tak jak sprawdzone złącze (dopisek ma tylko złącze nietestowane). Powód: przy domyślnej szerokości okna (1200 px, dwie kolumny) i z zakresem min–maks podtytuł z „automat BIOS” łamał się na dwa wiersze przy każdym wentylatorze; teraz w zwykłym stanie każdy mieści się w jednym, a na dwa łamie się tylko wiersz wentylatora w trybie ręcznym z zakresem. Tryby są nadal czytane przy każdej próbce przy widocznym Przeglądzie, bo inaczej nie byłoby widać przejścia w tryb ręczny | §5.5.3, §7 M2 (krok 5) |
| D60 | 2026-11-11 | Strona Wentylatory w wersji bez sterowania, po przeglądzie właściciela (makiety w tych punktach nie są przerysowane, obowiązuje ta decyzja): (1) zamiast nieaktywnego przełącznika ramka z trybem jako tekstem („Tryb: Automat BIOS”, „Tryb: Ręczny”, przy błędzie odczytu „Nie udało się odczytać trybu”) i kłódką, w stylu ramki złącza niesprawdzonego; (2) nad kartami raz legenda wykresów: strefa poniżej minimum złącza, próg awaryjny, punkt pracy („teraz: temperatura i wypełnienie”), krzywa poza skrajnymi punktami (od D61 „przedłużenie krzywej”), z próbkami rysowanymi jak na wykresie; w wąskim oknie łamie się całymi parami (próbka z podpisem), nigdy w środku podpisu; (3) przycisk „Szczegóły krzywej” zamiast „Pokaż krzywą” (podstrona zostaje: dokładne wartości punktów, źródło, opis działania; w 0.4.0 edytor); (4) opis pod „Źródło temperatury”: „Z tej temperatury kontroler płyty liczy wypełnienie. Szary zakres: minimum i maksimum od startu aplikacji.” (po angielsku „The board's controller computes the duty cycle from this temperature. Gray range: minimum and maximum since the app started.”); drugie zdanie to samo co notka nad Przeglądem (jedno tłumaczenie); po polsku przy 1200 px w jednym wierszu, po angielsku łamie się, a ostatni wiersz ma zawsze co najmniej trzy słowa; (5) w rzędzie dwóch kart ramki trybu i złącza niesprawdzonego mają tę samą wysokość przy dowolnej liczbie wierszy, więc wykresy i stopki stoją równo i żadna karta nie ma pustego miejsca pod wykresem (własny widżet miejsca ramki, który mierzy obie ramki przy szerokości kolumny; `GtkSizeGroup` porównuje bez niej, więc ramka łamana w kolumnie na trzy wiersze liczyła się jak dwa). Wiersz „Sterowanie wentylatorami niedostępne” (sterownik bez sterowania) ma ikonę informacji zamiast trójkąta ostrzeżenia, bo to zwykły stan wydanych jąder, tekst „Sterownik w tym jądrze tylko odczytuje wentylatory[, bez wypełnienia wentylatorów systemowych]. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.” („nowsza wersja” sugerowałaby, że wystarczy aktualizacja) i przycisk „Stan w jądrze” do sekcji o jądrze na stronie projektu; w wąskim oknie przycisk stoi pod tekstem, a tekst ma całą szerokość (odstępstwo od makiety, w której przycisk wiersza „Podgląd bez sterowania”, „Jak zainstalować”, stoi obok tekstu; ten wiersz przyjdzie w 0.3.0 i dostanie ten sam układ); wiersz strony System ma ten sam tekst (bez zdania o wentylatorach systemowych) i nie ma ikony ostrzeżenia. Trójkąt ostrzeżenia zostaje tylko przy prawdziwych problemach (napięcie poza tolerancją, pominięte pliki, brak sterownika lub układu, niesprawdzona rewizja PCB, inne programy sterujące wentylatorami). Numer wersji jądra dopiero po wejściu zmian do wydanego jądra (D41; pozycja w M2.2) | §5.4, §5.5.3, §7 M2 (M2.2, M2.4) |
| D61 | 2026-11-12 | Układ kart strony Wentylatory jako jedna reguła zależna od szerokości, nie od chwilowej treści. Powód: przy oknie 1280 px (połowa ekranu 2560 px, dwie kolumny) nazwy i podtytuły kart łamały się na 2–3 wiersze, także w spoczynku, a przy strzałce z celem („70 % → 100 %”) bardziej; wykresy i przyciski w rzędzie nie stały równo; notka w stopce miała trzy wiersze i zmieniała wysokość karty, gdy się pojawiała i znikała. Reguła: (1) nazwa i podtytuł karty nigdy się nie łamią; (2) obroty i wypełnienie stoją obok nazwy, dopóki mieści się najszersza możliwa treść: obroty czterocyfrowe, „wypełnienie 100 % → 100 %”, najdłuższa nazwa i podtytuł na stronie, mierzone w języku i czcionce aplikacji; inaczej schodzą pod nazwę, obok siebie (wypełnienie zaczyna się za miejscem na najszersze obroty, więc nie przesuwa się przy zmianie liczby cyfr), a w najwęższym oknie jedno pod drugim; (3) dwie kolumny, dopóki w połowie szerokości mieści się nagłówek z wartościami pod nazwą, inaczej jedna; (4) wszystkie karty strony mają ten sam układ, rząd jest wysoki jak wyższa karta, a ramki trybu w rzędzie mają tę samą wysokość (D60, pkt 5), więc wykresy i przyciski stoją równo; (5) karta z wykresem ma stałą wysokość: notka „W automacie kontroler zmienia wypełnienie stopniowo.” ma zawsze miejsce na tyle wierszy, ile zajmuje przy danej szerokości, i tylko się pokazuje albo znika; zajmuje całą szerokość obok przycisku „Szczegóły krzywej”, który stoi zawsze w tym samym miejscu. Granice układów nie są stałymi progami szerokości, wynikają z pomiaru treści. Własne widżety nagłówka karty i siatki kart; siatka paruje ramki trybu po zmianie liczby kolumn, już poza układaniem, bo GTK zachowuje zmierzone rozmiary. W legendzie „przedłużenie krzywej” (po angielsku „curve extension”, z komentarzem dla tłumaczy) zamiast „krzywa poza skrajnymi punktami” (D60, pkt 2) | §7 M2 (M2.4) |

**D8: zakres M1 (2026-11-10).**
- Strona System tylko do odczytu jest w M1: płyta, BIOS, procesor, system, pamięć z bazy udev, karta graficzna z VBIOS, dyski z firmware, stan `nct6683`, konfiguracja lm-sensors, programy mogące konfliktować. Restart do UEFI i profile energii zostają w M4.
- „Kopiuj informacje o systemie” jest od M1, bez profilu energii (M4) i wersji firmware RGB (M3); te pola dochodzą w swoich etapach.
- Producent modułu pamięci przy „Unknown” w SMBIOS: surowy identyfikator JEDEC obok numeru części, bez własnej tablicy nazw (O14). Od D37 identyfikator jest tylko w tekście do schowka.
- Czujniki `spd5118` w Przeglądzie nazywamy gniazdem z bazy udev (np. „DIMMA2”), gdy adres SPD 0x50+*i* jednoznacznie odpowiada gniazdu *i*; w przeciwnym razie „Moduł pamięci *N*”.
- Nazwy urządzeń PCI (karta graficzna, karty sieciowe) z bazy sprzętu udev (hwdb), a nie z własnego parsera `pci.ids`. Zmienia to sposób z D6, nie wynik.

**D1: sprawdzenie nazwy (2026-11-06).**
- Flathub: brak aplikacji o nazwie Baseboard i brak App ID z tym słowem.
- Apps for GNOME, GNOME Circle, gitlab.gnome.org: brak.
- Pakiety `baseboard` w Fedorze 45, Arch, AUR i Debianie: brak.
- GitHub: tylko małe projekty z innych dziedzin, np. webowa konsola BMC dla serwerów i panel do Basecampa. Żaden nie jest aplikacją desktopową.
- Słabość: „baseboard” to też ogólny termin sprzętowy (tabela SMBIOS, BMC w IPMI), więc nazwa gorzej się wyszukuje.
- Znak towarowy: niesprawdzony, bo TMview i wyszukiwarka USPTO były niedostępne. Przed pierwszym publicznym wydaniem trzeba to sprawdzić ręcznie.
- Ponownie 2026-11-11, przed publikacją repozytorium (bez baz znaków towarowych): Flathub, Apps for GNOME i GNOME Circle, pakiety Fedory, Debiana, Arch i AUR oraz crates.io bez oprogramowania o tej nazwie (App ID i crate `baseboard` wolne). Na GitHubie repozytoria z gwiazdkami to projekty płytek drukowanych („baseboard” w znaczeniu elektronicznym); najbliższe oprogramowanie to mała webowa konsola do zarządzania serwerami przez BMC. Ryzyko kolizji niskie; słabość jak wyżej: nazwa źle się wyszukuje.

Odrzucone propozycje nazw: Bellows, Fanfare, Vane, Northbridge.

**D6: ustalenia z przeglądu makiet.**
- Nazwy odczytów domyślnie z profilu płyty i z nazw wbudowanych, tłumaczone przez gettext. Nazwa złącza stoi w podtytule. Etykiety z lm-sensors to opcja w ustawieniach (§5.1, §6.2).
- Polska typografia liczb i jednostek w całej aplikacji (§5.1).
- Wiersze nie powtarzają nazwy urządzenia z opisu grupy, np. przy karcie graficznej (§5.1).
- Nazwy wbudowane: `amdgpu` edge → „Układ graficzny”, `k10temp` Tccd*N* → „Chiplet CCD*N*”. Nazwa karty graficznej z `product_name`, a gdy go brak, z `pci.ids` (§5.1).
- Przegląd w dwóch kolumnach przy szerokim oknie. Minimum i maksimum to szary zakres obok wartości (§5.1).
- Tryb wentylatora wybiera się przełącznikiem segmentowym w karcie złącza. Edytor krzywej leży na osobnej podstronie, a zmiany wymagają „Zastosuj” (§5.5).
- „Przywróć automat BIOS” jest tylko w banerze, widocznym wyłącznie przy przejętym kanale (§5.5).
- EPP i tryb sterownika opisowo, surowa wartość w podtytule (§5.3).
- Komunikaty o braku sterowania wentylatorami nie podają numeru wersji jądra (§5.4, §5.5).

**D38: poprawki interfejsu przed zamknięciem M1 (2026-11-11).**
- Preferencje: interwał odświeżania z jednostką („1 s”); wiersz „Własne nazwy” w jednej linii, z przyciskiem otwierającym plik w domyślnej aplikacji, a bez pliku jego katalog (§3.2).
- Te same zasady odczytów w Przeglądzie i w preferencjach (§5.1):
  - dyski w kolejności numerów urządzeń, także na stronie System;
  - VSB → „Zasilanie czuwania (VSB)”, VBAT → „Bateria podtrzymująca (VBAT)”; AVSB, VTT i VREF zostają skrótami, bo ich znaczenie nie jest pewne;
  - podtytuł nie powtarza tytułu;
  - nazwa układu w podtytule bez końcówek, według ogólnej zasady, a nie listy wyjątków;
  - „złącze nietestowane” zamiast „przypisanie niesprawdzone”, w jednej linii podtytułu;
  - napięcia bez tolerancji bez dopisku „bez oceny”, z podtytułem źródła;
  - bez zakresu, dopóki minimum i maksimum wyglądają tak samo (doprecyzowanie D6).
- Strona System (§5.4):
  - krótka nazwa procesora wszędzie, z jednej funkcji (Przegląd, strona System, tekst do schowka);
  - wiersz profilu płyty: stan w tytule, plik albo przyczyna w podtytule;
  - „Sterownik częstotliwości procesora” zamiast „Sterownik cpufreq”;
  - czcionka o stałej szerokości tylko dla ścieżek i poleceń;
  - kod płyty zawsze jako „MS-7E61”.
- Puste złącza wentylatorów zostają domyślnie włączone w Przeglądzie (bez zmian).

---

## 13. Otwarte pytania techniczne

| # | Pytanie | Jak sprawdzić | Potrzebne w |
|---|---|---|---|
| O1 | Które złącza RGB fizycznie są na płycie i jakim strefom raportu 0x50 odpowiadają | instrukcja płyty + test z urządzeniem ARGB | M3 |
| O2 | Czy stan RGB z RAM (store = 0) przeżywa ciepły restart | test | M3 |
| O3 | Przypisanie PUMP_SYS1 → fan2/pwm2 i JAF_1 → fan4/pwm4. Oba złącza są na MS-7E61 puste, więc do testu pompa zostaje bez sterowania (D52) | wentylator podłączany kolejno do obu złączy | M1 (etykieta), M2b (sterowanie) |
| O4 | ~~Zachowanie `pwmN_enable` i wypełnienia po uśpieniu i po ciepłym restarcie~~. **Częściowo rozstrzygnięte (testy sterownika, 2026-11-09):** po S3 BIOS cofa tryb ręczny (EC w automacie); sterownik z `7dc9dff9608d` go odtwarza. Krzywa zapisana przez v1 jest po S3 taka jak przed uśpieniem, ale na tej płycie nie da się rozróżnić, czy zachował ją EC, czy odtworzył sterownik. Po restarcie EC ma ustawienia BIOS-u. Otwarte: czy BIOS przy S3 nadpisuje krzywe; S4 (hibernacja). | test S4 za zgodą | M2b |
| O5 | Zapis na CPU_FAN1 i PUMP_SYS1, w trybie ręcznym i w krzywej (dotąd sprawdzono tylko SYS_FAN1); dla kanałów 0–1 otwarte jest też uzgadnianie w sterowniku (notatki z projektu sterownika) | CPU_FAN1: test według procedury z §7 M2, M2.5 (D53), uruchamia właściciel; PUMP_SYS1: po podłączeniu wentylatora albo pompy (D52) | M2b (D16) |
| O6 | Jak powiadamiać o zdarzeniach awaryjnych, gdy aplikacja nie działa (np. mały proces w tle z autostartem albo portal Background). W M2 bez procesu w tle (D54): blokada awaryjna jest trwała w `/run`, a aplikacja pokazuje ją po uruchomieniu | prototyp | po M2 |
| O7 | Rzeczywiste umiejscowienie czujników „System” i „Chipset A” | długi odczyt z dysku podłączonego do chipsetu, albo odczyt konfiguracji kanałów przez sterownik | M1 (opis), M2b (wybór źródeł) |
| O8 | Czy pomocnik może działać z pustym `CapabilityBoundingSet` | test jednostki systemd | M2b |
| O9 | Progi awaryjne (B4) i minima (B5) | testy obciążeniowe, dane pompy, jeśli zostanie podłączona | M2b |
| O10 | Czy temp1 układu płyty (źródło progu CPU, D34) nadąża pod obciążeniem za Tctl z `k10temp` (opóźnienie i różnica) i czy osiąga próg 95 °C. Próg CPU równa się Tjmax (D5), a procesor dławi przy Tjmax, więc temp1 może zatrzymać się na progu albo poniżej i warunek „przekroczy” mógłby nigdy nie zadziałać. Dlatego warunek B4 dla CPU to już „osiągnie” (D48). Tctl służy tu tylko do porównania. W M1 (krok 13) przy krzywej BIOS-u temp1 doszło do 92,0 °C, a Tctl do 92,1 °C po 12 s pełnego obciążenia. | test obciążeniowy z wentylatorem CPU na minimum, z zapisem temp1 i Tctl co 1 s, według procedury z §7 M2, M2.5 (D53) | M2b |
| O11 | Lista wydań BIOS-u: dokładna ścieżka na serwerze, pola, które czytamy (płyta, wersja, data, adres pliku), zapis wersji względem DMI `bios_version`. Zapis i znaczenie warunków dopasowania (zakres wersji, wersje pominięte, wykluczenia). Czy lista podaje sumę kontrolną i jakiego rodzaju, a jeśli nie, skąd wziąć sumę wzorcową. | notatki z analizy oprogramowania producenta (§2) oraz jednorazowe pobranie listy, za zgodą | M5 |
| O12 | Wymagania wbudowanego narzędzia aktualizacji w UEFI: tablica partycji (MBR czy GPT), FAT32 a rozmiar nośnika, położenie i nazwa pliku, czy plik trzeba rozpakować z archiwum | instrukcja płyty i test z pendrivem, bez zatwierdzania flashowania | M5 |
| O13 | Odczyt wersji firmware kontrolera RGB na MS-7E61: czy kontroler odpowiada, format odpowiedzi, czy pole `bcdDevice` urządzenia USB (dziś 0001) ma z nią związek | test na sprzęcie (tylko odczyt) | M3 |
| O14 | ~~Nazwa producenta modułu pamięci, gdy SMBIOS podaje „Unknown”~~. **Rozstrzygnięte (D8):** hwdata nie ma tablicy nazw JEDEC; pokazujemy surowy identyfikator (na tej maszynie „Bank 8, Hex 0x5D”) obok numeru części; od D37 tylko w tekście do schowka. | – | M1 |
| O15 | Tempo dochodzenia wyjścia do celu w automacie na pozostałych kanałach i przy innych czasach Step Up/Down. **Częściowo rozstrzygnięte (2026-11-11, M2a krok 2, obciążenie bez żadnego zapisu):** SYS Fan 1 i 2 ok. 5 jednostek/s, CPU Fan 1 ok. 3,3 jednostki/s (ze 124 do 255 w ok. 40 s), w obu kierunkach. Otwarte: pompa (złącze puste), SYS 3 i 4 (ich krzywe są płaskie, więc obciążenie nic nie zmienia), inne ustawienia Step Up/Down. | odczyty `pwmN` co sekundę przy obciążeniu, bez dodatkowych zapisów | M2b (D14) |
| O16 | Ostateczny kształt ABI zapisu krzywych po recenzji v1 i łatki 5 | lista linux-hwmon | M2c, M2d |
| O17 | Czy krzywa zapisana przez v1 przetrwa hibernację (S4) i ciepły restart przez kexec | test za zgodą | M2c |
| O18 | RGB z Flatpaka: hidraw jest dostępny tylko przez `--device=all`, które odsłania też `/dev/port` i inne urządzenia. Czy RGB przenieść do pomocnika (§3.4), czy wymagać instalacji natywnej, czy przyjąć `--device=all` | decyzja przed M3 | M3 |
| O19 | Czy polkit z `auth_admin_keep` (D3) działa dla wywołań z Flatpaka przez pośrednika D-Bus tak jak dla aplikacji natywnej (jedno hasło na kilka minut); zapamiętanie autoryzacji może dotyczyć procesu pośrednika | test w kroku 13: natywnie i we Flatpaku, ile razy w ciągu 5 minut pojawia się pytanie o hasło (§7 M2, M2.6) | M2b |
