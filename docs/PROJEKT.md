# Projekt: natywna aplikacja GNOME do monitoringu i sterowania płytą główną

Wersja 0.6 · 2026-11-10 · status: decyzje D1–D20 podjęte (§12)

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
| Sterowanie wentylatorami w jądrze | Sterownik z jądra Fedory 7.3.3: `pwm1–4` tylko do odczytu (0444), brak `pwmN_enable` i krzywych. Seria łatek do `nct6683` z `hwmon-next` (`hwmon-next` stoi na 7.4-rc2, więc do jądra trafi w 7.5) daje na tej płycie `pwm1`, `pwm2`, `pwm13–16` z `pwmN_enable`; płyta ma customer ID 0x20D, które ta seria obejmuje. Sprawdzone modułem testowym 2026-11-06, łącznie z zapisem na `pwm16`. |
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
- **Czas życia:** startuje na żądanie. Kończy się po 30 s bezczynności, ale tylko wtedy, gdy żaden kanał nie jest w trybie ręcznym ustawionym przez pomocnika. Krzywe w EC nie potrzebują działającego pomocnika. Zamknięcie aplikacji nie zmienia ustawień wentylatorów.
- **Pętla nadzoru:** krok 1 s, tylko gdy któryś kanał jest w trybie ręcznym ustawionym przez pomocnika: progi awaryjne (B4), zatrzymany wentylator (B11), cudze zmiany (B10). Krzywej programowej nie liczy (D10). Sterownik odświeża odczyty mniej więcej raz na sekundę, więc częstszy krok nic nie daje.
- **Telemetria:** pomocnik jej nie przesyła. Aplikacja sama czyta obroty i temperatury z hwmon, a pomocnik wysyła tylko zmiany stanu.
- **Utwardzenie jednostki systemd:** `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateNetwork`, `RestrictAddressFamilies=AF_UNIX`, `SystemCallFilter=@system-service`, `WatchdogSec`. Zapis dozwolony tylko w katalogu urządzenia hwmon i w `/run/baseboard` (dziennik B2, krzywe pierwotne B18). Docelowo pusty `CapabilityBoundingSet`, bo pliki `pwm*` mają właściciela root i tryb 0644 (do potwierdzenia w M2b).

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
| `ApiVersion` | `u` | wersja w obrębie `Fans1`; rośnie przy każdej nowej metodzie lub właściwości (§10.4) |
| `HelperVersion` | `s` | wersja pakietu pomocnika, do strony System i zgłoszeń błędów (§10.4) |
| `BoardId` | `s` | klucz profilu (np. `ms-7e61`); pusty, gdy brak profilu |
| `ProfileDigest` | `s` | SHA-256 pliku profilu pomocnika; aplikacja porównuje go ze swoim (§10.4) |
| `DriverLevel` | `s` | najniższy poziom wspólny dla kanałów sterowalnych z profilu: `read-only`, `manual`, `manual+curve-view`, `curves`, `curves+source` (§5.5.2) |
| `Emergency` | `b` | blokada awaryjna aktywna (B4) |
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
| `AcknowledgeEmergency` | `() → ()` | zdejmuje blokadę awaryjną (dopiero gdy temperatura spadła o histerezę). Ustawień nie przywraca: aplikacja wysyła je ponownie. | control |

**Sygnały:**

| Sygnał | Argumenty | Kiedy |
|---|---|---|
| `ChannelChanged` | `(s key, a{sv} state)` | zmiana trybu, wypełnienia zadanego albo krzywej |
| `EmergencyEntered` | `(s sensor, d value_c, d threshold_c)` | przekroczenie progu albo błąd odczytu (B4) |
| `EmergencyCleared` | `()` | po `AcknowledgeEmergency` |
| `ControlLost` | `(s key, s reason)` | Baseboard przestał sterować kanałem: `external-change`, `read-back-failed`, `stall`, `sensor-error`, `resume-failed`, `driver-error` |

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
website/                    strona projektu (D41): HTML i CSS bez skryptów, angielska pod /, polska pod /pl/, zrzuty w img/
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

Zasady obowiązują od M2b (pierwszy zapis przez pomocnika). Testy odwołują się do ich numerów. Złagodzenie którejkolwiek wymaga osobnej decyzji zapisanej w tym dokumencie. Względem 0.5 zmienione są B2, B3, B5–B8, B10–B12 i B14, nowe B16–B18. Złagodzenia wymagające decyzji: usunięcie B3(c) (D13), wymóg 100 % na progu tylko dla procesora i pompy (D19) i niemalejące zamiast rosnących temperatury punktów krzywej (D31).

