# Projekt: natywna aplikacja GNOME do monitoringu i sterowania płytą główną

Specyfikacja: jak aplikacja działa dziś i co jest w planie. Decyzje z datami są w `docs/DECYZJE.md`, a przebieg prac w historii gita.

Aplikacja nazywa się **Baseboard** (D1): App ID `me.bojanowski.Baseboard`, pliki binarne `baseboard` i `baseboard-helper`, pakiet i domena gettext `baseboard`.

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
| krzywa w EC | 7 punktów (temperatura, wypełnienie) w EC. EC sam liczy z nich wypełnienie dla temperatury źródła, bez pomocnika; edycja krzywych w M2b (wydanie 0.4.0). |
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
| Wentylatory | podgląd krzywych EC (M2a), tryb ręczny i edycja krzywych EC (M2b), wybór źródła temperatury, gdy sterownik pozwoli (M2c) | sysfs hwmon `nct6683`; zapis przez pomocnika |

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
- Znaki towarowe (D58): nazwa aplikacji i ikona nie używają nazw ani oznaczeń producenta. Nazwa producenta pada tylko opisowo w opisie projektu („do płyt głównych MSI”: strona, README, metainfo, plik `.desktop`, okno „O programie”, opis repozytorium), z zastrzeżeniem o znakach towarowych tam, gdzie opis ma na nie miejsce. W pozostałych tekstach interfejsu nie, poza modelem płyty, który pokazujemy tak, jak podaje go profil albo DMI, wyłącznie jako informację o sprzęcie.

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
- **Ustawienia (GSettings):** interwał odświeżania, źródło nazw odczytów (nazwy aplikacji albo etykiety z lm-sensors, §5.1), ukryte odczyty i złącza, ostatnia strona, sprawdzanie aktualizacji BIOS-u w sieci (domyślnie wyłączone, M5), ostatnio widziana wersja BIOS-u (do listy kontrolnej, §5.6). Są klucze `refresh-interval` (1–10 s, domyślnie 1), `sensors-labels` (domyślnie wyłączone) i `hidden-readings` (lista kluczy odczytów `układ/kanał@urządzenie`, np. `nct6687/fan2@devices/platform/nct6683.2592`; ścieżka urządzenia zamiast `hwmonN`, więc klucz przeżywa restart). Do tego `shown-readings`: klucze odczytów domyślnie ukrytych, które użytkownik włączył (D42). Preferencje (`AdwPreferencesDialog`, Ctrl+,; jedna strona z podstroną „Widoczne odczyty”, D64) zapisują każdą zmianę od razu, a okno aplikacji słucha GSettings i stosuje ją bez restartu: nowy interwał i źródło nazw dostaje wątek odczytu z natychmiastową próbką, ukryte odczyty znikają z bieżącej próbki. Interwał stoi z jednostką („1 s”; wpisać można „5” albo „5 s”). Wiersz „Własne nazwy” ma podtytuł o restarcie, pełną ścieżkę pliku w podpowiedzi i przycisk, który otwiera plik w domyślnej aplikacji, a gdy pliku jeszcze nie ma, jego katalog, tworzony w razie potrzeby (D38, D64). Ostatnia strona dochodzi później.
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
| `SetEcSource` | `(s key, s source) → s` | źródło temperatury krzywej; tylko przy `source-write` (M2c, D17) | control |
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
Cargo.toml, Cargo.lock      workspace, linty (§8) i zablokowane wersje zależności
clippy.toml                 próg długości funkcji (§8)
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
tests/tour.sh               przejście przez całą aplikację na niewidocznym ekranie (test meson `tour`, §8)
tests/core_api.py           publiczne API core bez martwych elementów, z listą wyjątków core-api-exceptions.txt (§8)
tests/resources.py          zasoby i style bez sierot (§8)
tests/translations.sh       tłumaczenia kompletne, bez fuzzy i przestarzałych wpisów (§8)
tests/work_status.py        „Stan prac” krótki, decyzje poza specyfikacją (§8)
docs/                       PROJEKT.md (specyfikacja i plan), DECYZJE.md (decyzje z datami)
makiety/                    makiety interfejsu: HTML, OPIS.md, renderuj.sh (PNG w png/, poza repozytorium)
website/                    strona projektu (D41): HTML i CSS bez skryptów, angielska pod /, polska pod /pl/, zrzuty strony w img/,
                            zrzuty metainfo w screenshots/<wersja>/ (stałe adresy, nie zmieniamy ich, D43)
.github/workflows/          website.yml: wdrożenie website/ na GitHub Pages po zmianach na main (D41);
                            ci.yml: sprawdzenia z §8 przy każdym pushu i PR na main (D68)
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
| B4 | **Próg awaryjny** (D48). Gdy temperatura z listy progów przekroczy próg w 3 kolejnych odczytach (temperatura procesora: osiągnie go, ≥ 95 °C, bo procesor przy Tjmax dławi się i temp1 może stanąć dokładnie na progu), pomocnik od razu ustawia **wszystkie** kanały w trybie ręcznym ustawionym przez Baseboard na 100 % (tryb ręczny, 255) i włącza blokadę zapisaną w `/run/baseboard/emergency`, która trwa po wyjściu i restarcie pomocnika. Do jej zdjęcia trzeba jawnej akcji (`AcknowledgeEmergency`), a temperatura musi spaść o co najmniej 10 °C poniżej progu; po zdjęciu te kanały przechodzą w automat. Wartość nieodczytywalna lub niewiarygodna (< 0 °C, > 125 °C, brak pliku) przez 3 odczyty działa jak przekroczenie progu. Kanały z krzywą w EC już są w automacie: krzywe procesora i pompy osiągają 100 % najpóźniej na progu (B14), krzywe wentylatorów systemowych nie muszą (D19), a B4 ich nie zmienia. Pełne wypełnienie od razu, a nie automat, bo w automacie wyjście dochodzi do celu stopniowo (pomiar wyżej: kilkadziesiąt sekund do 100 %; pompa: O15). |
| B5 | **Minimalne wypełnienie** w jednostkach 0–255, przeliczone z procentów z obcięciem jak w BIOS-ie (§3.5, D20): CPU co najmniej 30 % (76), pompa co najmniej 50 % (127) (§6.3). Dotyczy trybu ręcznego i **każdego punktu krzywej**. Za niskie żądanie trybu ręcznego pomocnik podnosi do minimum i zwraca faktycznie ustawioną wartość; krzywą odrzuca (B7). W kodzie są stałe dolne granice, przeliczone tak samo (CPU 20 % = 51, pompa 40 % = 102). Profil może je tylko podnieść, więc nawet błędny profil nie zejdzie niżej. Sterownik minimum nie ma, więc to jedyna ochrona. |
| B6 | **Pompa nigdy nie staje.** Żądanie 0 dla roli `pump`, w trybie ręcznym albo w punkcie krzywej, kończy się błędem `InvalidArgument`, a nie podniesieniem do minimum, żeby interfejs nie sugerował, że pompę można zatrzymać. Interfejs nie ma też przełącznika „zatrzymaj”. Złącze pompy traktujemy jak pompę, także gdy podłączono do niego zwykły wentylator. |
| B7 | **Walidacja wejścia.** Tylko klucze kanałów z profilu, które mają potrzebną możliwość (§5.5.2). Krzywa w EC ma dokładnie tyle punktów, ile podaje sterownik (dziś 7); temperatury niemalejące (D31) w zakresie 0–100 °C, wypełnienie niemalejące, każdy punkt co najmniej minimum B5. Pomocnik **nie polega** na przycinaniu przez sterownik: krzywą spoza reguł odrzuca (`InvalidArgument`). Źródło temperatury tylko z listy temperatur EC w profilu (M2c). **Wyjątek:** krzywa pierwotna (B18) i powrót w B17(b) nie przechodzą tej walidacji. |
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

**Wygląd wszystkich stron (D63):** nowoczesny minimalizm, bez „geek factor”, jak w aplikacjach dla zwykłego użytkownika, nie w panelu diagnostycznym. Na pierwszym planie zwykłe słowa i najważniejsze liczby; oznaczenia sterowników, czujników i złączy, ścieżki plików i wartości pomocnicze o jedno kliknięcie dalej; mało ramek, dużo powietrza, jeden kolor akcentu, układy libadwaita jak w Ustawieniach GNOME. Nic nie ginie: wszystko, co strona pokazuje dziś, da się nadal znaleźć i skopiować do zgłoszenia błędu („Kopiuj informacje”). Ostrzeżenia i informacje o bezpieczeństwie zostają, krótko i prostym językiem. Opisy stron poniżej sprzed D63 obowiązują, dopóki strona nie dostanie projektu w tym stylu (Wentylatory: D62; Przegląd, System, okno i Preferencje: D64). Okno (D64): tytuł strony bez podtytułu (bez „Odświeżanie co 1 s”), w stopce paska bocznego sama nazwa płyty; przyciski okna i ikony paska bocznego bez zmian.

**Szerokość wszystkich stron (D67):** treść rośnie z oknem do 1320 px (`AdwClamp`), a układ zmienia się na tych samych progach szerokości strony na każdej stronie i podstronie, zapisanych w jednym miejscu (`app::layout`, punkty przerwania `AdwBreakpointBin`): do 599 sp wąsko, wszystko w jednej kolumnie; 600–879 sp (okno ok. 1024 px) jedna karta w rzędzie, z liczbami obok historii; 880–1099 sp (ok. 1200 px) gęściej, karty i grupy obok siebie, mniejsze liczby; od 1100 sp szeroko. Przy progu wąskości okno zwija też pasek boczny, a przycisk „Kopiuj informacje” pokazuje samą ikonę.

### 5.1 Przegląd (M1)

- **Strona (D67, „karta na każdy element komputera”):** od góry:
  - zdanie stanu (reguła niżej) w wierszu na całą szerokość: ikona, tytuł i zdanie pod nim; przy problemie w kolorze ostrzeżeń, ze strzałką, i otwiera to, o czym mówi (stojący wentylator stronę Wentylatory, temperatura jej grupę „Wszystkich czujników”, napięcie grupę Napięcia);
  - karty Procesor (pierwsza temperatura grupy Procesor, na AMD Tctl), Karta graficzna (pierwsza temperatura pierwszej karty) i Płyta (temperatura systemowa z profilu); tylko te, które są, bez pustych miejsc. Karta ma ikonę części w kółku, temperaturę dużą cyfrą w pełnych stopniach, historię ostatnich 5 minut z podpisem „Ostatnie 5 minut” i zakresem („63–70 °C”), jedną liczbę części („Wentylator procesora” z obrotami albo „Stoi” w kolorze ostrzeżeń; „Pobór mocy” karty; „Zasilanie”: „W normie”, szyna poza normą z odczytem („+12 V: 11,20 V”, w kolorze ostrzeżeń) albo liczba napięć poza normą; bez szyn z oceną tego wiersza nie ma) i „Najwyższa od uruchomienia”. Temperatura blisko progu: ikona ostrzeżenia zamiast ikony części, liczba i historia w kolorze ostrzeżeń. Karta (kliknięcie albo Enter) otwiera „Wszystkie czujniki” na swojej grupie;
  - kafelki Wentylatory (ile się kręci, pod tym zakres obrotów „Od 750 do 1010 rpm”, który łamie się tylko przed wyższą liczbą, „Przyspieszają” albo „Wentylator procesora stoi”; słupki obrotów każdego wentylatora, razem zawsze tej samej szerokości; przy stojącym wentylatorze procesora cały kafelek w kolorze ostrzeżeń; otwiera stronę Wentylatory), Pamięć i Dyski (najcieplejszy moduł i dysk, „Najcieplejszy z 2 modułów”; otwierają swoją grupę);
  - wiersz „Wszystkie czujniki”: każdy odczyt ze źródłem i zakresem od uruchomienia.

  Szeroko i gęściej karty i kafelki stoją po trzy w rzędzie, przy ok. 1024 px po jednym w rzędzie (karta: liczba i fakty po lewej, w kolumnie co najmniej 210 px i tak szerokiej jak podpisy faktów w jednym wierszu, historia po prawej), w wąskim oknie jedno pod drugim.
