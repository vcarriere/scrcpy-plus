# Building

## Portable Windows test build

From the repository root, run `./buildtools/Build-TestPreview.ps1`. It loads the
development environment, builds an embedded-frontend preview, and places a
portable copy in `artifacts/adb-controls-preview/`. By default it copies the
runtime from the current user's installed `scrcpy-plus` folder; use
`-ScrcpyDirectory` to select another existing runtime directory.

Double-click **Start Test Build.cmd** in that folder. This build has a separate
app identifier/settings profile and a **SCRCPY+ Test Build** window title. It does
not install over the normal app or import its custom password wrapper/settings.
ADB is still shared: close the original app before testing Stop, since its device
polling can restart ADB. See the generated `TESTING.txt` for the manual checklist.

## Intro
This is the guide to building the latest version of SCRCPY+ for all operating systems. Note: ***PREREQUISITES FOR RUNNING SCRCPY+ ARE DIFFERENT THAN BUILDING***

## Prerequisites
- Windows, Linux & MacOS
    - [Node 26 LTS](https://nodejs.org/en/) (verified with 26.13.1)
    - [Rust](https://rustup.rs/)
      (`rust-toolchain.toml` pins stable 1.100.0)
- Windows only: Visual Studio C++ Build Tools and a Windows SDK. From PowerShell,
  `. .\buildtools\Use-DevelopmentEnvironment.ps1` loads the available tools for
  the current shell. It can also use the documented optional local SDK fallback.
- Linux only
    - **Debian/Ubuntu-based:**
      ```bash
      sudo apt install libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libwebkit2gtk-4.1-dev
      ```
    - **Fedora/RHEL-based:**
      ```bash
      sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel
      sudo dnf group install "c-development"
      ```
    - **Arch-based:**
      ```bash
      sudo pacman -S gtk3 libsoup webkit2gtk
      ```
  \* More information available at https://v2.tauri.app/start/prerequisites/

## Build
***ALL COMMANDS ARE RUN IN THE 'application' FOLDER OF THE PROJECT***

- Linux & MacOS
    - run `npm install`
    - run `NO_STRIP=true npm run tauri build`
    - 🎉 The compiled project should be in the `application/src-tauri/target/release` folder

- Windows
    - run `npm install`
    - run `npm run tauri build`
    - 🎉 The compiled project should be in the `application/src-tauri/target/release` folder