**Zachowanie sterownika, na którym opierają się zasady** (poziomy pewności w §5.5.1):
- Sterownik **nie ma minimum** wypełnienia, także dla pompy. Polityka B5 i B6 jest wyłącznie w pomocniku.
- Zapis punktu krzywej przycina temperaturę do 0–127 °C, wypełnienie do 0–255, a obie wartości do sąsiednich punktów. Przycięcie nie jest błędem: zapis się udaje, ale w EC trafia inna wartość.
- Krzywej o malejącym wypełnieniu nie da się zapisać przez sysfs: sterownik przycina wypełnienie do sąsiednich punktów. Gdy zapis się nie uda, kończy się `-EIO` i obowiązuje poprzednia krzywa.
- Zapis jednego punktu to jedna transakcja z EC dla całej krzywej (ok. 30 ms), z weryfikacją odczytem. Gdy EC zapisu nie przyjmie albo odczyt się nie zgadza, sterownik przywraca krzywą sprzed zapisu i zwraca `-EIO`. Zwraca też `-EIO` bez zapisu, gdy stan EC zmienił ktoś inny, a `-ETIMEDOUT`, gdy EC nie odpowiada na uzgadnianie.
- Zapis krzywej w trybie ręcznym zmienia krzywą w EC, ale wentylator zostaje przy wypełnieniu ręcznym. Nowa krzywa działa po `pwmN_enable = 2`.
- W automacie wyjście dochodzi do celu stopniowo. **Pomiar** na SYS Fan 1 (`pwm16`) przy jednym ustawieniu Step Up/Down w BIOS-ie (0,1 s): ok. 5 jednostek na sekundę (ok. 2 %/s) w górę i w dół, czyli pełny zakres w ok. 50 s. Dla pozostałych kanałów (CPU Fan 1 ma w BIOS-ie 0,3 s) i innych ustawień Step Up/Down tempa nie mierzyliśmy (O15). W trybie ręcznym zmiana jest natychmiastowa.
- Po uśpieniu (S3) **potwierdzone jest tylko to, że BIOS cofa tryb ręczny**: po wznowieniu EC jest w automacie. Czy BIOS nadpisuje też krzywe, na tej płycie nie da się rozróżnić: krzywa zapisana przez sterownik z v1 była po S3 taka jak przed uśpieniem, ale mógł ją zachować EC albo odtworzyć sterownik (O4). Sterownik z `7dc9dff9608d` odtwarza tryb ręczny, a v1 zapisane krzywe, gdy EC się od nich różni; starszy sterownik nie odtwarza niczego. Po restarcie komputera EC ma ustawienia BIOS-u.
- Przy wyładowaniu sterownik niczego nie przywraca: ostatni tryb i krzywa zostają w EC do restartu.

| # | Zasada |
|---|---|
| B1 | **Tylko pomocnik pisze do `pwm*`**, w tym do `pwmN_auto_point*` i `pwmN_auto_channels_temp`. Aplikacja, testy i skrypty developerskie nie piszą do sysfs. |
| B2 | **Dziennik.** Przed pierwszą zmianą kanału pomocnik zapisuje w `/run/baseboard/claimed` klucz kanału i rodzaj zmiany (`manual`, `curve`, `source`), a przed pierwszą zmianą krzywej lub źródła także krzywą pierwotną (B18). Dopiero potem pisze do sysfs. Tryb ręczny: najpierw `pwmN_enable = 1`, potem `pwmN`. Zawsze w tej kolejności, bo `pwmN` przyjmuje zapis tylko w trybie ręcznym. |
| B3 | **Powrót do automatu (`pwmN_enable = 2`)** dla każdego kanału w trybie ręcznym ustawionym przez pomocnika: (a) przy normalnym zakończeniu (SIGTERM; z bezczynności pomocnik w tym stanie się nie kończy); (b) przy awarii (panic z `panic = "abort"`, SIGKILL, OOM, przekroczenie watchdoga). Wtedy przywraca `ExecStopPost=baseboard-helper --restore-auto`, które czyta dziennik B2 i nie potrzebuje D-Bus; (c) *usunięte w 0.6* (przed uśpieniem; zastępuje je B12, D13); (d) przed wyłączeniem i restartem (`PrepareForShutdown`); (e) po przekroczeniu progu awaryjnego (B4); (f) po błędzie odczytu temperatury lub obrotów. **Krzywych w EC B3 nie cofa:** działają same, są bezpieczne z założenia (B14) i do restartu komputera zostają (D4). Po awarii pomocnik **nie** wznawia trybu ręcznego sam. Aplikacja pokazuje komunikat i czeka na decyzję użytkownika. |
| B4 | **Próg awaryjny.** Gdy temperatura z listy progów przekroczy próg w 3 kolejnych odczytach, pomocnik przełącza **wszystkie** kanały w trybie ręcznym ustawionym przez Baseboard w automat i włącza blokadę. Do jej zdjęcia trzeba jawnej akcji (`AcknowledgeEmergency`), a temperatura musi spaść o co najmniej 10 °C poniżej progu. Wartość nieodczytywalna lub niewiarygodna (< 0 °C, > 125 °C, brak pliku) przez 3 odczyty działa jak przekroczenie progu. Kanały z krzywą w EC już są w automacie: krzywe procesora i pompy osiągają 100 % najpóźniej na progu (B14), krzywe wentylatorów systemowych nie muszą (D19), a B4 ich nie zmienia. Po przejściu na automat wyjście dochodzi do celu stopniowo; na SYS Fan 1 zmierzono ok. 5 jednostek/s (z 30 % do 100 % w ok. 36 s), na pozostałych kanałach tempo jest nieznane (D14, O15). |
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

