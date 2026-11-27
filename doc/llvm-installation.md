# LLVM und Build-Umgebung

Stand: 2026-11-27. Der Compiler verwendet `inkwell` 0.9 mit dem Feature `llvm21-1` und dem Target
`x86`. Dafür braucht der Build LLVM 21 mit den Entwicklungsdateien (Header, Bibliotheken und
`llvm-config`). Die Laufzeitbibliothek und der Compiler selbst sind Rust-Code.

## Gemeinsame Voraussetzungen

1. **Rust** mit `cargo`, aktuelle stable-Version. Unter Windows die MSVC-Variante (`x86_64-pc-windows-msvc`).
2. **LLVM 21.1.x** mit Entwicklungsdateien. Das Repo wurde mit 21.1.8 getestet.
3. **Ein C-Linker** für die Ausgabe-EXE (siehe jeweiliges Betriebssystem).
4. **Umgebungsvariable `LLVM_SYS_211_PREFIX`**: zeigt auf das Installationsverzeichnis von LLVM.
   Der Build von `llvm-sys` liest sie. Der Wert ist das Verzeichnis, in dem `bin/llvm-config`
   (bzw. `llvm-config.exe`) liegt.

Die Prüfung, ob LLVM richtig gefunden wird:

```
llvm-config --version        # Linux/macOS: erwartet 21.1.x
```

Unter Windows steht `llvm-config.exe` im Ordner `bin` der LLVM-Installation.

## Windows (MSVC)

**Voraussetzungen**

- Visual Studio Build Tools mit der Workload „Desktop-Entwicklung mit C++“ (liefert `link.exe`
  und das Windows SDK mit `kernel32.lib` usw.). `abapc` findet `link.exe` über die Registrierung,
  eine Developer-Shell ist nicht nötig.
- LLVM 21.1.x als Windows-Archiv (im Projekt liegt `dl/LLVM-21.1.8-win64.7z`). Entpacken in ein
  Verzeichnis ohne Leerzeichen im Pfad, z. B. `C:\LLVM-21.1.8-win64`. Vorher prüfen, dass dort
  `bin\llvm-config.exe`, `include\llvm` und `lib\` vorhanden sind.

**Konfiguration**

Das Repo setzt den Pfad in `.cargo/config.toml`:

```toml
[env]
LLVM_SYS_211_PREFIX = "C:\\Users\\<name>\\w\\LLVM-21.1.8-win64"
```

Der Pfad ist benutzerspezifisch. `.cargo/` steht in `.gitignore`, die Datei ist also lokal und wird
nicht eingecheckt. Auf einem anderen Rechner den Pfad anpassen. Alternativ die Variable systemweit
setzen:

```
setx LLVM_SYS_211_PREFIX "C:\LLVM-21.1.8-win64"
```

Bei `setx` muss anschließend ein neues Terminal gestartet werden.

**Build**

```
cargo build --workspace
cargo test -p abap-compiler --test acceptance
```

## Linux (Debian/Ubuntu)

**Voraussetzungen**

- `build-essential` (liefert `cc` und `ld`).
- LLVM 21 mit Entwicklungspaketen. Unter Ubuntu/Debian über das LLVM-Repository
  (https://apt.llvm.org, Installationsskript `llvm.sh 21`) und dann `llvm-21-dev`.
  Auf anderen Distributionen den Paketnamen des LLVM-21-Devel-Pakets suchen.
- Für den Linker werden je nach Installation `zlib1g-dev` und `libzstd-dev` gebraucht, wenn LLVM
  statisch gegen diese Bibliotheken gelinkt wird. Bei Link-Fehlern zuerst nachinstallieren.

**Konfiguration**

```
export LLVM_SYS_211_PREFIX=/usr/lib/llvm-21
```

Der Pfad hängt von der Installation ab. Das Installationsverzeichnis mit `llvm-config --prefix`
prüfen.

**Build**

```
cargo build --workspace
cargo test -p abap-compiler --test acceptance
```

Der Linker-Aufruf in `abap-compiler/src/main.rs` verwendet `-lpthread -ldl -lm -lgcc_s`. Das passt
zu glibc und gcc auf Linux.

## macOS

**Voraussetzungen**

- Xcode Command Line Tools (`xcode-select --install`). Sie liefern `cc` und den Linker.
- LLVM 21 über Homebrew, z. B. `brew install llvm@21`. Den Formelnamen vorher mit `brew search llvm`
  prüfen, da er sich ändern kann.

**Konfiguration**

```
export LLVM_SYS_211_PREFIX="$(brew --prefix llvm@21)"
```

Auf Apple Silicon liegt das Präfix unter `/opt/homebrew/opt/llvm@21`, auf Intel unter
`/usr/local/opt/llvm@21`. `brew --prefix` liefert den richtigen Pfad.

**Build**

```
cargo build --workspace
```

**Bekannte Risiken (nicht getestet)**

- Der Linker-Aufruf übergibt `-lgcc_s`. Auf macOS gibt es diese Bibliothek in aktuellen Xcode-Versionen
  meist nicht. Das Linken wird dort vermutlich scheitern. Die Änderung ist eine Frage an den Code:
  Für macOS müsste der Aufruf plattformabhängig gemacht werden.
- Auf Apple Silicon ist der Host `aarch64`. Das Feature `target-x86` allein deckt diese Architektur
  nicht ab. Ob `Target::initialize_native` dort ohne weiteres Feature läuft, ist nicht geprüft.
  Das Feature `target-aarch64` gehört wahrscheinlich ergänzt.
- Die Laufzeit `libabap_rt.a` muss für dieselbe Architektur gebaut sein wie die Objektdatei.

Diese Punkte sind Vermutungen aus dem Code. Die Abschnitte müssen auf einem echten Mac geprüft werden,
bevor sie als belegt gelten.

## Fehlersuche

- **`Could not find LLVM` oder ähnlich bei `cargo build`**: `LLVM_SYS_211_PREFIX` prüfen. Der Pfad muss
  auf die Installation zeigen, nicht auf ein Unterverzeichnis.
- **`Linker 'link.exe' nicht startbar`**: Visual Studio Build Tools mit C++-Workload installieren.
- **`Laufzeitbibliothek nicht gefunden`**: `cargo build --workspace` ausführen, oder `--runtime` angeben.
- **`Linken fehlgeschlagen`**: Die Ausgabe enthält den Linker-Fehler wörtlich. Meistens fehlt eine
  Systembibliothek oder die Architektur passt nicht.