- **Liczby na kartach i kafelkach (D67)** Przeglądu i na karcie strony Wentylatory są całkowite, jak duża temperatura („12 W”, „32 °C”, `core::units::whole`); dokładne wartości są we „Wszystkich czujnikach”. Wyjątek: szyna poza normą na karcie Płyta („+12 V: 11,20 V”), bo „11 V” nie mówi, o ile odbiega. Jednostka nigdy nie odrywa się od liczby (twarda spacja, §5.1 „Liczby i jednostki”).
- **Bez wielokropków (D67):** żaden tekst aplikacji nie kończy się wielokropkiem. Gdy brakuje miejsca, tekst zawija się do następnego wiersza (nazwy kart i wentylatorów, podtytuły wierszy wentylatorów, stopka paska bocznego) albo dostaje krótszą wersję (tytuł „Szczegóły techniczne” w wąskim oknie: „Szczegóły”). Zakres we „Wszystkich czujnikach” ma miejsce na najszerszy zakres (15 znaków).
- **Zdanie stanu (D67)**, ta sama reguła dla wiersza stanu strony System (`core::summary::state`): problemy w kolejności ważności: stojący wentylator procesora (D65); temperatura blisko progu; napięcie poza tolerancją (D64). Przy kilku najważniejszy jest w tytule, a pod jego zdaniem „I jeszcze 2 problemy”. Bez problemów „Wszystko w normie” („Temperatury, wentylatory i zasilanie bez zastrzeżeń.”, po angielsku „Temperatures, fans and power are normal.”, a gdy wentylatory przyspieszają, „Wentylatory przyspieszają.”), na płycie bez profilu „Odczyty działają” („Ta płyta nie ma jeszcze profilu, więc część odczytów jest bez nazw i bez oceny.”). Teksty problemów:
  - procesor: „Procesor jest bardzo gorący: 93 °C” i „Przy 95 °C zaczyna zwalniać, żeby się nie przegrzać.” (po angielsku „At 95 °C it slows down to avoid overheating.”), dalej zdanie o wentylatorach, tylko prawdziwe (`Summary::fans_now`): „Wentylatory pracują na pełnych obrotach.”, gdy wentylator procesora ma pełne wypełnienie i żaden wentylator już nie przyspiesza; „Wentylatory przyspieszają.”, dopóki któryś przyspiesza; bez znanego wypełnienia wentylatora procesora albo przy stałym niepełnym bez tego zdania; inny odczyt: „Bardzo gorąco: Composite, 87 °C” i „Granica to 94 °C.”;
  - napięcie: „Napięcie +12 V poniżej normy: 11,20 V” i „Norma to 11,40–12,60 V. Odczyt z płyty bywa niedokładny; jeśli komputer jest niestabilny, warto sprawdzić zasilacz.” (bez stanowczego „Sprawdź zasilacz”, bo odczyt z płyty bywa niedokładny);
  - stojący wentylator: „Wentylator procesora stoi” i „Kręcił się wcześniej, a od co najmniej 10 s ma 0 rpm.”.
- **Temperatura blisko progu (D67, `core::heat`):** progi ze sterowników (`tempN_crit` każdego układu, gdy wynosi 20–150 °C) i z profilu (`emergency_c`, który dla układu płyty ma pierwszeństwo); sprawdzane są wszystkie odczyty z progiem, także ukryte. Odczyt jest blisko, gdy w 3 kolejnych próbkach ma 5 °C lub mniej do progu, i przestaje być, gdy spadnie co najmniej 3 °C poniżej tej granicy. Kilka odczytów: najbliższy progu pierwszy. Karta jest „gorąca”, gdy blisko progu jest odczyt jej grupy (na MS-7E61 próg 95 °C ma czujnik procesora na płycie, a karta pokazuje Tctl).
- **Wentylatory przyspieszają (D67):** gdy wypełnienie któregoś złącza wzrosło o co najmniej 5 punktów procentowych w ciągu ostatnich 10 s (ten sam próg co „Przyspiesza” na liście wentylatorów, `core::summary::SpeedWatch`); Przegląd nie czyta krzywych, bo każdy odczyt idzie do kontrolera płyty.
- **Historia (D67, `core::history`):** ostatnie 5 minut każdej temperatury, zbierane od uruchomienia aplikacji w pamięci, bez zapisu na dysk. Wykres (`app::history_chart`) rysuje się tylko przy nowej próbce; przy ukrytym oknie próbek nie ma, a w linii zostaje przerwa (luka dłuższa niż trzy zwykłe odstępy). Skala co najmniej 12 °C, punkt „teraz” na końcu linii. Ten sam widżet karty (`app::part_card`) jest na Przeglądzie i na stronie Wentylatory. Czytnik ekranu: karta jako „Procesor: 68 °C” z opisem „Ostatnie 5 minut: 63–70 °C. Teraz 68 °C, rośnie.” (rośnie, spada albo bez zmian: zmiana o więcej niż 1 °C w ostatniej minucie) i faktami karty; kafelek jako „Wentylatory: 5 kręci się” z zakresem w opisie.

  „Wszystkie czujniki” to podstrona (jak „Szczegóły krzywej”) z dotychczasową listą grup, opisaną niżej, łącznie z grupą Wentylatory (w trybie ogólnym strona Wentylatory nie ma listy). Linii „Szary zakres: …” nad odczytami nie ma; zakres ma podpowiedź „Najniższa i najwyższa wartość od uruchomienia aplikacji”, a czytnik ekranu dostaje go w opisie wiersza.
- **Stojący wentylator procesora (D65):** ostrzeżenie „Wentylator procesora stoi”, gdy jednocześnie: złącze wentylatora procesora jest sprawdzone w profilu płyty, wentylator kręcił się wcześniej od uruchomienia aplikacji, wypełnienie jest powyżej zera, a obroty wynoszą 0 przez co najmniej 10 s. Puste złącze od startu (nigdy się nie kręciło) i wentylator zatrzymany przez krzywą (0 %) nie wywołują alarmu. Przerwa między próbkami dłuższa niż 20 s (okno ukryte) liczy czas od nowa. Działa także przy ukrytym wierszu wentylatora. Ostrzeżenie stoi w zdaniu stanu, na kafelku Wentylatory i na karcie Procesor („Stoi”) Przeglądu, nad listą strony Wentylatory (z opisem „Kręcił się wcześniej, a od co najmniej 10 s ma 0 rpm.” i stanem „Stoi” w wierszu wentylatora) i w wierszu stanu strony System. Model i licznik czasu: `core::summary`.
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
- **Ukryte odczyty i złącza:** w preferencjach podstrona „Widoczne odczyty” (D64; wcześniej osobna strona „Odczyty”) ma przełącznik dla każdego odczytu, w grupach jak na Przeglądzie, z nazwą i wartością z chwili otwarcia (np. 0 rpm przy złączu bez wentylatora), bez sterownika i czujnika w podtytule; wiersz w Preferencjach ma podtytuł „Wybierz, które odczyty pokazuje Baseboard”, a podstrona opis „Wyłączone odczyty nie są pokazywane na żadnej stronie…” (D66). Wyłączony odczyt znika od razu z Przeglądu i ze strony Wentylatory, a grupa bez odczytów znika cała; ostrzeżenie o stojącym wentylatorze procesora zostaje (D65). Gdy ukryte są wszystkie, strona mówi o tym i odsyła do preferencji. Odczyty ukryte w profilu (`[hidden]`) i przez `ignore` z lm-sensors nie trafiają na listę. Numerowane czujniki temperatury dysków NVMe (`temp2`–`temp9` sterownika `nvme`) są domyślnie ukryte (D42): na liście mają wyłączony przełącznik, włączone trafiają do klucza `shown-readings`, a wyłączenie zawsze wygrywa. Temperatura złożona dysku (`temp1`) jest widoczna jak każdy inny odczyt.
- **Napięcia:** wartość i ocena względem tolerancji z profilu (np. ±5 % dla +12 V, +5 V i +3,3 V według ATX). Ocena to ikona i tekst, np. „W tolerancji ±5 % (+0,8 %)”, a nie sam kolor. Napięcia bez tolerancji w profilu (rdzenie, SoC, VDDIO) nie mają oceny: podtytuł pokazuje źródło, jak przy temperaturach, a miejsce na ikonę zostaje puste, żeby wartości stały w jednej linii (D38).
- **Odświeżanie:** domyślnie co 1 s (w preferencjach 1–10 s), wstrzymane, gdy okno jest niewidoczne. Minimum i maksimum od uruchomienia stoją jako szary zakres obok bieżącej wartości.
- **Układ „Wszystkich czujników”:** przy szerokim oknie grupy stoją w dwóch kolumnach, przy wąskim w jednej, w kolejności: Procesor, Płyta, Wentylatory, Napięcia, Karta graficzna, Dyski, Pamięć, Inne. Otwarta na grupie podstrona przewija się do niej i przenosi fokus klawiatury do jej pierwszego wiersza.
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
  - sterownik `nct6683`: załadowany czy nie (jest na liście `/proc/modules` albo jest urządzenie hwmon z profilu), moduł z jądra czy spoza niego (D40: flaga `O` w nawiasie na końcu linii modułu w `/proc/modules`; linia bez flagi `O` to moduł z jądra; brak modułu na liście przy obecnym urządzeniu to sterownik wbudowany w jądro, czyli też z jądra; gdy listy nie da się odczytać, „pochodzenie modułu nieznane”, nigdy zgadywanie; lista jest czytana przy każdym odczycie strony, więc po przeładowaniu modułu pokazuje stan bieżący; tylko informacja, poziomu obsługi z tego nie wyliczamy, §5.5.2), poziom obsługi wentylatorów (`DriverLevel`, §5.5.2). Gdy jądro nie pozwala sterować wentylatorami, wiersz „Sterowanie wentylatorami niedostępne” ma to samo zdanie co strona Wentylatory i wiersz stanu: „Ten system na razie tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.”, bez numeru wersji jądra (D41, D60, D66), jako informacja bez ikony ostrzeżenia; poziomy ze sterowaniem: „Tryb ręczny”, „Tryb ręczny i podgląd krzywych”, „Krzywe w kontrolerze płyty”, „Krzywe w kontrolerze płyty z wyborem temperatury”; we Flatpaku bez konfiguracji systemu wiersz biblioteki mówi „Wersja 3.6.2: odczyty bez konfiguracji lm-sensors”, a powód (Flatpak) podaje tylko wiersz konfiguracji lm-sensors,
  - kontroler RGB: obecny czy nie, czy jest reguła udev, wersja firmware (od M3, §5.2),
  - konfiguracja lm-sensors dla płyty: pliki, które czyta libsensors (`/etc/sensors3.conf`, zapasowo `/etc/sensors.conf`, oraz `/etc/sensors.d/*` bez plików ukrytych), z instrukcją `chip` dla układu płyty (wzorzec, którego część przed pierwszym „-” to `[hwmon] name` z profilu, np. `nct6687-isa-*`),
  - działające programy, które mogą konfliktować przy sterowaniu wentylatorami: `fancontrol`, `coolercontrold`, `fan2go` i `thinkfan`, rozpoznawane po nazwie procesu (`/proc/<pid>/comm`, bez linii poleceń).

  Numerów seryjnych nie czytamy ani nie pokazujemy: płyty i systemu (wymagają roota), modułów pamięci (w bazie udev są dostępne bez roota) i dysków. Tak samo adresów MAC (§1.3).