**Ryzyko szczątkowe:** jeśli zawiesi się całe jądro, EC dalej wykonuje ostatni tryb, bo sam nie ma watchdoga. W trybie ręcznym oznacza to stałe wypełnienie aż do restartu, w automacie ostatnią krzywą. Dlatego minimum B5 i zasada B14 są twarde.

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
  - sterownik `nct6683`: załadowany czy nie (jest na liście `/proc/modules` albo jest urządzenie hwmon z profilu), moduł z jądra czy spoza niego (D40: flaga `O` w nawiasie na końcu linii modułu w `/proc/modules`; linia bez flagi `O` to moduł z jądra; brak modułu na liście przy obecnym urządzeniu to sterownik wbudowany w jądro, czyli też z jądra; gdy listy nie da się odczytać, „pochodzenie modułu nieznane”, nigdy zgadywanie; lista jest czytana przy każdym odczycie strony, więc po przeładowaniu modułu pokazuje stan bieżący; tylko informacja, poziomu obsługi z tego nie wyliczamy, §5.5.2), poziom obsługi wentylatorów (`DriverLevel`, §5.5.2). Gdy jądro nie pozwala sterować wentylatorami, tekst mówi, że sterowanie wymaga nowszego jądra z rozszerzonym sterownikiem `nct6683`, bez numeru wersji jądra,
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
| **Przyjęte w `hwmon-next`** (do jądra 7.5; `hwmon-next` stoi na 7.4-rc2) | seria Hamzy: tryb ręczny dla kanałów CPU, pompy i SYS przy customer ID 0x20D; `e0fdadf44480`: odczyt krzywych; `7dc9dff9608d`: odtworzenie trybu ręcznego po wznowieniu | `pwm1`, `pwm2`, `pwm13–16`: `pwmN` i `pwmN_enable` 0644 (1 = ręczny, 2 = automat z bieżącą krzywą); `pwmN_auto_point[1-7]_temp` (milistopnie), `_pwm` (0–255) i `pwmN_auto_channels_temp` (maska `tempX`) 0444 |
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
| `manual` | `mode` i `duty-write`, bez `curve-read` | jądro z samą serią Hamzy |
| `manual+curve-view` | dodatkowo `curve-read` | `hwmon-next` |
| `curves` | dodatkowo `curve-write` | `hwmon-next` z serią v1 (moduł testowy) |
| `curves+source` | dodatkowo `source-write` | po łatce 5 |

- Po atrybutach nie widać, czy sterownik odtwarza stan po wznowieniu. B12 działa więc tak samo dla każdego sterownika.

#### 5.5.3 Co widzi użytkownik

| | Sterownik Fedory 7.3.3 (`read-only`) | `hwmon-next` (`manual+curve-view`) | Nasz moduł z v1 (`curves`) |
|---|---|---|---|
| Przegląd | obroty; wypełnienie procesora, pompy i wentylatora dodatkowego; SYS 1–4 bez wypełnienia | obroty i wypełnienie wszystkich złączy; tryb (automat, ręczny) w podtytule | jak przy `hwmon-next` |
| Strona Wentylatory | karty tylko do odczytu, bez przełącznika trybu. Jeden wiersz z wyjaśnieniem: sterowanie wymaga nowszego jądra z rozszerzonym sterownikiem `nct6683`. Interfejs nie podaje numeru wersji jądra. | przełącznik „Automat BIOS / Ręczny”; w automacie miniatura krzywej z EC i jej źródło; podstrona z krzywą tylko do podglądu i dopiskiem, że edycja wymaga sterownika z zapisem krzywych | przełącznik „Automat BIOS / Ręczny / Krzywa”; „Krzywa” otwiera edytor 7 punktów; źródło temperatury tylko do podglądu |
| Pomocnik | niepotrzebny, nie startuje | tryb ręczny (hasło raz na kilka minut, D3) | tryb ręczny i krzywe |
| Baner „Ustawienia działają do restartu komputera” | nigdy | gdy któryś kanał jest w trybie ręcznym ustawionym przez Baseboard | jak obok albo gdy któryś kanał ma krzywą inną niż pierwotna |
| Strona System, poziom obsługi | „Sterowanie wentylatorami niedostępne” z wyjaśnieniem jak wyżej | „Tryb ręczny i podgląd krzywych” | „Krzywe w EC” |

Nieistniejące `pwm3` jest w każdym przypadku tylko do odczytu. Wentylator dodatkowy (EZ Conn, `pwm4`) jest tylko do odczytu, dopóki sterownik nie obejmie tego kanału (`pwm4_enable`) i zapis nie zostanie sprawdzony na sprzęcie (D32). Oba z wyjaśnieniem.

#### 5.5.4 Strona Wentylatory

- **Karta złącza:** nazwa z profilu (§6.2) i złącze, obroty, wypełnienie (bieżące, a w automacie także cel z krzywej, dopóki wyjście do niego dochodzi) oraz tryb wybierany przełącznikiem segmentowym (AdwToggleGroup). Przy szerokim oknie karty stoją w dwóch kolumnach.
- **Tryby w interfejsie a sterownik** (D11):

  | Przełącznik | `pwmN_enable` | Krzywa w EC |
  |---|---|---|
  | Automat BIOS | 2 | pierwotna (B18); gdy była zmieniona, wybranie zapisuje ją z powrotem, bez walidacji |
  | Ręczny | 1 | bez zmian |
  | Krzywa | 2 | zmieniona przez użytkownika |

  „Krzywa” jest widoczna tylko przy `curve-write`. Przy samym `curve-read` karta w automacie pokazuje krzywą z EC do podglądu.
- **Ręczny:** suwak w karcie z dolną granicą równą minimum (B5). Zakres poniżej minimum jest zakreskowany. Zmiana działa od razu.
- **Krzywa:** karta pokazuje miniaturę krzywej i bieżący punkt pracy. Kliknięcie karty otwiera podstronę (AdwNavigationPage) z edytorem:
  - dokładnie tyle punktów, ile podaje sterownik (7). Ostatnie punkty mogą być kopią poprzedniego, tak jak BIOS wypełnia punkty 5–7 przy czterech poziomach;
  - temperatury i wypełnienia niemalejące. Edytor nie pozwala zapisać krzywej łamiącej B5, B6, B7 i B14 (dla procesora i pompy 100 % najpóźniej na progu awaryjnym źródła; dla wentylatorów systemowych bez tego wymogu, D19);
  - **krzywa z EC, która łamie zasady** (np. pierwotna z BIOS-u, §6.3): edytor otwiera się i pokazuje ją bez zmian. Przy każdym punkcie, który trzeba zmienić, stoi ikona i tekst z powodem, np. „Poniżej minimum 30 %” albo „Przy 95 °C wymagane 100 %”. „Zastosuj” jest nieaktywne, dopóki krzywa nie spełni zasad. Edytor sam niczego nie poprawia;
  - źródło temperatury tylko do podglądu (nazwa jak w Przeglądzie); wybór dopiero przy `source-write` (M2d, D17);
  - wykres z bieżącym punktem pracy, wartościami poza skrajnymi punktami i progiem awaryjnym źródła;
  - „Zastosuj” wysyła krzywą do pomocnika (polkit, D3), „Cofnij zmiany” wraca do krzywej z EC, „Przywróć krzywą BIOS” do pierwotnej (B18);
  - po zastosowaniu dopisek, że wentylator dochodzi do nowego wypełnienia stopniowo; bez liczby, dopóki O15 nie da pomiaru dla wszystkich kanałów.
- **Kanał w trybie ręcznym:** zapis krzywej nie zmienia wypełnienia. „Zastosuj” ustawia więc także automat (`activate`), o czym edytor mówi przed zapisem.
- **Baner (AdwBanner)** „Ustawienia działają do restartu komputera” (D4) z przyciskiem „Przywróć automat BIOS” (`SetAllAuto`: krzywe pierwotne i automat na wszystkich kanałach zmienionych przez Baseboard). Kiedy jest widoczny: §5.5.3; widać go także na podstronie krzywej. Pasek nagłówka nie ma osobnego przycisku. Opis strony dodaje, że ustawienia działają także po zamknięciu aplikacji.
- **Pompa:** stałe ostrzeżenie w karcie, dopisek o niesprawdzonym przypisaniu złącza (`verified = false`) i brak możliwości zatrzymania (B6), także w krzywej.
- **Błędy sterownika:** `DriverRejected` → „Kontroler płyty nie przyjął ustawień. Obowiązuje poprzednia krzywa.”, z odczytaną krzywą; `Timeout` → „Kontroler płyty nie odpowiedział.” Bez kodów błędów w interfejsie; szczegóły w dzienniku (B15).
- **Makiety do zmiany w M2a:** edytor z 7 punktami zamiast 2–8, stan `read-only` i `manual+curve-view`, przełącznik dwupozycyjny.

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
| M2 | Wentylatory | M2a podgląd (bez pomocnika i zapisu), M2b pomocnik i tryb ręczny, M2c krzywe w EC, M2d źródło temperatury (po łatce 5) | tak od M2b (pomocnik) | M2a: brak. M2b: sterownik z `pwmN_enable` (`hwmon-next` albo moduł testowy; w jądrze od 7.5). M2c: sterownik z v1 (moduł testowy, §1.4) |
| M3 | RGB | hidraw, raporty 0x50/0x51, odczyt wersji firmware, reguła udev w pakiecie RPM | tak, raz (reguła udev) | brak |
| M4 | Wydajność + restart do UEFI | profile energii, podgląd cpufreq, restart do UEFI, profil energii w „Kopiuj informacje” | nie | tuned-ppd lub power-profiles-daemon, logind |
| M5 | Aktualizacja BIOS-u bez flashowania z systemu | sprawdzenie wersji z warunkami dopasowania i wynikiem trójstanowym (opcja, domyślnie wyłączona), pobranie z sumą kontrolną, pendrive FAT32, restart do UEFI, lista kontrolna | nie | sieć tylko na żądanie, UDisks2, logind |

**Stan prac** (jedyne miejsce w dokumencie; aktualizowany w commicie, który kończy krok): M1 ukończony (2026-11-11, kroki 1–13, wynik kryteriów w wierszu kroku 13); M2 nie rozpoczęty. Flatpak (D39, D40): krok F1 zrobiony 2026-11-11 (manifest w `build-aux/flatpak/`, libsensors, źródła Cargo bez sieci, zmiany dla piaskownicy, porównanie z instalacją natywną w §10.3, `strace` w piaskownicy bez otwarć do zapisu w `/sys`, przeładowanie modułu przy działającej aplikacji z Flatpaka sprawdzone z właścicielem, sekcja README); następny jest F2 przed pierwszym wydaniem (§10.6). Strona projektu (2026-11-11, D41): `website/` po angielsku i po polsku, ze zrzutami aplikacji i sekcją o pracy w jądrze, wdrażana przez GitHub Pages (`.github/workflows/website.yml`); w README sekcja „Kernel work” i adres strony bez dopisku „coming soon”; strona działa pod https://baseboard.bojanowski.me, Pages włączone (2026-11-11: źródło GitHub Actions, domena własna, wymuszone HTTPS). F2, wydanie 0.1.0 (D42–D44): w toku. Publikacja (2026-11-11): repozytorium na GitHubie zaczyna się od jednego commita początkowego; historia sprzed publikacji w archiwum lokalnym. Przed publikacją zasady lokalne, ścieżki do notatek i plan budowania modułu testowego przeszły do plików poza repozytorium, rendery makiet (`makiety/png/`, tworzy je `makiety/renderuj.sh`) wypadły z repozytorium, doszły plik LICENSE (GPL-3.0), nagłówki SPDX i adresy: strona https://baseboard.bojanowski.me, zgłoszenia i kod na GitHubie (`config.rs`, metainfo, okno „O programie”, README). Ikona docelowa wstawiona w miejsce roboczej (2026-11-11: `data/icons/hicolor/scalable/apps/me.bojanowski.Baseboard.svg` i `…/symbolic/apps/me.bojanowski.Baseboard-symbolic.svg`; symboliczna przyjmuje kolor tekstu w jasnym i ciemnym motywie; tego samego dnia symboliczna zastąpiona poprawioną, bo poprzednia była wypełnionym kwadratem i w pasku bocznym wyglądała jak klocek, a nowa to ramka z elementami w środku). Poza krokami (2026-11-11): dopasowanie profilu po nazwie modelu (D28), kryterium CPU (D29), odtworzenie treści, które zginęły bez decyzji przy przejściu z 0.5 na 0.6 (tabela progów w §4, B2, B8, B10, §5.5.3, §5.5.4, testy M2), decyzje z tego przeglądu (D30–D34), wybory z kroku 10 (D35, D36), poprawki z przeglądu M1 (D37, D38). Otwarte z M1, przeniesione do M2: test z Orca (robi właściciel); pomiar CPU przy ukrytym oknie w buildzie release (w kroku 8 w buildzie debug 0,00 %; dziś okno nie dało się zminimalizować przez `BASEBOARD_DEBUG_MINIMIZED`, więc pomiar wymaga ręcznej minimalizacji) i na module Fedory (D29); zrzuty ekranu w metainfo, gdy będą publiczne adresy obrazów (kandydaci: zrzuty strony, §10.6), i `<releases>` przy pierwszym wydaniu. Zamknięte 2026-11-11: plik `.desktop` i metainfo przebudowują się po zmianie tłumaczeń (`custom_target` z `msgfmt` zależny od `po/LINGUAS` i plików `.po` zamiast `i18n.merge_file`, który zależał tylko od pliku `.in`). Do testu obciążeniowego M2b (O10): po 12 s pełnego obciążenia temp1 doszło do 92 °C przy progu 95 °C. Przed pierwszym wydaniem: sprawdzenie znaku towarowego w bazach (D1; kolizje z oprogramowaniem sprawdzone ponownie 2026-11-11).

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
- Koszt odczytu sprzętu podajemy osobno, jako pomiar, a nie kryterium. Na MS-7E61 to ok. 0,63 % jednego rdzenia dla `nct6683` i ok. 0,2 % dla pozostałych układów. To czas jądra w sterownikach, w `nct6683` dostęp do rejestrów EC przez porty I/O z aktywnym czekaniem: pierwszy odczyt po upływie sekundy odświeża wszystkie rejestry (ok. 6,3 ms), następne biorą je z pamięci podręcznej sterownika. Według kodu sterownik v1 czyta przy odświeżeniu 112 rejestrów, a sterownik Fedory 108 (bez wypełnień `pwm13–16`). Krzywe (`pwmN_auto_point*`) i `pwmN_enable` są poza cyklem odświeżania: każdy odczyt atrybutu czyta rejestr z EC, a aplikacja nie czyta ich przy każdej próbce.
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