- **Strona (D64, D67):** na górze, na całą szerokość, jeden wiersz stanu słowami zdania stanu Przeglądu (§5.1), który otwiera „Diagnostykę”: „Wszystko w normie” („Czujniki, sterownik i konfiguracja w porządku”, ikona OK); „Odczyty działają” z informacją, gdy system tylko odczytuje wentylatory („Ten system na razie tylko odczytuje wentylatory…”) albo gdy płyta nie ma profilu („Brak profilu dla tej płyty”); przy problemie w kolorze ostrzeżeń najważniejszy problem jako tytuł, a pod nim „I jeszcze 1 problem”, gdy jest ich więcej. Problemy: te ze zdania stanu Przeglądu (stojący wentylator procesora, temperatura blisko progu, napięcie poza tolerancją), potem brak sterownika albo układu płyty, niesprawdzona rewizja PCB, inne programy sterujące wentylatorami, pominięte pliki. Pod nim krótka lista jak „O systemie” w Ustawieniach GNOME, nazwa i wartość obok siebie: „Ten komputer” (płyta, BIOS z datą, procesor, cała pamięć w jednej linii, np. „32,0 GiB DDR5 · 6000 MT/s”, karta graficzna; każdy wiersz z własną ikoną części w kółku, jak na kartach Przeglądu, BIOS jako sześcian), „Dyski” (model i rozmiar), „Sieć” (nazwy kart), „System”, i wiersz „Szczegóły techniczne”. W szerokim i gęstszym oknie (§5) dwie kolumny: „Ten komputer” i „System” po lewej, „Dyski”, „Sieć” i „Szczegóły techniczne” po prawej; węziej jedna, w tej kolejności: Ten komputer, Dyski, Sieć, System, Szczegóły techniczne. Dwie podstrony z przyciskiem „Kopiuj informacje”: „Diagnostyka” (ostrzeżenia odczytów, Czujniki i wentylatory, Profil płyty, Pominięte pliki) i „Szczegóły techniczne” (grupy niżej bez Czujników i wentylatorów); w szerokim oknie ich grupy stoją w dwóch kolumnach, każda w kolumnie, która ma dotąd mniej wierszy, a w jednej kolumnie w kolejności niżej. Każdy wiersz strony sprzed D64 jest w jednej z podstron; „Kopiuj informacje” kopiuje ten sam tekst co wcześniej. W „Diagnostyce” każdy wiersz grupy Czujniki i wentylatory i wiersz profilu płyty ma ikonę: OK, ostrzeżenie albo informację (gdy nie jest ani jednym, ani drugim, np. „Biblioteka lm-sensors” bez biblioteki), żeby teksty zaczynały się w jednym miejscu.
- **Grupy podstron (M1):** w kolejności Płyta, BIOS, Procesor i system, Pamięć, Karta graficzna (osobna grupa na kartę, nazwa w opisie grupy), Dyski, Karty sieciowe („Szczegóły techniczne”), Czujniki i wentylatory (sterownik `nct6683`, sterowanie wentylatorami, konfiguracja lm-sensors, inne programy) i Profil płyty („Diagnostyka”). Wiersze `AdwActionRow`: pola jako para nazwa–wartość (styl `property`), urządzenia z nazwą i szczegółami w podtytule, stany z ikoną i tekstem. Czytnik ekranu dostaje nazwę wiersza i jego wartość. Szczegóły:
  - grupa bez danych ma wiersz „Brak danych”, a pole bez wartości wartość „Brak danych”; puste gniazda pamięci mają „Puste”;
  - kod płyty zawsze z przedrostkiem, „MS-7E61”, także w tekście do schowka (D38); stopka paska bocznego od D64 pokazuje samą nazwę płyty;
  - nazwa płyty według jednej reguły (D66, `core::board::board_name`): model z profilu, bez profilu `board_name` z DMI, a bez niego `product_name`; tak w stopce paska bocznego, w wierszu „Płyta” na stronie System i w „Szczegółach technicznych” oraz w opisie grupy „Płyta” w „Wszystkich czujnikach”; tekst do schowka podaje pola DMI tak, jak są;
  - wiersz profilu płyty mówi w tytule, co profil pozwala, a w podtytule podaje plik profilu albo przyczynę jego braku (D38): „Sprawdzony dla tej rewizji PCB” (ikona OK) albo „Rewizja PCB 2 niesprawdzona: tylko odczyt” i „Nieznana rewizja PCB: tylko odczyt” (ostrzeżenie; te same zdania co w wierszu stanu), wszystkie z podtytułem „Profil płyty: ms-7e61.toml”; „Brak profilu dla tej płyty” (to samo zdanie co w wierszu stanu i na stronie Wentylatory) z podtytułem „Profile dla MS-7E56 dotyczą modeli z innymi złączami wentylatorów” (D28) albo „Odczyty bez nazw złączy, bez sterowania wentylatorami” (D66: jeden tekst na jeden stan, bez słów „tryb ogólny”);
  - sterownik cpufreq nazywa się w interfejsie „Sterownik częstotliwości procesora” (D38);
  - czcionka o stałej szerokości tylko dla ścieżek i poleceń (pliki lm-sensors, polecenia sprawdzenia sterownika); wersja jądra i VBIOS zwykłą czcionką (D38);
  - wersja BIOS-u z datą i miesiącem słownie („1.A68 z 7 września 2026”, „1.A68 of 7 September 2026”), bo samo `%x` daje w części języków dwuznaczne „09/07/26” (D67, zastępuje D35): kolejność dnia, miesiąca i roku z tłumaczenia (format `g_date_time_format`, po polsku i angielsku „%-d %B %Y”), nazwa miesiąca z `LC_TIME`, tak jak liczby idą za `LC_NUMERIC`; tekst „Kopiuj informacje” podaje pole DMI tak, jak jest (`MM/DD/YYYY` według SMBIOS);
  - rozmiary przez `g_format_size` według lokalizacji: pamięć w jednostkach binarnych („16,0 GiB”), dyski w dziesiętnych („2,0 TB”);
  - poziom obsługi wentylatorów słowami (§5.5.3), bez numeru wersji jądra; przy niesprawdzonej rewizji PCB z dopiskiem „tylko odczyt”. Bez profilu stan sterownika jest informacją, nie ostrzeżeniem, bo płyta może nie mieć takiego układu;
  - dane czyta wątek roboczy przy każdym wejściu na stronę, a gdy strona jest widoczna, także po zmianie zestawu urządzeń hwmon (sprawdzanej co 2 s). Przy niewidocznym oknie wątek stoi;
  - przycisk „Kopiuj informacje” w pasku nagłówka, w wąskim oknie (do 599 sp, §5) sama ikona; po skopiowaniu powiadomienie `AdwToast`;
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
| Wentylatory, 0.2.0 (sam podgląd, bez pomocnika) | pod dużą temperaturą procesora zdanie „Ten system na razie tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.” z linkiem „Stan w jądrze” (strona projektu), bez numeru wersji jądra (D60, D62); lista z obrotami i wypełnieniem tam, gdzie jest, bez wykresów | lista z obrotami i wypełnieniem, tryb ręczny jako „Ręcznie”, bez wykresu; stopka „Baseboard na razie tylko pokazuje ustawienia wentylatorów z BIOS-u.” (D62) | jak `manual`, z małym wykresem krzywej w wierszu i stroną „Szczegóły krzywej” | jak `manual+curve-view` |
| Wentylatory od 0.3.0, bez pomocnika | jak w 0.2.0 (przeszkodą jest sterownik) | informacja „Podgląd bez sterowania” z „Jak zainstalować” (miejsce w układzie D62 do zaprojektowania); lista jak w 0.2.0 | jak obok | jak obok |
| Wentylatory od 0.3.0, z pomocnikiem | jak w 0.2.0; pomocnik startuje tylko na odczyt właściwości | przełącznik „Automat BIOS / Ręczny” | to samo; krzywe w podglądzie z dopiskiem, że edycja wymaga nowszego sterownika | od 0.4.0 przełącznik „Automat BIOS / Ręczny / Krzywa” i edytor; w 0.3.0 jak `manual+curve-view` |
| Baner „Zmiany działają także po zamknięciu aplikacji, do restartu komputera” | nigdy | gdy któryś kanał jest w trybie ręcznym ustawionym przez Baseboard | jak `manual` | jak `manual` albo gdy któryś kanał ma krzywą inną niż pierwotna |
| Strona System, poziom obsługi | „Sterowanie wentylatorami niedostępne” z tym samym zdaniem co wyżej, bez ikony ostrzeżenia (D60, D66) | „Tryb ręczny” | „Tryb ręczny i podgląd krzywych” | „Krzywe w kontrolerze płyty” |

Złącze bez wpisu w `tested_writes` (na MS-7E61 pompa, D52) ma zamiast przełącznika ramkę „Tylko podgląd: złącza nie sprawdzono na tej płycie.”. Nieistniejące `pwm3` jest w każdym przypadku tylko do odczytu. Wentylator dodatkowy (EZ Conn, `pwm4`) jest tylko do odczytu, dopóki sterownik nie obejmie tego kanału (`pwm4_enable`) i zapis nie zostanie sprawdzony na sprzęcie (D32). Oba z wyjaśnieniem.

#### 5.5.4 Strona Wentylatory

Wygląd według zaakceptowanych makiet (D45; układ strony od 0.2.0 według D62): szczegóły wiersza, wykresu, edytora, stanów, ruchu i dostępności oraz realizacja w GTK4 w §7 M2, M2.4. Tu zasady zachowania.

- **Karta „Procesor” (D67):** ta sama karta co na Przeglądzie (temperatura, historia 5 minut z zakresem, wentylator procesora, „Najwyższa od uruchomienia”), która niczego nie otwiera; pod faktami zawsze zarezerwowane zdanie „Wentylatory przyspieszają”, widoczne, gdy któryś wentylator dochodzi do wyższego celu (D62). W szerokim i gęstszym oknie (§5) karta stoi po lewej, a lista po prawej, obie od tej samej wysokości; węziej karta nad listą. Próg awaryjny jest na stronie „Szczegóły krzywej”. Nad listą pasek „Wentylator procesora stoi” z opisem „Kręcił się wcześniej, a od co najmniej 10 s ma 0 rpm.”, a w wierszu wentylatora stan „Stoi” w kolorze ostrzeżeń (D65).
- **Wiersz wentylatora:** nazwa z profilu (§6.2), stan zwykłymi słowami, mały wykres krzywej z punktem pracy, obroty i wypełnienie w %; oznaczenie złącza, minimum, tryb i cel szczegółowo na stronie „Szczegóły krzywej”, którą otwiera wiersz (D62). Tryb widać na liście tylko, gdy nie jest automatyczny. Etykiety „Krzywa z BIOS-u” nie ma. Przełącznik trybu (AdwToggleGroup) i „Edytuj krzywą” z M2b dostaną miejsce w układzie D62 przed 0.3.0.
- **Tryby w interfejsie a sterownik** (D11):

  | Przełącznik | `pwmN_enable` | Krzywa w EC |
  |---|---|---|
  | Automat BIOS | 2 | pierwotna (B18); gdy była zmieniona, wybranie zapisuje ją z powrotem, bez walidacji |
  | Ręczny | 1 | bez zmian |
  | Krzywa | 2 | zmieniona przez użytkownika |

  „Krzywa” jest widoczna tylko przy `curve-write`. Przy samym `curve-read` wiersz w automacie pokazuje krzywą z EC do podglądu.
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
- **Pompa ze sterowaniem:** stałe ostrzeżenie przy pompie, że nie może się zatrzymać, także w krzywej (B6). Pompa bez sprawdzonego zapisu (D52) nie dostaje przełącznika; informacja o niesprawdzonym złączu (`verified = false`) stoi na stronie „Szczegóły krzywej”, w grupie „Złącze” (D62).
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

   Gdy kod pasuje, ale żaden profil nie pasuje do modelu, aplikacja działa w trybie ogólnym (pkt 5), a strona System (wiersz profilu płyty w „Diagnostyce”) mówi, że profile dla tego kodu dotyczą modeli z innymi złączami; strona Wentylatory mówi „Brak profilu dla tej płyty” z przyciskiem „Szczegóły” do strony System (D66). Sterowania wtedy nie ma nigdy.
3. **Rewizja PCB:** pierwsza cyfra `board_version`, u nas „1.0” → 1. Rewizje mogą różnić się sprzętem, więc profil wymienia te, które zweryfikowano. Inna albo nieznana rewizja oznacza tryb tylko do odczytu z ostrzeżeniem (B9); ostrzeżenie stoi w wierszu stanu strony System i w „Diagnostyce”. Gdy płycie odpowiada po modelu kilka profili, wygrywa ten, który wymienia jej rewizję. Model rozstrzyga przed rewizją: profil z nazwą modelu, ale bez tej rewizji, daje tryb tylko do odczytu, nawet gdy profil bez listy nazw wymienia rewizję.
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
  | temperatura `cpu` | Processor (board sensor) | Procesor (czujnik płyty) |
  | temperatura `system` | System temperature | Temperatura systemowa |
  | temperatura `vrm` | Processor power stage | Sekcja zasilania procesora |
  | temperatura `chipset` | Chipset | Chipset |
  | napięcie `p12v`, `p5v`, `p3v3` | bez msgid: napięcie nominalne formatowane jak liczby (§5.1, D26) | +12 V, +5 V, +3,3 V |
  | napięcie `vcore`, `soc`, `vddio` | Processor cores, Processor SoC, Processor VDDIO | Rdzenie procesora, SoC procesora, VDDIO procesora |

  Po angielsku procesor to w nazwach czujników i podpisach zawsze „Processor” (D66), tak jak duża liczba na Przeglądzie i na stronie Wentylatory; tylko nazwy wentylatorów zostają przy „CPU fan *N*”, bo tak brzmi złącze `CPU_FAN`.

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