**Warunki wstępne:** M1. M2a działa na każdym jądrze. M2b i M2c na sprzęcie wymagają modułu testowego (§1.4), dopóki łatki nie trafią do jądra Fedory. Każdy test zapisu na sprzęcie wymaga osobnej zgody właściciela (CLAUDE.md).

**M2a – podgląd (bez pomocnika, bez zapisu)**

| # | Krok | Wynik |
|---|---|---|
| 1 | `core::fans::driver`: odczyt trybu, wypełnienia, krzywej i źródła; cecha `FanDriver`, `SysfsDriver` i `ModelDriver` w części do odczytu | testy na trzech drzewach z M1 krok 4; na module v1 krzywe i źródła wszystkich sześciu kanałów zgodne z `cat` |
| 2 | Model krzywej w `core`: n punktów, wartość w punkcie pracy (do podglądu), źródło → klucz temperatury z profilu → próg awaryjny | testy jednostkowe |
| 3 | Strona Wentylatory tylko do odczytu: karty, wyjaśnienie przy `read-only`, miniatury i podstrona krzywej przy `curve-read`. Makiety zaktualizowane (§5.5.4). | strona działa na 7.3.3 i na module testowym |

**M2b – pomocnik i tryb ręczny**

| # | Krok | Wynik |
|---|---|---|
| 4 | Crate `helper`: aktywacja D-Bus, jednostka systemd, polityka magistrali, akcje polkit, `GetChannels`, `GetState`, `DriverLevel` | testy na prywatnej magistrali z atrapą polkit |
| 5 | Zapis trybu i wypełnienia w `SysfsDriver`, dziennik B2, `--restore-auto` (B3), weryfikacja B10 | testy z `ModelDriver` i na sztucznym sysfs; testy zabijania |
| 6 | Pętla nadzoru: B4, B11, B13; wybudzenie (B12), wyłączenie (B3 d), dziennik B15 | testy wstrzykiwania błędów |
| 7 | Interfejs trybu ręcznego: przełącznik, suwak z minimum, baner, `SetAllAuto`, komunikaty błędów | tryb ręczny przez pomocnika |
| 8 | Pakiet RPM z podpakietem `baseboard-helper` (§10). Test na sprzęcie za zgodą: SYS Fan 1, potem kanały według D16 | kryteria M2b |

**M2c – krzywe w EC**

| # | Krok | Wynik |
|---|---|---|
| 9 | Planer zapisu (B16) i walidacja krzywej (B7, B14) w `core` | testy właściwościowe: dla dowolnej pary krzywych każdy stan pośredni jest niemalejący i chłodzi co najmniej tak jak stara i nowa krzywa |
| 10 | Pomocnik: `GetEcCurve`, `SetEcCurve`, `RestoreOriginalCurve`, migawka B18, błędy B17, `SetAllAuto` z krzywymi | testy z `ModelDriver`: przycinanie, `-EIO` w połowie planu, `-ETIMEDOUT` |
| 11 | Edytor krzywej (podstrona), tryb „Krzywa”, „Przywróć krzywą BIOS” | edytor zgodny z makietą |
| 12 | Test na sprzęcie z modułem v1 za zgodą: pwm16, potem pozostałe SYS; S3 z krzywą; restart (krzywa BIOS-u po starcie) | kryteria M2c |

**M2d – źródło temperatury** (po łatce 5)

| # | Krok | Wynik |
|---|---|---|
| 13 | `source-write` w warstwie sterownika według przyjętej łatki 5, `SetEcSource`, wybór źródła w edytorze (temperatury EC z profilu), B14 dla nowego źródła | wybór źródła na module z łatką 5 |

**Kryteria ukończenia M2a**
- Strona Wentylatory działa na jądrze Fedory 7.3.3, na `hwmon-next` i z modułem v1, a każdy przypadek wygląda jak w §5.5.3.
- Krzywe na podglądzie są zgodne z `cat` atrybutów `pwmN_auto_point*`.
- Aplikacja niczego nie zapisuje (`strace` jak w M1).

**Kryteria ukończenia M2b**
- Na module testowym kanały `sys1`–`sys4` (oraz `cpu1` i `pump1` według D16) działają w trybie ręcznym, a obroty reagują. `ezconn` (D32) i `pwm3` są tylko do odczytu z wyjaśnieniem.
- Każda droga zakończenia z B3 (SIGTERM, SIGKILL, abort, zawieszenie pętli i watchdog, wyłączenie) zostawia każdy kanał w trybie ręcznym w `pwmN_enable = 2`, co sprawdzamy odczytem sysfs po zdarzeniu.
- Po S3 tryb ręczny jest taki jak przed uśpieniem: odtworzony przez sterownik albo przez B12.
- Minimum B5 i zakaz B6 nie dają się obejść żadnym wywołaniem D-Bus.
- Próg awaryjny działa na sprzęcie. Test bez przegrzewania: próg tymczasowo obniżony w `/etc` do 50 °C i krótkie obciążenie CPU.
- Wykrywanie zatrzymanego wentylatora (B11) działa: test na atrapie i ręcznie przez odłączenie wentylatora SYS, któremu profil testowy w `/etc/baseboard/boards` nadał rolę `cpu`.
- Bez autoryzacji metody `control` są odrzucane, a `restore-auto` przechodzi.
- Na jądrze Fedory 7.3.3 pomocnik niczego nie zapisuje, a strona pokazuje wyjaśnienie.
- `systemd-analyze security baseboard-helper.service` daje ocenę ≤ 5,0, a każde wyjątkowe uprawnienie jest opisane w pliku jednostki.

**Kryteria ukończenia M2c**
- Obroty podążają za krzywą zapisaną w EC pod obciążeniem. Krzywa działa dalej po zatrzymaniu pomocnika i zamknięciu aplikacji.
- Odczyt po każdym kroku planu jest równy planowi (planer nigdy nie wywołuje przycinania).
- „Przywróć krzywą BIOS” daje dokładnie krzywą pierwotną (porównanie odczytu).
- Po S3 krzywa i tryb są takie jak przed uśpieniem; po restarcie komputera obowiązuje krzywa BIOS-u.
- Krzywa łamiąca B5, B6, B7 albo B14 nie trafia do sterownika.

**Testy**
- Jednostkowe i właściwościowe (proptest) dla logiki z `core`: minimum dla `cpu` i `pump` przy dowolnym wejściu (w jednostkach 0–255, z obcięciem: 76 i 127), walidacja krzywych, planer B16 (dowolna para krzywych → każdy stan pośredni poprawny i chłodzący co najmniej jak stara i nowa), B14 dla `cpu` i `pump` oraz brak wymogu 100 % dla `sys` (D19), zapis krzywej pierwotnej łamiącej B5 (np. domyślna krzywa CPU z §6.3) bez walidacji, edytor z nieaktywnym „Zastosuj” dla takiej krzywej, limit tempa spadku wypełnienia w trybie ręcznym (B8).
- `ModelDriver` jako model sterownika: przycinanie temperatur i wypełnień do sąsiadów, `-EIO` z przywróceniem poprzedniej krzywej, `-ETIMEDOUT`, tempo w automacie jako parametr (testy dla kilku wartości, nie tylko zmierzonych 5 jednostek/s), uśpienie z odtworzeniem i bez.
- Integracyjne: pomocnik na prywatnej magistrali sesyjnej, ze sztucznym sysfs z trzech wariantów i atrapą polkit (szablon `polkitd` z python-dbusmock). Ta konfiguracja jest możliwa tylko w buildzie testowym.
- Wstrzykiwanie błędów na sztucznym sysfs: znikający plik temperatury, wartości niewiarygodne, nieudany zapis, zmiana `pwmN_enable` lub krzywej z zewnątrz, zerowe obroty.
- Testy zabijania: SIGKILL i SIGABRT pomocnika uruchomionego przez `systemd-run` ze sztucznym sysfs, potem sprawdzenie, że `--restore-auto` zadziałało.
- Ręczne na sprzęcie, każdorazowo za zgodą: kanały według D16, obciążenie, uśpienie i wybudzenie, wyłączenie, próg awaryjny, równoległy zapis innego narzędzia.
- Proste testy losowe argumentów D-Bus: złe typy, puste i długie tablice, nieznane klucze, krzywe z niewłaściwą liczbą punktów.

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
- **Brak pomocnika:** podgląd M2a działa bez pomocnika, bo czyta sysfs sam. Przy wejściu na stronę Wentylatory aplikacja odczytuje właściwość `ApiVersion`, co aktywuje pomocnika przez D-Bus. Błąd `ServiceUnknown` oznacza, że pomocnik nie jest zainstalowany: strona zostaje w podglądzie i mówi, że sterowanie wymaga pakietu `baseboard-helper`, z odnośnikiem do instrukcji. `AccessDenied` od pośrednika D-Bus Flatpaka oznacza odebrane uprawnienie: strona podaje polecenie, które je przywraca. Strona System ma wiersz „Pomocnik systemowy” (zainstalowany z wersją, brak, niezgodny), a tekst do schowka zawiera wersję pomocnika.
- **Zgodność wersji:** nazwa interfejsu niesie wersję główną (`Fans1`, §3.5). W jej obrębie zmiany są tylko zgodne wstecz, a `ApiVersion` rośnie przy każdej nowej metodzie lub właściwości. Aplikacja korzysta tylko z tego, co zgłasza `ApiVersion`, a resztę ukrywa z dopiskiem, że wymaga nowszego pomocnika. Zmiana niezgodna wstecz to `Fans2` obok `Fans1` przez co najmniej jedno wydanie. Nowa właściwość `HelperVersion` (`s`) daje wersję pakietu do strony System i zgłoszeń błędów.
- **Profile:** o sterowaniu rozstrzyga profil pomocnika (B9, D33): aplikacja pokazuje sterowanie tylko przy kanałach, które pomocnik zgłosił w `GetChannels` jako sterowalne. Profil aplikacji z Flatpaka służy do nazw i odczytów. Nowa właściwość `ProfileDigest` (`s`, SHA-256 pliku profilu) pozwala wykryć, że oba profile się różnią. Wtedy aplikacja trzyma się danych pomocnika i pokazuje dopisek o różnych wersjach profilu.
- **Zasady B1–B18:** wszystkie działają po stronie pomocnika i żadna nie wymaga, żeby aplikacja była z tego samego pakietu. Wspólną instalację zakładało tylko zdanie z §3.6, że aplikacja i pomocnik „liczą to samo” z tego samego `core`. Przy różnych wersjach wynik walidacji w aplikacji (B5, B7, B14) jest tylko podpowiedzią w interfejsie. Pomocnik i tak decyduje, a jego `InvalidArgument` lub `NotSupported` aplikacja pokazuje jako zwykły komunikat. B1 piaskownica dodatkowo wymusza, bo `/sys` jest w niej tylko do odczytu.