**Stan prac** (zastępowany w commicie, który kończy krok; najwyżej 250 słów, bez przebiegu prac, który jest w historii gita):
- **Wydane:** 0.1.0 (M1: Przegląd, System, Preferencje; Flatpak w repozytorium flatpak.bojanowski.me; strona projektu).
- **W toku:** M2a, wydanie 0.2.0. W `main` są strona Wentylatory w podglądzie, wygląd D63 i D64, układ szerokiego okna D67 przyjęty przez właściciela, z poprawkami po arkuszach (prawdziwe zdanie o wentylatorach, liczby całkowite na kartach, bez wielokropków, data BIOS-u słownie), ostrzeżenie D65, teksty D66 i zasady jakości D68 z CI. Dalej krok 6 (domknięcie M2a i wydanie, §7 M2.3).
- **Otwarte przed 0.2.0:** zielony przebieg CI po pushu (pierwsze dwa padły na brakującym schemacie GSettings w tourze, i-węzłach w kontenerze i nazwie układu, której libsensors nie parsuje; poprawione); test z Orca (właściciel); pomiar CPU w buildzie release przy widocznym i ukrytym oknie, także na module Fedory (D29); sprawdzenie Flatpaka zainstalowanego w osobnym katalogu; znak towarowy w bazach (D1); metainfo, README i strona w kroku wydania (§10.7).
- **Później:** po 0.2.0 powtórzenia w kodzie i podział dużych plików (`app/system.rs`, `app/fan_chart.rs`, teksty Przeglądu, `core/system.rs`), wspólny moduł FFI dla `dlopen`, nowa nazwa `core::overview` (model „Wszystkich czujników”), cecha `display` crate'u `toml`; `fans::driver::read` czeka na `GetState` pomocnika (M2b, lista wyjątków w `tests/core-api-exceptions.txt`); `cargo deny` przy zgłoszeniu na Flathub (F3); M2b (pomocnik, tryb ręczny 0.3.0, edytor krzywych 0.4.0), M2c, M3–M5.

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

**Kroki** 1–13 ukończone 2026-11-11; jak przebiegały, mówi historia gita.

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

**Plan** (D45–D56): M2a podgląd bez pomocnika (0.2.0), M2b pomocnik, tryb ręczny (0.3.0) i edytor krzywych (0.4.0), M2c wybór źródła temperatury.

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
- **Gdy v1 zmieni interfejs:** tabela „Gdy recenzja v1 coś zmieni” (koniec sekcji). Edytor powstaje pod v1, ale do wydania trafia dopiero, gdy zapis krzywych jest w `hwmon-next` (D47); wtedy warstwa sterownika i próbki idą za wersją przyjętą.
- **Gdy v1 nie zostanie przyjęta:** M2a i tryb ręczny od niej nie zależą i wychodzą normalnie. Podgląd krzywych zostaje, bo odczyt jest w `hwmon-next`. Edytor zostaje na `main` bez wydania (pomocnik i tak go nie włączy bez `curve-write` w atrybutach) albo go usuwamy. Nowa propozycja zapisu w jądrze to nowy wiersz tabeli zmian.
- **Gdy `hwmon-next` zmieni się przed wejściem do drzewa Linusa** (np. cofnięcie serii): poziom wykrywany po atrybutach spada sam, bez zmian w kodzie; przesuwa się plan wydań, nie kod.
- **Do uzupełnienia, gdy sterowanie wejdzie do wydanego jądra (D41, D60):** numer wersji Linuksa w zdaniu „Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.” (strona Wentylatory i wiersz strony System; wtedy np. „Sterowanie wymaga Linuksa X.Y lub nowszego.”), w README i na stronie projektu (sekcja o jądrze).
- **Seria Dahlina a zasady bezpieczeństwa:** dziś po wyładowaniu modułu w trybie ręcznym zostaje stałe wypełnienie bez nadzoru, do ponownego załadowania albo restartu (§4). Pomocnik nie może temu zapobiec, bo bez sterownika nic nie zapisze. Plan: nowa reguła B3(g) (D49) i opis ryzyka szczątkowego w §4. Gdy seria wejdzie, wyładowanie samo przywróci automat, a B3(g) niczego nie zmieni. Jeśli przy wyładowaniu przywraca też krzywe BIOS-u, migawka B18 dalej się zgadza; pomocnik po powrocie urządzenia porówna krzywe z migawką i z tym, co sam zapisał (D49), bez założeń o zachowaniu sterownika, tak jak przy wznowieniu (B12).
- **Wybór źródła temperatury** dopiero po dowodzie sprzętowym: (a) łatka 5 w kształcie wysłanym na listę (D17); (b) test na MS-7E61, że po zmianie źródła EC liczy wypełnienie z nowej temperatury (np. źródło temp3, sekcja zasilania nagrzana obciążeniem, `pwmN` zgodne z krzywą dla temp3, a nie dla temp1); (c) źródło po S3 i po restarcie. Do tego czasu źródło jest tylko do podglądu.

#### M2.3 Etapy, kroki i to, co widzi użytkownik

Każdy krok kończy się działającym buildem, zielonymi sprawdzeniami z §8 i osobnym commitem. Kolumna „Na sprzęcie” wymienia to, co sprawdza właściciel; odczyt sysfs nie wymaga zgody, zapis i polecenia z `sudo` tak, za każdym razem osobno.

**M2a – podgląd krzywych i źródła, tylko odczyt, bez pomocnika (natywnie i we Flatpaku), wydanie 0.2.0**

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 1 | Warstwa sterownika do odczytu: cecha `FanDriver` (tryb, wypełnienie, krzywa, źródło), `SysfsDriver` i `ModelDriver` do odczytu, milistopnie ↔ °C, maska `pwmN_auto_channels_temp` → klucz temperatury z profilu. Kodu zapisu w tym kroku nie ma. | testy na trzech drzewach z M1 i na `tryby-mieszane`; na MS-7E61 z modułem v1 krzywe i źródła sześciu kanałów równe `cat` (odczyt). **Zrobione.** | – |
| 2 | Model krzywej w `core`: n punktów z liczby plików, interpolacja liniowa, wartości poza skrajnymi punktami, cel w punkcie pracy, źródło → próg awaryjny, opis krzywej dla czytnika ekranu. | testy jednostkowe; na MS-7E61 cel z modelu równy `pwmN` w stanie ustalonym na sześciu kanałach, w spoczynku i po krótkim obciążeniu procesora (odczyt, bez zapisu do sprzętu); wynik w §4. **Zrobione.** | właściciel: trzy obciążenia po 60 s (zrobione) |
| 3 | Widżet wykresu `app::fan_chart` (M2.4): miniatura i duży wykres, strefy, punkt pracy, przejścia, tryb podglądu bez uchwytów. Geometria, rozmieszczanie etykiet i opis dla czytnika jako czyste funkcje z testami. | testy; zrzut debug wykresu zgodny z makietą w obu stylach; koszt CPU zmierzony. **Zrobione.** | – |
| 4 | Strona Wentylatory w podglądzie (dziś w układzie D62: lista z dużą temperaturą), stany `read-only`, `manual`, `manual+curve-view` i `curves` bez pomocnika, złącze puste, wentylator stoi, „Tylko odczyt” dla JAF_1, podstrona krzywej w podglądzie, wpis w pasku bocznym. Odczyt trybu przy każdej próbce tylko przy widocznej stronie, krzywych przy wejściu i co 10 s. | zrzuty prawdziwej aplikacji na module v1 (jasny i ciemny styl, EN i PL, szerokie i wąskie okno) obok renderów makiet, z listą różnic; natywnie i we Flatpaku. **Zrobione.** | wygląd z modułem v1 i z modułem Fedory (przeładowanie przy otwartej stronie, `sudo`), Orca na stronie i podstronie |
| 5 | Przegląd: tryb w podtytule wentylatora przy `mode` (§5.5.3). | testy modelu; zrzut. **Zrobione.** | wygląd Przeglądu z modułem Fedory i v1, natywnie i we Flatpaku (w tym kroku Flatpak nie był budowany); pomiar CPU Przeglądu bez pracy na pulpicie |
| 6 | Domknięcie M2a: tłumaczenia, metainfo (opis wydania, zrzuty w `website/screenshots/0.2.0/`), README i strona, kryteria M2a (M2.8), wydanie 0.2.0 według §10.7. | kryteria M2a | push, tag, wydanie, publikacja w repozytorium Flatpak |

**M2b – pomocnik jako osobny pakiet systemowy, potem tryb ręczny, potem edytor krzywych**

*Pomocnik (bez zapisu do sprzętu):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 7 | Crate `helper` (binarka `baseboard-helper`): aktywacja D-Bus, jednostka systemd z utwardzeniem (§3.3), polityka magistrali, akcje polkit tłumaczone przez `msgfmt`, właściwości z §3.5, `GetChannels`, `GetState`; profile tylko z katalogów systemowych; opcja meson do budowania i instalacji samego pomocnika. | testy na prywatnej magistrali z atrapą polkit; `systemd-analyze security` ≤ 5,0 | instalacja pomocnika (`meson install` z `sudo`), odczyt właściwości przez `busctl` |
| 8 | Klient w aplikacji: wykrycie pomocnika (`ServiceUnknown`, `AccessDenied` od pośrednika Flatpaka, brak odpowiedzi), `ApiVersion`, `ProfileDigest`; wiersz „Pomocnik systemowy” na stronie System i wersja w tekście do schowka; `--system-talk-name` w manifeście; stany „brak pomocnika”, „Flatpak bez uprawnienia”, „starszy pomocnik”, „inny profil”. | testy z atrapą pomocnika na prywatnej magistrali; zrzuty stanów | Flatpak z gałęzi `devel` w osobnym katalogu Flatpak: wykrycie pomocnika i jego braku |
| 9 | Pakiet: `packaging/rpm/baseboard.spec` z podpakietami `baseboard-helper` (pomocnik, jednostka, pliki D-Bus i polkit, profile) i `baseboard` (aplikacja natywna), zależności Rust przez `cargo vendor`, skryptlety z D50; budowanie `rpmbuild`, potem COPR. | `rpmlint`; instalacja, aktualizacja i odinstalowanie w maszynie wirtualnej bez śladów | instalacja RPM (`sudo`), projekt w COPR (konto właściciela) |

*Tryb ręczny (wydanie 0.3.0):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 10 | Zapis trybu i wypełnienia w `SysfsDriver` za cechą `write`, którą włącza tylko crate `helper` (B1); `SetManual`, `SetAuto`, `SetAllAuto` bez krzywych, `AcknowledgeEmergency`; dziennik B2, `--restore-auto` w `ExecStopPost` (B3 a, b), B5–B10 (z D16), B15. | testy z `ModelDriver` i na sztucznym sysfs, testy zabijania (`systemd-run`: SIGTERM, SIGKILL, SIGABRT), testy losowe argumentów D-Bus | – (zapis dopiero w kroku 13) |
| 11 | Pętla nadzoru: B4 (z D48, blokada w `/run/baseboard`), B11, B13, B12, B3 d (`PrepareForShutdown` z blokadą opóźniającą logind), B3 f, B3 g (D49), sygnały. | testy wstrzykiwania błędów, atrapa logind na prywatnej magistrali, zawieszenie pętli i watchdog | – |
| 12 | Interfejs trybu ręcznego: przełącznik (2 albo 3 pozycje), suwak z minimum, baner i „Przywróć automat BIOS”, wszystkie stany z `makiety/wentylatory-stany.html`. | zrzuty zgodne z makietą stanów; testy modelu widoku | – |
| 13 | Testy na sprzęcie, w tej kolejności: SYS_FAN1; SYS_FAN2–4 (po każdym wpis w `tested_writes`, D16); CPU_FAN1 z testem O10 (D53); drogi zakończenia B3; S3; próg obniżony do 50 °C; B11 z profilem testowym; zapis innym narzędziem; aktualizacja i odinstalowanie pakietu; przeładowanie modułu; polkit z Flatpaka (O19). | kryteria trybu ręcznego (M2.8) | wszystko w tym wierszu, każde za zgodą |
| 14 | Domknięcie: README (instalacja pomocnika), strona, zrzuty trybu ręcznego, wydanie 0.3.0 (aplikacja i pakiet `baseboard-helper`). | kryteria trybu ręcznego | push, tag, wydanie, COPR, publikacja Flatpaka |

*Edytor krzywych (wydanie 0.4.0, gdy zapis krzywych jest w `hwmon-next`, D47):*

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 15 | Planer B16 i walidacja B7, B14 w `core`. | testy właściwościowe (M2.8) | – |
| 16 | Pomocnik: `GetEcCurve`, `SetEcCurve`, `RestoreOriginalCurve`, migawka B18, B17, `SetAllAuto` i `--restore-all` z krzywymi, B12 dla krzywych; `ApiVersion` 2. | testy z `ModelDriver`: przycinanie, `-EIO` w połowie planu, `-ETIMEDOUT`, nieudany powrót | – |
| 17 | Edytor (podstrona): wykres z uchwytami, przeciąganie, klawiatura, lista punktów, tryb „Krzywa”, „Przywróć krzywą BIOS”, stany (niezapisane zmiany, błąd zapisu, krzywa łamiąca zasady, podgląd), okno „Odrzucić zmiany krzywej?”. Dwie uwagi do dużego wykresu ze zrzutów kroku 3 M2a (2026-11-11), rozwiązanie do zaprojektowania przy edytorze: uchwyt punktu przy 0 °C zasłania podpis osi wypełnienia (na prawdziwej krzywej CPU uchwyt 1 przy 48 % zakrywa „50 %”); w wąskim oknie uchwyty przy 85–95 °C i kółko progu awaryjnego przy 100 % nachodzą na siebie. | zrzuty zgodne z makietą; testy logiki edycji (ograniczenie do sąsiadów i do minimum, klawiatura, opis dla czytnika) | Orca na edytorze |
| 18 | Testy na sprzęcie z modułem v1: `pwm16`, potem SYS 2–4, CPU według D16; obroty pod obciążeniem idą za krzywą; S3 z krzywą; restart (po starcie krzywa BIOS-u); „Przywróć krzywą BIOS”; O17 opcjonalnie. | kryteria krzywych (M2.8) | wszystko w tym wierszu, każde za zgodą |
| 19 | Domknięcie: zrzuty edytora, README i strona, wydanie 0.4.0; potem F3 według D51. | kryteria krzywych | push, tag, wydanie, COPR, publikacja |