### 10.5 Instalacja natywna

1. M1: `meson setup build --prefix=…` i `meson install`. Do testów wystarczy prefiks w katalogu domowym.
2. Od M2b: `packaging/rpm/baseboard.spec` budowany lokalnie (`rpmbuild`, potem COPR), z podpakietem `baseboard-helper` (pomocnik, jednostka systemd, pliki D-Bus, akcje polkit). Zależności Rust dołączone przez `cargo vendor`, żeby budowanie działało bez sieci.
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
| F3 (zgłoszenie na Flathub, po M2: D44) | manifest Flathub i zgłoszenie przygotowuje i prowadzi właściciel sam (zasady Flathub wobec AI), repozytorium zgłoszenia, uzasadnienie uprawnień z §10.1, domena bojanowski.me i weryfikacja ID; tylko za zgodą właściciela |

Wymagania Flathub, których dziś nie spełniamy:

| Wymaganie | Dziś | Krok |
|---|---|---|
| Budowanie bez sieci: zależności Cargo jako źródła manifestu (`cargo-sources.json` z `flatpak-cargo-generator`) | zrobione w F1: `flatpak-cargo-generator.py` z oficjalnego `flatpak/flatpak-builder-tools`, commit `9d86d336a39b9af38b86f114b6ce5820fe260f91`, SHA-256 skryptu `0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380`, uruchomiony poza repozytorium; plik trzeba wygenerować od nowa po każdej zmianie `Cargo.lock` | F1 |
| `<releases>` w metainfo z wersją i datą | brak (`appstreamcli --pedantic`: `releases-info-missing`) | F2 |
| Wersja wydania i podpisany tag, zgodne w `meson.build`, `Cargo.toml` i metainfo | 0.1.0 bez tagu | F2 |
| Zrzuty ekranu w metainfo pod stałymi publicznymi adresami | brak w metainfo; zrzuty strony (`website/img/`) mają okno 1500×1000, a wytyczne Flathub chcą najwyżej 1000×700, więc w F2 nowe zrzuty pod https://baseboard.bojanowski.me/screenshots/0.1.0/ (D43) | F2 |
| Działająca strona domowa (walidator Flathub sprawdza adresy) | zrobione 2026-11-11: https://baseboard.bojanowski.me (D41, GitHub Pages) | F2 |
| Opis zgodny z tym, co aplikacja umie, `summary` najwyżej 35 znaków | zrobione w F1 (§10.3) | F1 |
| Kolory marki w metainfo (`<branding>`), zalecane | brak; w F2 #deddda i #3d3846 (D43) | F2 |
| Domena z App ID, bojanowski.me: linter Flathub wymaga odpowiedzi https://bojanowski.me (`appid-url-not-reachable`), weryfikacja ID plikiem `https://bojanowski.me/.well-known/org.flathub.VerifiedApps.txt` (przekierowania dozwolone, HTTPS obowiązkowe) albo rekordem DNS TXT `_flathub.bojanowski.me` | domena główna odrzuca połączenie (sprawdzone 2026-11-11); strona Baseboard jest pod baseboard.bojanowski.me | F3 (D44) |
| Uzasadnienie uprawnienia `/run/udev:ro` i zmiennej `SENSORS_SYSFS_ROOT` | tylko w tym projekcie; linter ich nie zgłasza, oceniają je moderatorzy | F3 |
| Manifest Flathub bez treści generowanych przez AI, zgłoszenie otwierane i prowadzone przez właściciela, ujawnienie AI (zasady Flathub od 2026-09); źródło git z `tag` i `commit` (typ `dir` jest w Flathub błędem) | manifest lokalny (`dir`) służy tylko do budowania własnego; w repozytorium nie ma plików pod Flathub | F3, właściciel |

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

Decyzje D1–D7 podjęte 2026-11-06, D8–D27 2026-11-10, D28–D44 2026-11-11. Numery etapów w D1–D8 są według numeracji z 0.6.

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
| O3 | Przypisanie PUMP_SYS1 → fan2/pwm2 i JAF_1 → fan4/pwm4 | wentylator podłączany kolejno do obu złączy | M1 (etykieta), M2b (sterowanie) |
| O4 | ~~Zachowanie `pwmN_enable` i wypełnienia po uśpieniu i po ciepłym restarcie~~. **Częściowo rozstrzygnięte (testy sterownika, 2026-11-09):** po S3 BIOS cofa tryb ręczny (EC w automacie); sterownik z `7dc9dff9608d` go odtwarza. Krzywa zapisana przez v1 jest po S3 taka jak przed uśpieniem, ale na tej płycie nie da się rozróżnić, czy zachował ją EC, czy odtworzył sterownik. Po restarcie EC ma ustawienia BIOS-u. Otwarte: czy BIOS przy S3 nadpisuje krzywe; S4 (hibernacja). | test S4 za zgodą | M2b |
| O5 | Zapis na CPU_FAN1 i PUMP_SYS1, w trybie ręcznym i w krzywej (dotąd sprawdzono tylko SYS_FAN1); dla kanałów 0–1 otwarte jest też uzgadnianie w sterowniku (notatki z projektu sterownika) | test za zgodą | M2b, M2c (D16) |
| O6 | Jak powiadamiać o zdarzeniach awaryjnych, gdy aplikacja nie działa (np. mały proces w tle z autostartem albo portal Background) | prototyp | M2b |
| O7 | Rzeczywiste umiejscowienie czujników „System” i „Chipset A” | długi odczyt z dysku podłączonego do chipsetu, albo odczyt konfiguracji kanałów przez sterownik | M1 (opis), M2b (wybór źródeł) |
| O8 | Czy pomocnik może działać z pustym `CapabilityBoundingSet` | test jednostki systemd | M2b |
| O9 | Progi awaryjne (B4) i minima (B5) | testy obciążeniowe, dane pompy, jeśli zostanie podłączona | M2b |
| O10 | Czy temp1 układu płyty (źródło progu CPU, D34) nadąża pod obciążeniem za Tctl z `k10temp` (opóźnienie i różnica) i czy osiąga próg 95 °C. Próg CPU równa się Tjmax (D5), a procesor dławi przy Tjmax, więc temp1 może zatrzymać się na progu albo poniżej i warunek „przekroczy” mógłby nigdy nie zadziałać. Wtedy warunek B4 dla CPU trzeba zmienić na „osiągnie”. Tctl służy tu tylko do porównania. | test obciążeniowy z celowo niskim wypełnieniem wentylatora CPU (za zgodą), z zapisem temp1 i Tctl co 1 s | M2b |
| O11 | Lista wydań BIOS-u: dokładna ścieżka na serwerze, pola, które czytamy (płyta, wersja, data, adres pliku), zapis wersji względem DMI `bios_version`. Zapis i znaczenie warunków dopasowania (zakres wersji, wersje pominięte, wykluczenia). Czy lista podaje sumę kontrolną i jakiego rodzaju, a jeśli nie, skąd wziąć sumę wzorcową. | notatki z analizy oprogramowania producenta (§2) oraz jednorazowe pobranie listy, za zgodą | M5 |
| O12 | Wymagania wbudowanego narzędzia aktualizacji w UEFI: tablica partycji (MBR czy GPT), FAT32 a rozmiar nośnika, położenie i nazwa pliku, czy plik trzeba rozpakować z archiwum | instrukcja płyty i test z pendrivem, bez zatwierdzania flashowania | M5 |
| O13 | Odczyt wersji firmware kontrolera RGB na MS-7E61: czy kontroler odpowiada, format odpowiedzi, czy pole `bcdDevice` urządzenia USB (dziś 0001) ma z nią związek | test na sprzęcie (tylko odczyt) | M3 |
| O14 | ~~Nazwa producenta modułu pamięci, gdy SMBIOS podaje „Unknown”~~. **Rozstrzygnięte (D8):** hwdata nie ma tablicy nazw JEDEC; pokazujemy surowy identyfikator (na tej maszynie „Bank 8, Hex 0x5D”) obok numeru części; od D37 tylko w tekście do schowka. | – | M1 |
| O15 | Tempo dochodzenia wyjścia do celu w automacie na pozostałych kanałach (CPU, pompa, SYS 2–4) i przy innych czasach Step Up/Down; czas dojścia do 100 % po przejściu na automat w zdarzeniu awaryjnym. Dziś tylko pomiar na SYS Fan 1 (ok. 5 jednostek/s przy 0,1 s). | odczyty `pwmN` co 0,5 s po przełączeniu, bez dodatkowych zapisów | M2b (D14) |
| O16 | Ostateczny kształt ABI zapisu krzywych po recenzji v1 i łatki 5 | lista linux-hwmon | M2c, M2d |
| O17 | Czy krzywa zapisana przez v1 przetrwa hibernację (S4) i ciepły restart przez kexec | test za zgodą | M2c |
| O18 | RGB z Flatpaka: hidraw jest dostępny tylko przez `--device=all`, które odsłania też `/dev/port` i inne urządzenia. Czy RGB przenieść do pomocnika (§3.4), czy wymagać instalacji natywnej, czy przyjąć `--device=all` | decyzja przed M3 | M3 |
| O19 | Czy polkit z `auth_admin_keep` (D3) działa dla wywołań z Flatpaka przez pośrednika D-Bus tak jak dla aplikacji natywnej (jedno hasło na kilka minut) | test w M2b | M2b |