**M2c – wybór źródła temperatury** (po łatce 5 i dowodzie sprzętowym z M2.2)

| # | Krok | Kryterium zakończenia | Na sprzęcie |
|---|---|---|---|
| 20 | `source-write` według łatki 5 w kształcie wysłanym na listę (D17), `SetEcSource`, `ApiVersion` 3, wybór w edytorze (temperatury EC z profilu), B14 dla nowego źródła. | wybór źródła na module z łatką 5; dowód sprzętowy | test zmiany źródła, za zgodą |

Uwaga do projektu (D62, D67): karta „Procesor” na stronie Wentylatory zakłada jedno wspólne źródło krzywych. Gdy wentylatory będą mogły mieć różne źródła, ten układ trzeba zaprojektować od nowa.

**Co widzi użytkownik** na każdym poziomie sterownika, w każdym wydaniu, z pomocnikiem i bez, natywnie i we Flatpaku: tabela w §5.5.3.

#### M2.4 Wygląd strony Wentylatory (D62)

**Makiety** w `makiety/` (opis w `makiety/OPIS.md`), rendery poza repozytorium (`makiety/renderuj.sh` z `OUT=…`, `NARROW=1`, `ACCENT=…`):
- `wentylatory.html`: bez parametru pełne sterowanie (`curves` z pomocnikiem), `?stan=podglad` (brak pomocnika), `?stan=tylko-odczyt` (jądro Fedory);
- `wentylatory-krzywa.html`: edytor z niezapisanymi zmianami, `?stan=blad`, `?stan=bios-cpu` (domyślna krzywa CPU z §6.3, łamie B5), `?stan=podglad`;
- `wentylatory-stany.html`: 20 stanów karty i strony, każdy z tekstem i uwagą, kiedy występuje.

Wszystkie w jasnym i ciemnym stylu, w szerokim (1200 px) i wąskim oknie (400 px, pasek boczny zwinięty jak poniżej 600 sp). Pasek boczny ma tylko istniejące strony: Przegląd, Wentylatory, System. Dane to odczyty z MS-7E61 z 2026-11-11 (moduł v1, krzywe z ustawień BIOS-u właściciela, temp1 46 °C); własne krzywe, tryb ręczny i stany błędów są przykładowe.

**Strona (D62, D67)**, nowoczesny minimalizm: zwykłe słowa, typografia zamiast ramek, jeden kolor akcentu. W szerokim i gęstszym oknie dwie kolumny (karta po lewej, lista po prawej, od tej samej wysokości), węziej jedna. Od góry:
- karta „Procesor”, ta sama co na Przeglądzie (§5.1, D67): temperatura procesora z profilu w pełnych stopniach, historia, wentylator procesora i najwyższa temperatura od uruchomienia, bez zdań oceniających. Zakłada jedno wspólne źródło krzywych (notatka w M2c);
- pod faktami karty zawsze zarezerwowane zdanie „Wentylatory przyspieszają” (zwykły przygaszony kolor tekstu), widoczne, gdy co najmniej jeden wentylator dochodzi do wyższego celu o co najmniej 5 punktów procentowych; dzięki rezerwie lista się nie przesuwa. Na sterowniku bez sterowania pod kartą zdanie „Ten system na razie tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.” z linkiem „Stan w jądrze” do sekcji o jądrze na stronie projektu (pogrubiony, w kolorze akcentu, bez podkreślenia, z ikoną otwarcia na zewnątrz; nigdy nie łamie się w środku, w razie potrzeby przechodzi cały do następnego wiersza); to stały stan, więc może zajmować więcej;
- lista wentylatorów (boxed list libadwaita) w kolumnie do 600 sp (rośnie z rozmiarem tekstu), w kolejności złączy na płycie; złącza bez sterowania (JAF_1) w tej samej liście, bez wykresu;
- stopka „Baseboard na razie tylko pokazuje ustawienia wentylatorów z BIOS-u.” (0.2.0, na każdym poziomie sterownika poza „tylko odczyt”, przy którym stoi zdanie o jądrze).
Bez banera i ramek u góry, bez grupy „Źródło temperatury” i bez legendy nad listą.

**Wiersz wentylatora (D62):** nazwa z profilu; pod nią stan zwykłymi słowami, tylko gdy jest: „Nie kręci się – złącze może być puste.” (brak obrotów od uruchomienia aplikacji przy wypełnieniu większym od 0), „Nie udało się odczytać krzywej” (sterownik ma krzywą, ale jej odczyt się nie udał; także w trybie ręcznym, D66), „Ręcznie” (tryb widać tylko, gdy nie jest automatyczny), „Przyspiesza” albo „Zwalnia” (cel krzywej różni się od wypełnienia o co najmniej 5 punktów; mniejsze różnice dostaje tylko czytnik ekranu); mały spokojny wykres krzywej (jedna linia z lekko zaokrąglonymi narożnikami i delikatnym wypełnieniem pod nią, jeden punkt „teraz”, bez siatki, osi, kreskowania, progu i kropek punktów; w trybie ręcznym pozioma linia na wypełnieniu; 192 px, w wąskim oknie 84 px); obroty nad procentem (bez słowa „wypełnienie”); strzałka (przygaszona, `dimmed`, jak w innych wierszach). Bez oznaczenia złącza, sterownika, czujnika i „minimum”. Kliknięcie wiersza (albo Enter) otwiera „Szczegóły krzywej”, gdy kontroler ma krzywą do pokazania, także w trybie ręcznym, bo krzywa zostaje w kontrolerze, i gdy odczyt krzywej się nie udał; wiersz bez krzywej (sterownik jej nie podaje) nie otwiera niczego i nie ma strzałki.
- Podtytuł nigdy nie łamie się w środku zdania. W wąskim oknie obroty i procent schodzą do podtytułu („890 rpm · 49 %”) i podtytuł ma szerokość nazwy i wykresu; gdy z nimi zdanie stanu się nie mieści, zostaje samo zdanie. Gdy i ono się nie mieści (np. po angielsku obok karty), łamie się między słowami do następnego wiersza i wiersz rośnie; wielokropka nie ma (D67). Nazwa wentylatora też łamie się, zamiast się urywać.
- Liczby nie przesuwają się przy zmianie liczby cyfr: kolumna obrotów ma szerokość najszerszej wartości („8888 rpm”).

**Szczegóły krzywej (D62):** jak w 0.2.0 przed D62 (duży wykres, punkty, źródło, jak działa krzywa), plus to, co zniknęło z listy: oznaczenie złącza w podtytule („CPU_FAN1”), legenda pod wykresem (strefa poniżej minimum złącza, próg awaryjny, punkt pracy, przedłużenie krzywej; łamana całymi pozycjami) i grupa „Złącze”: tryb („Automat BIOS”, „Ręczny”, „Nie udało się odczytać trybu”), minimum z opisem „Najniższe wypełnienie, jakie Baseboard dopuści” (gdy złącze je ma), „Złącza nie sprawdzono na tej płycie” i „Nie kręci się od uruchomienia aplikacji” (z opisem „Złącze może być puste.”). W trybie ręcznym strona pokazuje krzywą z kontrolera, punkt pracy na stałym wypełnieniu (bez celu, bo krzywa niczego wtedy nie ustawia) i w grupie „Złącze” wiersz „W trybie ręcznym krzywa nie działa” z opisem „Wentylator ma stałe wypełnienie, a krzywa zostaje w kontrolerze płyty.”. Lista punktów: równe kolejne punkty jako jeden wiersz z numerem „4–7” (czytnik ekranu: „Punkty od 4 do 7: 85 °C, 100 %.”), bez dopisku pod listą. Gdy odczyt krzywej się nie udał, w miejscu wykresu i punktów wiersz „Nie udało się odczytać krzywej” z opisem „Baseboard próbuje ponownie co 10 sekund.”; grupa „Złącze”, źródło i opis działania zostają. Bez ramki „podgląd”: jedyna informacja o wersji to na dole strony to samo zdanie co stopka listy („Baseboard na razie tylko pokazuje ustawienia wentylatorów z BIOS-u.”); w opisie działania krzywej „Baseboard nie musi przy tym działać.” (D66).

**Do M2b:** baner (D4), przełącznik trybu (D11), tryb ręczny z suwakiem (B5), pompa ze sterowaniem (B6) i edytor mają w makietach jeszcze układ A (`wentylatory-krzywa.html`, `wentylatory-stany.html`); przed 0.3.0 dostaną projekt w układzie D62.

**Wykres krzywej** (duży, na stronie „Szczegóły krzywej” i w edytorze; w wierszu listy mały, spokojny wykres opisany wyżej, D62):
- Odcinki proste z zaokrąglonymi złączami, bo EC liczy wypełnienie liniowo między punktami (§4, model z kroku 2). Gładka krzywa (spline) pokazywałaby wypełnienia, których wentylator nigdy nie dostaje. Poniżej pierwszego i powyżej ostatniego punktu EC trzyma ich wypełnienie: linia przerywana. Pod krzywą gradient akcentu.
- Osie: temperatura źródła 0–100 °C (zakres B7), siatka co 10 °C, podpisy co 20 °C z jednostką („40 °C”); wypełnienie 0–100 % co 25 % („50 %”). W edytorze tytuły osi: „Wypełnienie” i „Temperatura: Procesor (czujnik płyty)”. W miniaturze mniej linii i podpisów.
- Strefa poniżej minimum B5 (procesor, pompa): kreskowanie i przerywana linia granicy; w edytorze także napis „Poniżej minimum 30 %” pod linią krzywej, więc jej nie zasłania. Próg awaryjny źródła: pas w kolorze błędu i przerywana pionowa linia; w edytorze napis poziomo nad wykresem, przy górze linii („Próg awaryjny 95 °C”, dla procesora i pompy „Próg awaryjny 95 °C: od niego 100 %” i kółko w punkcie progu przy 100 %, B14).
- Punkt pracy na żywo: kropka w zieleni z ikony aplikacji z poświatą, przerywane prowadnice do obu osi i znaczniki na osiach. W edytorze etykieta „Teraz 46,0 °C · 28 %” tam, gdzie nie zasłania uchwytów ani napisów; gdy krzywa jest zmieniona, a niezastosowana, puste kółko w akcencie i druga linia „po zastosowaniu 24 %” (w automacie przy dochodzeniu do celu: „cel 38 %”). Gdy etykieta się nie mieści (wąskie okno), te same wartości stoją w legendzie.
- Uchwyty (edytor): kółka z numerem punktu, takim jak na liście; punkty o tych samych wartościach to jeden uchwyt w kształcie pigułki („6–7”). Wybrany uchwyt jest wypełniony, a przy fokusie klawiatury ma obwódkę fokusu. Punkt, który łamie zasadę, ma kolor ostrzeżenia i znak ostrzeżenia przy uchwycie; powód („Ustaw co najmniej 30 %”) stoi raz, w jego wierszu listy.
- Krzywa, którą EC wykonuje teraz, gdy w edytorze jest inna: przerywana linia w kolorze tekstu.
- Legenda pod dużym wykresem tylko dla oznaczeń, których wykres sam nie podpisuje: „Nowa krzywa” i „W kontrolerze teraz”, a w wąskim oknie punkt pracy. Bez krzywej w kontrolerze i z etykietą punktu pracy na wykresie legendy nie ma. Nic nie opiera się na samym kolorze: strefy mają wzór, linie różny wzór kreskowania, punkt łamiący zasadę znak.
- Kolory (D55): krzywa, obszar i uchwyty w akcencie systemu (`accent_bg_color`, idzie za wyborem użytkownika; makiety z `?accent=…` pokazują inne akcenty libadwaita), numery i tekst akcentu w jego wersji tekstowej; punkt pracy w zieleni z ikony (#26a269 w jasnym stylu, #33d17a w ciemnym); próg w `error_color`, ostrzeżenia w `warning_color`, kreskowanie w kolorze tekstu. Przy zielonym akcencie punkt pracy odróżnia kształt (kropka z poświatą) i etykieta.

**Edytor (podstrona):** tytuł nazwą złącza, podtytuł „SYS_FAN1 · niezapisane zmiany”, „· krzywa z BIOS-u” albo „· podgląd”; duży wykres z legendą; lista 7 punktów, w której każdą wartość zmienia się przyciskami albo wpisuje z klawiatury (`GtkSpinButton`); grupa „Źródło temperatury” (nazwa i kanał, kłódka, bez wartości, którą pokazuje wykres); grupa „Jak działa krzywa”; dolny pasek „Przywróć krzywą BIOS”, „Cofnij zmiany”, „Zastosuj” (w wąskim oknie przywracanie z podpisem w osobnej linii). Ramka u góry tylko tam, gdzie mówi coś, czego nie mówi podtytuł ani legenda: błąd zapisu („Kontroler płyty nie przyjął ustawień”), krzywa łamiąca zasady („Ta krzywa nie spełnia zasad Baseboard”, „Zastosuj” nieaktywne z wyjaśnieniem na dolnym pasku), podgląd („Edycja krzywych i wybór źródła wymagają nowszego sterownika jądra.”).

**Stany sterowania** (M2b; `wentylatory-stany.html` w układzie sprzed D62, każdy z wyglądem i tekstem; opis makiet w `makiety/OPIS.md`): sterownik bez sterowania; wersja 0.2.0 (sam podgląd); brak pomocnika; Flatpak bez uprawnienia do pomocnika (z poleceniem i przyciskiem kopiowania); pomocnik w starszej wersji; inna wersja profilu w pomocniku; sterownik bez podglądu krzywych (`manual`); sterownik bez zapisu krzywych (`manual+curve-view`); tryb ręczny z minimum; pompa ze sterowaniem (stałe ostrzeżenie B6); wentylator stoi (0 %, zamierzone, bez ostrzeżenia); wentylator stanął (B11, czerwona obwódka karty, ikona i tekst); złącze puste; krzywa zmieniona i niezapisana (okno „Odrzucić zmiany krzywej?” przy wyjściu z edytora); błąd zapisu w trybie ręcznym; dochodzenie do celu; blokada awaryjna (D48: pełne wypełnienie, „Zdejmij blokadę” aktywne po spadku o 10 °C, wiersz wraca po ponownym uruchomieniu aplikacji); inny program zmienił ustawienia (B10); pomocnik przestał działać; krótkie komunikaty (`AdwToast`: anulowana autoryzacja, zastosowana krzywa, powrót do automatu BIOS). Błędy bez kodów; szczegóły w dzienniku (B15). Na jednym ekranie żaden tekst nie powtarza informacji z innego miejsca tego ekranu.

**Ruch.** `AdwTimedAnimation`, która przy wyłączonych animacjach (ustawienie „Ogranicz animacje”, `gtk-enable-animations = false`) kończy się od razu:
- punkt pracy: przejście 250 ms (ease-out-cubic), tylko gdy przesuwa się o więcej niż 2 px; mniejsze zmiany co sekundę bez animacji; w wierszu listy punkt przeskakuje, płynnie przechodzi tylko na dużym wykresie (D57: koszt CPU);
- po zastosowaniu krzywej linia przerywana przechodzi w nową w 300 ms; po „Cofnij zmiany” odwrotnie;
- krok uchwytu z klawiatury: 120 ms; przeciąganie myszą bez animacji (uchwyt idzie za kursorem);
- liczby zmieniają się od razu, bez animacji; żadnych animacji ciągłych (np. obracającej się ikony), bo kosztowałyby klatkę co 16 ms.

**Dostępność:**
- Klawiatura: przełącznik, suwak (strzałki, PageUp/PageDown), przyciski i pola liczbowe jak w libadwaita; wartości punktów można wpisać w polach listy (liczba z jednostką albo bez, „50” i „50 %”), a pole odrzuca wartość spoza dozwolonego zakresu tak jak przeciąganie. Wykres edytora przyjmuje fokus: Tab i Shift+Tab wybierają kolejny punkt (po ostatnim fokus wychodzi z wykresu), strzałki przesuwają punkt o 1 °C albo 1 %, z Shift o 5, Home i End wybierają pierwszy i ostatni punkt. Ruch jest ograniczony do sąsiednich punktów i do minimum B5, tak jak przy przeciąganiu. Skróty są w oknie skrótów klawiszowych i w wierszu „Klawiatura” edytora.
- Czytnik ekranu: wiersz listy ma nazwę wentylatora jako etykietę, a w opisie to, czego lista nie pokazuje albo pokazuje skrótem: oznaczenie złącza, obroty, wypełnienie, cel, do którego kontroler je zmienia, tryb, minimum i stan złącza (D62); mały wykres w wierszu ma rolę obrazu z pełnym opisem (punkty, źródło, temperatura i wypełnienie teraz). Wiersze przechodzi się klawiaturą, Enter otwiera szczegóły. W edytorze pełną drogą edycji jest lista punktów (`GtkSpinButton` z nazwami „Punkt 3, temperatura”, „Punkt 3, wypełnienie”); zmiana punktu z klawiatury na wykresie jest ogłaszana (`gtk_accessible_announce`, GTK 4.14).
- Kontrast (WCAG, policzony z wartości libadwaita 1.10 i kolorów makiet): podpisy osi 5,2:1 w jasnym i 7,3:1 w ciemnym stylu; linia krzywej 3,8:1 i 3,3:1; punkt pracy 3,3:1 i 6,3:1; próg 6,3:1 i 5,8:1; numer wybranego uchwytu 5,8:1 i 9,4:1; krzywa w kontrolerze 3,9:1 i 5,9:1; teksty błędów i ostrzeżeń w ramkach 4,9–7,9:1. W trakcie projektu poprawione: biały numer na akcencie (3,8:1) i przerywana linia przy kryciu 45 % (2,5:1 w jasnym stylu). Przygaszony tekst libadwaita (krycie 55 %) ma w jasnym stylu 3,2:1, więc wartości i podtytuły złączy na tej stronie mają krycie 72 % (5,2:1), a przygaszone są tylko objaśnienia powtórzone gdzie indziej.

**Spójność z Przeglądem i Systemem:** te same nazwy wentylatorów (rola i numer z profilu, własne nazwy, D22), ta sama kolejność złączy, tytuł strony bez podtytułu (D64), liczby i jednostki z `core::units`; zdanie o sterowniku bez sterowania to samo na stronie Wentylatory, w wierszu stanu strony System i w „Diagnostyce” (D66). Oznaczenia złączy i wypełnienia z podtytułów „Wszystkich czujników” (np. „CPU_FAN1 · wypełnienie 49 % · złącze niesprawdzone”) są na liście wentylatorów dopiero w „Szczegółach krzywej” i w opisie dla czytnika ekranu.

**Realizacja w GTK4:**
- Własny widżet `app::fan_chart` (podklasa `GtkWidget`): duży wykres „Szczegółów krzywej”, a w M2b edytora (mały wykres w wierszu listy to osobny, prostszy widżet `fans/spark.rs`). Rysuje w `snapshot()`: krzywa i linie przez `GskPathBuilder` i `append_stroke` (zaokrąglone złącza, przerywanie przez `GskStroke::set_dash`), obszar pod krzywą przez `push_fill` z gradientem, kreskowanie strefy jedną ścieżką ukośnych linii (1 px co 6 px) w przycięciu, tekst przez `PangoLayout` z kontekstu widżetu (Adwaita Sans, cyfry tabelaryczne `tnum`). Antyaliasing i skalę HiDPI zapewnia GSK, więc linie są gładkie przy każdej skali.
- Siatka, osie, strefy, podpisy i krzywa tworzą jeden węzeł renderowania, a punkty krzywej drugi; oba są w pamięci podręcznej do zmiany rozmiaru, stylu (jasny, ciemny, wysoki kontrast, akcent), czcionki albo krzywej. Prowadnice punktu pracy leżą między nimi, a sam punkt i etykieta nad punktami krzywej, jak w makiecie. Nowy odczyt rysuje tylko punkt pracy i etykietę; odczyt bez zmian nie rysuje niczego.
- Kolory: tekst z `widget.color()`, akcent z `AdwStyleManager` (`accent-color-rgba`, wersja tekstowa przez `AdwAccentColor::to_standalone_rgba`), kolory błędu i ostrzeżenia z wartości libadwaita dla stylu jasnego i ciemnego, zieleń z ikony; przerysowanie po `notify::dark`, `notify::high-contrast` i `notify::accent-color-rgba`, a po zmianie czcionki albo wielkości tekstu (`gtk-font-name`, `gtk-xft-dpi`) także podpisów. Ograniczenie: własny CSS użytkownika, który zmienia `error_color`, nie zmieni wykresu.
- Interakcja: `GtkGestureDrag` (uchwyt łapany w promieniu 16 px, wartości całkowite °C i %, ograniczenie do sąsiadów i minimum), `GtkEventControllerMotion` (kursor i podświetlenie), `GtkEventControllerKey` (klawisze wyżej).
- Ograniczenia: elementy wewnątrz własnego widżetu nie są dla czytnika ekranu osobnymi obiektami (GTK pozwala na to od 4.10 przez własną implementację `GtkAccessible`, której w M2 nie planujemy), dlatego lista punktów jest pełną drogą edycji. Etykietę punktu pracy rozmieszcza prosta reguła kandydatów (pozycje wokół punktu, bez zasłaniania uchwytów i napisów); gdy żadna nie pasuje, wartości idą do legendy.
- Koszt CPU przy odświeżaniu co sekundę: wykresy dostają nowy punkt pracy, a GSK składa węzły z pamięci podręcznej z kilkoma nowymi kształtami. Przejście 250 ms to tyle klatek, ile da odświeżanie ekranu (przy 120 Hz ok. 30), ale tylko na dużym wykresie i tylko przy zmianie o więcej niż 2 px (D57). Odczyty: `pwmN_enable` przy każdej próbce tylko przy widocznej stronie, która pokazuje tryb: Wentylatory (tryby kanałów z odczytem krzywych) albo Przegląd (od kroku 5 M2a tylko tryby kanałów z `mode`, do podtytułów, §5.5.3); każdy odczyt idzie do EC, poza pamięcią podręczną sterownika (M1), a każdy tryb jest czytany raz na próbkę, bo Przegląd bierze tryby odczytane dla strony Wentylatory; przy stronie System żadnych. Krzywe przy wejściu na stronę Wentylatory, co 10 s i po sygnale pomocnika (ok. 84 odczytów rejestrów). Kryterium jak w D29: cała aplikacja z widoczną stroną Wentylatory poniżej 1 % jednego rdzenia, przy ukrytym oknie praktycznie zero; koszt odczytu sprzętu osobno, jako pomiar.

#### M2.5 Bezpieczeństwo: B1–B18 po kolei

| Zasada | Krok | Test automatyczny | Test na sprzęcie (właściciel, za zgodą) | Czego nie da się sprawdzić w pełni |
|---|---|---|---|---|
| B1 tylko pomocnik pisze | 1, 10 | funkcje zapisu `SysfsDriver` są za cechą `write`, którą włącza tylko crate `helper`, więc aplikacja nie ma kodu zapisu; test zależności crate'ów; `strace` w kryteriach | `strace` aplikacji natywnej i z Flatpaka przy pracy z pomocnikiem (odczyt) | – |
| B2 dziennik i kolejność zapisu | 10 | `ModelDriver`: wpis w `/run/baseboard/claimed` przed pierwszym zapisem, kolejność `pwmN_enable` → `pwmN`; bez katalogu `/run/baseboard` zapis odrzucony | odczyt `/run/baseboard/claimed` po włączeniu trybu ręcznego | – |
| B3 a SIGTERM | 10 | `systemd-run` na sztucznym sysfs: SIGTERM → `pwmN_enable = 2` | `systemctl stop` przy trybie ręcznym, odczyt sysfs | – |
| B3 b awaria | 10 | SIGKILL i SIGABRT (`panic = "abort"`) → `ExecStopPost=… --restore-auto` | `kill -KILL` i `kill -ABRT` pomocnika | OOM: tylko przez SIGKILL, tą samą drogą |
| B3 d wyłączenie i restart | 11 | atrapa logind: `PrepareForShutdown(true)` → automat przed zwolnieniem blokady opóźniającej | wyłączenie i restart przy trybie ręcznym; dowodem jest wpis z poprzedniego uruchomienia (`journalctl -b -1 -u baseboard-helper`) | stanu sysfs po wyłączeniu nie da się odczytać, a po starcie BIOS i tak ustawia automat |
| B3 e, f próg i błąd odczytu | 11 | wstrzykiwanie: przekroczenie progu, znikający plik, wartości niewiarygodne | próg obniżony w `/etc` do 50 °C i krótkie obciążenie | – |
| B3 g przeładowanie modułu (D49, nowa) | 11 | sztuczny sysfs: urządzenie znika i wraca z `pwmN_enable = 1` → automat; krzywe porównane bez nadpisywania | `modprobe -r` i `modprobe` przy trybie ręcznym (`sudo`) | czas między wyładowaniem a załadowaniem jest bez nadzoru (ryzyko szczątkowe) |
| B4 próg awaryjny | 11 | 3 kolejne odczyty, warunek z D48, histereza 10 °C, blokada przeżywa restart pomocnika (`/run/baseboard`), `AcknowledgeEmergency` odrzucone przed spadkiem | próg obniżony do 50 °C; prawdziwy próg 95 °C tylko w teście CPU (D53) | MOS 90 °C i System 80 °C na prawdziwym progu (nie przegrzewamy płyty) |
| B5 minimum | 10, 15 | proptest: dowolne żądanie daje co najmniej 76 (CPU) i 127 (pompa); profil nie zejdzie poniżej stałej granicy; każdy punkt krzywej | `busctl` `SetManual cpu1 0` zwraca 76 (po teście zapisu CPU) | – |
| B6 pompa nie staje | 10, 15 | 0 dla roli `pump` → `InvalidArgument`, także w krzywej | – (na MS-7E61 pompa bez sterowania, D52) | zachowanie prawdziwej pompy: żadna nie jest podłączona |
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
- **Próg osiągalny w kilka sekund (O10).** Przy wentylatorze procesora w trybie ręcznym na minimum temperatura może dojść do progu w kilka sekund, a B4 reaguje po 3 odczytach. Wyjście w automacie dochodzi do celu stopniowo (§4), więc samo przejście na automat chłodzi wolno. Dlatego D48: przy progu od razu 100 % na kanałach sterowanych ręcznie przez Baseboard, a warunek dla procesora „osiągnie” zamiast „przekroczy”, bo procesor dławi przy Tjmax 95 °C i temp1 może stanąć dokładnie na progu. Procesor chroni się też sam (dławienie przy Tjmax). Test: D53.
- **Krzywa z BIOS-u łamiąca B5** (domyślna krzywa CPU, §6.3). Podgląd (M2a) pokazuje ją bez ostrzeżenia, bo to ustawienie firmware. Edytor pokazuje ją bez zmian, wskazuje punkt 1 („Poniżej minimum 30 %”) i blokuje „Zastosuj” (makieta `bios-cpu`). „Przywróć krzywą BIOS”, `SetAllAuto` i powrót w B17(b) zapisują ją bez walidacji (B18). Test automatyczny na tej krzywej; na sprzęcie tylko przy domyślnych ustawieniach BIOS-u.
- **Zapis sprawdzony tylko na SYS_FAN1.** Profil ma dziś `tested_writes` tylko dla `sys1`. SYS_FAN2–4 dostają wpis po teście w kroku 13, CPU_FAN1 po teście D53. Do tego czasu karta pokazuje „Tylko podgląd: zapisu na tym złączu nie sprawdzono na tej płycie”.
- **Puste złącza pompy i wentylatora dodatkowego.** Na PUMP_SYS1 nic nie jest podłączone (0 rpm przy 100 %), więc nie da się sprawdzić ani przypisania (O3), ani zapisu (O5); pompa zostaje bez sterowania (D52). B11 nie reaguje, bo złącze nie miało obrotów. Strona mówi „Brak obrotów od uruchomienia aplikacji. Złącze może być puste.”, a pustego złącza od pompy, która nie ruszyła, nie odróżnia. JAF_1: bez `pwm4_enable` w sterowniku, tylko odczyt (D32).
- **Awaria pomocnika:** B3(b) przywraca automat kanałom w trybie ręcznym; krzywe w EC działają dalej. Aplikacja dostaje zniknięcie nazwy z magistrali, pokazuje „Pomocnik systemowy przestał działać” i nie wznawia trybu ręcznego sama.
- **Zamknięcie aplikacji:** nic się nie zmienia. Pomocnik nadzoruje tryb ręczny dalej (B4, B11, B13), krzywe działają w EC; baner i opis strony o tym mówią.
- **Uśpienie:** bez zmian przed uśpieniem (D13); po wybudzeniu B12 (sterownik z `7dc9dff9608d` zwykle odtwarza tryb ręczny sam). Hibernacja: O4.
- **Wyłączenie i restart:** B3(d) z blokadą opóźniającą logind; po starcie BIOS ustawia automat i swoje krzywe (D4), `/run` jest pusty.
- **Przeładowanie modułu:** B3(g) (D49). Pomocnik widzi zniknięcie urządzenia (udev), wysyła `ControlLost` z nowym powodem `driver-reloaded`, po powrocie urządzenia przywraca automat kanałom z dziennika i porównuje krzywe. Czas bez modułu jest bez nadzoru (§4).
- **Aktualizacja pakietu pomocnika:** restart pomocnika (D50), tryb ręczny wraca do automatu przez B3(a), krzywe zostają; aplikacja pokazuje komunikat jak przy awarii, z inną treścią („Pomocnik został uruchomiony ponownie”).
- **Odinstalowanie:** D50, `baseboard-helper --restore-all` przed usunięciem plików: krzywe pierwotne i automat na kanałach zmienionych przez Baseboard.

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

Pomocnik, pakiety, zgodność wersji, brak pomocnika, polkit z Flatpaka i odinstalowanie: §3.3, §3.5, §10.4 i §10.5.

#### M2.7 Wydania

| Wydanie | Zawartość | Warunek | Strona, README, metainfo |
|---|---|---|---|
| 0.2.0 | M2a: strona Wentylatory w podglądzie, cała aplikacja w wyglądzie D63 i D64 | kryteria M2a | zrzuty (jasny i ciemny styl, pl i en) w `website/screenshots/0.2.0/` i w metainfo; opis w metainfo (`<description>` i `<caption>`), README „What it does today” i tabela poziomów, strona projektu przepisane pod 0.2.0; podsumowanie bez obietnicy sterowania; uprawnienia Flatpaka bez zmian |
| 0.3.0 | pomocnik (`baseboard-helper`, COPR) i tryb ręczny | kryteria trybu ręcznego; `hwmon-next` w wydanym jądrze nie jest warunkiem, ale bez niego sterowanie działa tylko z modułem testowym (README mówi to wprost) | instalacja pomocnika w README i na stronie; zrzuty trybu ręcznego; `--system-talk-name` w manifeście; nowe podsumowanie, jeśli obiecuje sterowanie |
| 0.4.0 | edytor krzywych | kryteria krzywych i zapis krzywych w `hwmon-next` (D47) | zrzuty edytora; tabela „Fans and the kernel” i karta na stronie (stan łatek z drzewa jądra, D41) |
| później | M2c (wybór źródła) | łatka 5 i dowód sprzętowy | jak wyżej |

Każde wydanie według procedury z §10.7.

**Zasady całego M2a:** tylko odczyt, żadnego zapisu do `/sys` (`strace` jak w M1); interfejs wygląda jak zaakceptowane makiety, a krok ze stroną Wentylatory pokazuje zrzuty prawdziwej aplikacji obok renderów makiet z listą różnic; jeden krok, raport (co zrobione, testy, co sprawdzone na sprzęcie, co ma sprawdzić właściciel), stop.

#### M2.8 Kryteria ukończenia i testy

**M2a (0.2.0)**
- Strona Wentylatory na sztucznych drzewach wszystkich poziomów i na sprzęcie z modułem Fedory i modułem v1 wygląda jak przyjęte makiety (`wentylatory.html`, `wentylatory-szczegoly.html`; D62), natywnie i we Flatpaku, w obu stylach, przy szerokim i wąskim oknie, po polsku i po angielsku.
- Krzywe na podglądzie równe `cat` atrybutów `pwmN_auto_point*`; cel z modelu równy `pwmN` w stanie ustalonym (krok 2).
- Aplikacja niczego nie zapisuje i nie łączy się z pomocnikiem (`strace` jak w M1).
- CPU (D29): cała aplikacja z widoczną stroną Wentylatory poniżej 1 % jednego rdzenia (`pidstat -t`, 60 s, build release); przy ukrytym oknie praktycznie zero (otwarte z M1).
- Orca odczytuje nazwy, wartości, tryby i opisy wykresów na stronie i podstronie (otwarte z M1).
- Przy włączonym „Ogranicz animacje” brak przejść.

**Tryb ręczny (0.3.0)**
- Na module testowym `sys1`–`sys4` i `cpu1` (po D53) działają w trybie ręcznym, a obroty reagują. `pump1` (D52), `ezconn` (D32) i `pwm3` są tylko do odczytu z wyjaśnieniem.
- Każda droga zakończenia z B3 (SIGTERM, SIGKILL, abort, zawieszenie pętli i watchdog, przeładowanie modułu, aktualizacja pakietu) zostawia każdy kanał w trybie ręcznym w `pwmN_enable = 2`, co sprawdzamy odczytem sysfs po zdarzeniu; wyłączenie i restart wpisem w dzienniku z poprzedniego uruchomienia.
- Po S3 tryb ręczny jest taki jak przed uśpieniem: odtworzony przez sterownik albo przez B12.
- Minimum B5 i zakaz B6 nie dają się obejść żadnym wywołaniem D-Bus.
- Próg awaryjny działa na sprzęcie z progiem obniżonym do 50 °C; test CPU według D53 daje wynik O10.
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

#### M2.10 Ryzyka

| # | Ryzyko | Skutek | Co je ogranicza |
|---|---|---|---|
| R1 | v1 odrzucona albo zmieniona w recenzji | edytor później albo inaczej | warstwa sterownika (§3.7), tabela zmian niżej, D47 |
| R2 | `hwmon-next` nie wejdzie do najbliższego wydania jądra albo zmieni się po drodze | użytkownicy długo bez sterowania | wykrywanie po atrybutach; M2a działa na każdym jądrze |
| R3 | dystrybucje dostaną jądro z tymi zmianami z opóźnieniem | mało testujących poza właścicielem | README i strona mówią wprost, czego potrzeba |
| R4 | CPU_FAN1 i pompa bez sprawdzonego zapisu (O5, O3; złącze pompy puste) | na MS-7E61 pompa bez sterowania, CPU dopiero po D53 | D16, D52, D53 |
| R5 | temp1 nie nadąża za Tctl albo staje poniżej progu (O10) | B4 spóźnione albo martwe dla procesora | D48, D53; procesor dławi się sam przy Tjmax |
| R6 | wolne dochodzenie w automacie na CPU (zmierzone ok. 3,3 jednostki/s, ze 124 do 255 w ok. 40 s; pompa niezmierzona, O15) | wolne chłodzenie po B4 | D48 (100 % od razu) |
| R7 | wyładowanie modułu w trybie ręcznym (bez serii Dahlina) | stałe wypełnienie bez nadzoru do załadowania albo restartu | minimum B5, D49, ryzyko opisane w §4 |
| R8 | zawieszenie całego jądra | EC trzyma ostatni tryb | minimum B5 i B14 (ryzyko szczątkowe, §4) |
| R9 | polkit przez pośrednika Flatpaka działa inaczej niż natywnie (O19) | pytanie o hasło przy każdej zmianie albo brak sterowania z Flatpaka | test w kroku 13, opis w README, instalacja natywna jako zapas |
| R10 | Flathub nie przyjmie aplikacji zależnej od pakietu gospodarza (D44) | dystrybucja tylko z własnego repozytorium Flatpak i COPR | aplikacja działa bez pomocnika (podgląd) |
| R11 | koszt CPU wykresów ponad D29 | kryterium M2a niespełnione | pamięć podręczna węzłów, odczyt krzywych co 10 s, bez animacji ciągłych, pomiar w krokach 3 i 4 |
| R12 | własny widżet słabo dostępny dla czytnika ekranu | gorsza obsługa z Orca | lista punktów jako pełna droga, ogłaszanie zmian, test z Orca w krokach 4 i 17 |
| R13 | inne programy sterujące wentylatorami (fancontrol, CoolerControl) | wzajemne nadpisywanie | B10 bez nadpisywania; we Flatpaku procesów nie widać (§10.3) |
| R14 | aktualizacja BIOS-u zmienia domyślne krzywe albo zachowanie EC | inne krzywe pierwotne, inne przypisania | zasady nie zależą od wersji BIOS-u, B18 bierze migawkę od nowa; test właściciela po aktualizacji (M5) |
| R15 | czas właściciela na testy na sprzęcie, każdy za zgodą | wydania się przesuwają | testy zebrane w krokach 13 i 18, z gotowymi poleceniami |

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

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` w buildzie debug i release (Z2: kod pod `cfg(debug_assertions)` ukrywa w debug martwe pola, które zostają w release), `meson test`.
- Dodatkowe linty jako błąd (Z1, `[workspace.lints]` w `Cargo.toml`): `too_many_lines` z progiem 150 linii (`clippy.toml`), `allow_attributes_without_reason`, `match_same_arms`, `option_option`, `ref_option`, `needless_pass_by_value`, `unnecessary_wraps`, `unused_self`, `redundant_else`, `unused_qualifications`. Świadomy wyjątek to `#[expect(…, reason = "…")]` przy kodzie. Bez całego `clippy::pedantic` i `unreachable_pub` (szum).
- Bez nieużywanych zależności (Z4): `#![warn(unused_crate_dependencies)]` w `core` i w aplikacji.
- Publiczne API `core` bez martwych elementów (Z3, test meson `core-api`, `tests/core_api.py`): każdy element `pub` jest używany poza testami, przez aplikację albo przez sam `core`, albo stoi w `tests/core-api-exceptions.txt` z krokiem planu, który go potrzebuje (np. `fans::driver::read` dla `GetState` pomocnika w M2b). Nazwa, która jest zwykłym słowem, może ukryć nieużywany element; sprawdzenie nigdy nie zgłasza używanego.
- Zasoby i style bez sierot (Z6, test meson `resources`, `tests/resources.py`): każdy plik z `baseboard.gresource.xml` jest używany, każda klasa z `style.css` jest gdzieś ustawiana, a każda ustawiana klasa ma regułę albo jest klasą GTK lub libadwaita.
- Tłumaczenia (Z7, test meson `translations`, `tests/translations.sh`): każdy język z `po/LINGUAS` przechodzi `msgfmt --check`, jest kompletny, bez wpisów fuzzy i przestarzałych (`#~`); po `baseboard-update-po` jeszcze `msgattrib --no-obsolete -o po/pl.po po/pl.po`.
- `desktop-file-validate`, `appstreamcli validate`, `glib-compile-schemas --strict`.
- Przejście przez aplikację (Z10, `tests/tour.sh`, test meson `tour`, tylko build debug): aplikacja na niewidocznym ekranie (Xvfb, prywatna magistrala sesji bez usług, własne katalogi, ustawienia w pamięci) na każdym drzewie z `tests/fixtures/`, w oknie gęstszym (1200 px), z kartami w rzędach (1024 px) i najwęższym (360 px), a na kopii drzewa v1 z procesorem przy 93 °C w najszerszym (1850 px), każdą szerokość po angielsku i po polsku, klika każdy wiersz paska bocznego, każdą kartę, każdy kafelek i każdy klikalny wiersz Przeglądu, Wentylatorów i Systemu i sprawdza po ich tekście, co się otworzyło (strona, podstrona, grupa na górze „Wszystkich czujników”); sprawdza, że zdanie stanu Przeglądu i wiersz stanu Systemu mówią to, co pokazuje drzewo („Wszystko w normie”, przy 93 °C „Procesor jest bardzo gorący”), a zdanie stanu jest klikalne tylko przy problemie; na każdej stronie, podstronie i w Preferencjach sprawdza, że żaden tekst nie jest ucięty (etykieta z wielokropkiem albo napis, który potrzebuje więcej wierszy, niż ma; D67); z menu głównego otwiera Preferencje (przełącznik „Nazwy z lm-sensors” zmienia ustawienie, „Widoczne odczyty” otwiera podstronę, a przełącznik w niej ukrywa odczyt), skróty klawiszowe i okno „O programie”; kopiuje informacje o systemie i sprawdza tekst w schowku. Klikalny element bez oczekiwania w `crates/app/src/tour.rs` to błąd testu, tak samo brak elementu, którego test oczekuje (karty, kafelki i wiersze Przeglądu, liczba wentylatorów z krzywą w danym drzewie). „Własne nazwy” otwierają zewnętrzną aplikację, więc test sprawdza tylko, że wiersz jest.
- Ostrzeżenie albo błąd GTK lub GLib to błąd testu (Z11): w przejściu przez aplikację i w testach z widżetami (`gtk_test::run`).
- Żaden test nie pomija się po cichu (Z12): `meson test` ustawia `BASEBOARD_TESTS_STRICT=1`, wtedy brak ekranu, narzędzia albo biblioteki (`glib-compile-schemas`, libsensors, libudev, polskiej lokalizacji w `tour`) to błąd (`test_env::skip`); przy zwykłym `cargo test` test tylko to wypisuje. Brak sprzętu nie jest błędem: test czytający prawdziwy `/sys` wypisuje jawnie „not applicable” i powód (`test_env::not_applicable`).
- Specyfikacja osobno od historii (Z13, test meson `work-status`, `tests/work_status.py`): `PROJEKT.md` opisuje, jak aplikacja działa dziś i co jest w planie; decyzje są w `DECYZJE.md` (zastąpione oznaczone „zastąpione przez Dxx”), przebieg prac w historii gita; „Stan prac” to lista (wydane, w toku, otwarte przed następnym wydaniem, później) do 250 słów.
- Każdy nowy test raz pokazany na zepsutym kodzie (Z9): wiadomość commita mówi, jak sprawdzono, że test nie przechodzi bez zmiany.
- CI na GitHubie (Z15, `.github/workflows/ci.yml`): przy każdym pushu i PR na `main`, w kontenerze Fedory 45 (te same wersje GTK i libadwaita co na komputerze właściciela, te same czcionki) na runnerze przypiętym do wersji systemu (`ubuntu-24.04`), tak jak akcje do commitów: `cargo fmt --check`, `clippy` debug i release, `meson test` z `tour` pod Xvfb. Runner nie ma płyty aplikacji, więc testy czytające prawdziwy `/sys` (libsensors, libudev) przechodzą jeszcze raz z wypisanym wyjściem, w którym widać, co tam nie dotyczy. Push robi właściciel; wynik przebiegu sprawdzam po pushu.
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
- wiersz biblioteki lm-sensors mówi „Wersja 3.6.2: odczyty bez konfiguracji lm-sensors”, a o Flatpaku mówi tylko wiersz konfiguracji (D66);
- tekst „Kopiuj informacje” dostaje pole ze sposobem instalacji (`Flatpak` z nazwą runtime'u albo `native`), także w instalacji natywnej.

Podsumowanie w metainfo i w pliku `.desktop` nie obiecuje sterowania, dopóki go nie ma: „MSI motherboard sensors and fans” (D58). Opis w metainfo, README i na stronie projektu przepisujemy w kroku wydania (§10.7).

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
| F1 (zrobione) | manifest `build-aux/flatpak/me.bojanowski.Baseboard.json` (miejsce przyjęte w aplikacjach GNOME), moduł libsensors, źródła Cargo `build-aux/flatpak/cargo-sources.json`, budowanie lokalne `flatpak-builder --user` bez sieci dla Cargo, zmiany z §10.3, porównanie z wersją natywną, `strace`, sekcja README |
| F2 (zrobione: wydanie 0.1.0) | wymagania Flathub z listy niżej poza F3, publikacja w repozytorium Flatpak flatpak.bojanowski.me (D43, procedura w §10.7) |
| F3 (zgłoszenie na Flathub po wydaniu 0.4.0, bez czekania na M2c: D44, D51) | manifest Flathub i zgłoszenie przygotowuje i prowadzi właściciel sam (zasady Flathub wobec AI), repozytorium zgłoszenia, uzasadnienie uprawnień z §10.1, domena bojanowski.me i weryfikacja ID; tylko za zgodą właściciela |

Wymagania Flathub spełnione i sprawdzane przy każdym wydaniu (§10.7): budowanie bez sieci (źródła Cargo w manifeście), `<releases>` z wersją i datą, ta sama wersja w `meson.build`, `Cargo.toml` i metainfo z podpisanym tagiem, zrzuty pod stałymi publicznymi adresami (D43), działająca strona domowa, `summary` najwyżej 35 znaków, `<branding>`.

Wymagania Flathub, których dziś nie spełniamy:

| Wymaganie | Dziś | Krok |
|---|---|---|
| Domena z App ID, bojanowski.me: linter Flathub wymaga odpowiedzi https://bojanowski.me (`appid-url-not-reachable`), weryfikacja ID plikiem `https://bojanowski.me/.well-known/org.flathub.VerifiedApps.txt` (przekierowania dozwolone, HTTPS obowiązkowe) albo rekordem DNS TXT `_flathub.bojanowski.me` | domena główna odrzuca połączenie (sprawdzone 2026-11-11); strona Baseboard jest pod baseboard.bojanowski.me | F3 (D44) |
| Uzasadnienie uprawnienia `/run/udev:ro` i zmiennej `SENSORS_SYSFS_ROOT` | tylko w tym projekcie; linter ich nie zgłasza, oceniają je moderatorzy | F3 |
| Manifest Flathub bez treści generowanych przez AI, zgłoszenie otwierane i prowadzone przez właściciela, ujawnienie AI (zasady Flathub od 2026-09); źródło git z `tag` i `commit` (typ `dir` jest w Flathub błędem) | manifest lokalny (`dir`) służy tylko do budowania własnego; w repozytorium nie ma plików pod Flathub | F3, właściciel |

### 10.7 Wydanie (procedura)

Ta sama procedura co dla openExpress i Vumi, które korzystają z tego samego repozytorium Flatpak (D43). Wydanie robimy tylko na prośbę właściciela, z notatkami wydania przez niego zaakceptowanymi. Push `main`, tag, wydanie na GitHubie i publikacja w repozytorium Flatpak tylko na jego wyraźne polecenie. Instalacji Flatpaka właściciela nie zmieniamy: żadnego `flatpak install`, `update`, `uninstall` ani `remote-add` na jego koncie; własne próby tylko na gałęzi `devel` w osobnym katalogu Flatpak (`FLATPAK_USER_DIR`, `FLATPAK_SYSTEM_DIR`, `FLATPAK_CONFIG_DIR` w `~/.cache`). W poleceniach `$HOME`, nie `~`, bo po „=” bash tyldy nie rozwija.

1. **Wersja.** `meson.build`, `Cargo.toml` (workspace) i nowy `<release>` na początku listy w metainfo: data dnia tagowania, krótki opis po angielsku (co działa, czego jeszcze nie ma), przetłumaczony w `po/pl.po`. Zgodność trzech miejsc sprawdza meson przy konfiguracji. Zrzuty metainfo nowego wydania idą do nowego katalogu `website/screenshots/<wersja>/` (okno 1000×700, ustawienia domyślne, jasny styl i jeden ciemny, angielski i polski), stare zostają. Opis w metainfo (`<description>`, `<caption>`), README i strona projektu (EN i PL) mówią o tym, co umie wydanie; stan łatek na stronie i w README sprawdzony w drzewie jądra (`git merge-base --is-ancestor`). Po zmianie `Cargo.lock` `build-aux/flatpak/cargo-sources.json` od nowa: `flatpak-cargo-generator.py` z `flatpak/flatpak-builder-tools`, commit `9d86d336a39b9af38b86f114b6ce5820fe260f91`, SHA-256 skryptu `0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380`, uruchomiony poza repozytorium. Sprawdzenia: lista „gotowe” z `CLAUDE.md` (§8), `appstreamcli validate --pedantic --no-net` na zbudowanym metainfo (dopuszczalne tylko `cid-contains-uppercase-letter`), linter Flathub `appstream`, audyt drzewa (ścieżki domowe, notatki, dane prywatne); commit.
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

Decyzje z datami są w `docs/DECYZJE.md`; odwołania „Dxx” w tym dokumencie prowadzą tam.

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
| O16 | Ostateczny kształt ABI zapisu krzywych po recenzji v1 i łatki 5 | lista linux-hwmon | M2b, M2c |
| O17 | Czy krzywa zapisana przez v1 przetrwa hibernację (S4) i ciepły restart przez kexec | test za zgodą | M2b |
| O18 | RGB z Flatpaka: hidraw jest dostępny tylko przez `--device=all`, które odsłania też `/dev/port` i inne urządzenia. Czy RGB przenieść do pomocnika (§3.4), czy wymagać instalacji natywnej, czy przyjąć `--device=all` | decyzja przed M3 | M3 |
| O19 | Czy polkit z `auth_admin_keep` (D3) działa dla wywołań z Flatpaka przez pośrednika D-Bus tak jak dla aplikacji natywnej (jedno hasło na kilka minut); zapamiętanie autoryzacji może dotyczyć procesu pośrednika | test w kroku 13: natywnie i we Flatpaku, ile razy w ciągu 5 minut pojawia się pytanie o hasło (§7 M2, M2.6) | M2b |
