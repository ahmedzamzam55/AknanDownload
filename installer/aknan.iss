; Aknan Download — Windows installer (Inno Setup 6)
; Built automatically by BUILD.bat -> dist\AknanDownload-Setup.exe
;
; Installs for the current user (no admin needed) into
;   %LOCALAPPDATA%\Programs\Aknan Download
; so yt-dlp can update itself. Adds a Start Menu folder, an optional desktop
; shortcut, "start with Windows", and a normal uninstaller.

#define MyAppName "Aknan Download"
#define MyAppExe "AknanDownload.exe"
#define MyAppURL "https://github.com/ahmedzamzam55/AknanDownload"
; BUILD.bat passes the version of Cargo.toml: /DMyAppVersion=1.0.1
; the files that go into the setup (gathered by BUILD.bat / the CI)
#ifndef Stage
  #define Stage "..\build-cache\stage"
#endif
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

[Setup]
AppId={{8B4E9A12-5C3D-4E71-9A2B-6F1D0E3C8B54}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher=Aknan
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\Aknan Download
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=AknanDownload-Setup
#ifndef NoIcon
SetupIconFile=..\crates\app\icon.ico
#endif
UninstallDisplayIcon={app}\aknan.ico
VersionInfoVersion={#MyAppVersion}
VersionInfoCompany=Aknan
VersionInfoDescription=Aknan Download Setup
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCopyright=Copyright (C) 2026 Aknan
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no

; the language of the setup is also the language of the program: asked
; once, at the first install (Windows' own language is chosen at first);
; an update keeps it and asks nothing
ShowLanguageDialog=yes
UsePreviousLanguage=yes
LanguageDetectionMethod=uilanguage

[Languages]
; the names are the program's own language codes (see [Registry])
Name: "en"; MessagesFile: "compiler:Default.isl"
#if FileExists(AddBackslash(CompilerPath) + "Languages\Arabic.isl")
Name: "ar"; MessagesFile: "compiler:Languages\Arabic.isl"
#else
Name: "ar"; MessagesFile: "lang\Arabic.isl"
#endif
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"

#include "messages.iss"

[Tasks]
Name: "desktopicon"; Description: "{cm:TaskDesktop}"; GroupDescription: "{cm:GroupShortcuts}"
Name: "autostart"; Description: "{cm:TaskAutostart}"; GroupDescription: "{cm:GroupStartup}"
Name: "torrents"; Description: "{cm:TaskTorrents}"; GroupDescription: "{cm:GroupTorrents}"
Name: "firewall"; Description: "{cm:TaskFirewall}"; GroupDescription: "{cm:GroupTorrents}"

[Files]
Source: "{#Stage}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\aknan.ico"; DestDir: "{app}"; Flags: ignoreversion
; the helper the browser starts (the program copies it into its data folder)
Source: "{#Stage}\aknan-host.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; the helpers go into {app}\tools; the app updates them there by itself
Source: "{#Stage}\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Stage}\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\extension-firefox\*"; DestDir: "{app}\extension-firefox"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\aknan-download-firefox.xpi"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; system runtime files, installed only when they are missing (see [Code])
#ifexist "..\build-cache\redist\vc_redist.x64.exe"
  #define HaveRedist
Source: "..\build-cache\redist\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: dontcopy
#endif

[InstallDelete]
; older versions kept the helpers next to the program
Type: files; Name: "{app}\yt-dlp.exe"
Type: files; Name: "{app}\ffmpeg.exe"
; torrents are shared by the program itself now: the old helper goes
Type: files; Name: "{app}\tools\aria2c.exe"
Type: files; Name: "{app}\tools\aria2-COPYING.txt"
; the browser add-in is put in again whole: nothing of an older version
; stays behind to clash with the new one (the folder is the same, so the
; browser keeps the add-in; the program writes its key in again at start)
Type: filesandordirs; Name: "{app}\extension"
Type: filesandordirs; Name: "{app}\extension-firefox"
; the Firefox add-on of an older version never stays: only this setup's own
; one (or none) is there to be added to Firefox
Type: files; Name: "{app}\aknan-download-firefox.xpi"

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\aknan.ico"
Name: "{group}\Browser extension folder"; Filename: "{app}\extension"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\aknan.ico"; Tasks: desktopicon

[Registry]
; tells the app the installer already decided about "start with Windows"
Root: HKCU; Subkey: "Software\Aknan\AknanDownload"; ValueType: string; ValueName: "Installed"; ValueData: "1"; Flags: uninsdeletekey
; the first start of the program takes these (its own settings win later)
Root: HKCU; Subkey: "Software\Aknan\AknanDownload"; ValueType: string; ValueName: "Language"; ValueData: "{language}"
Root: HKCU; Subkey: "Software\Aknan\AknanDownload"; ValueType: string; ValueName: "Torrents"; ValueData: "1"; Tasks: torrents
Root: HKCU; Subkey: "Software\Aknan\AknanDownload"; ValueType: string; ValueName: "Torrents"; ValueData: "0"; Tasks: not torrents
; start in the background (next to the clock) when the user signs in
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AknanDownload"; ValueData: """{app}\{#MyAppExe}"" --background"; Flags: uninsdeletevalue; Tasks: autostart
; the browsers' entry for the program's helper (the program fills it in at
; start): gone again when Aknan Download is removed
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\com.aknan.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Chromium\NativeMessagingHosts\com.aknan.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Edge\NativeMessagingHosts\com.aknan.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.aknan.download"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Mozilla\NativeMessagingHosts\com.aknan.download"; ValueType: none; Flags: uninsdeletekey
; "Open Aknan Download" in the browser add-in (aknan-download://open). The
; address given by the browser is never passed to the program.
Root: HKCU; Subkey: "Software\Classes\aknan-download"; ValueType: string; ValueName: ""; ValueData: "URL:Aknan Download"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\aknan-download"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\aknan-download\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"
Root: HKCU; Subkey: "Software\Classes\aknan-download\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" --from-browser"

; .torrent files and magnet: links (only when chosen while installing).
; Windows may still ask once which program to use.
Root: HKCU; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: string; ValueName: "AknanDownload.torrent"; ValueData: ""; Flags: uninsdeletevalue; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueName: ""; ValueData: "AknanDownload.torrent"; Flags: uninsdeletevalue; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\AknanDownload.torrent"; ValueType: string; ValueName: ""; ValueData: "Torrent file"; Flags: uninsdeletekey; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\AknanDownload.torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\AknanDownload.torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet link"; Flags: uninsdeletekey; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExe},0"; Tasks: torrents
Root: HKCU; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: torrents

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "{cm:RunNow}"; Flags: nowait postinstall skipifsilent
; after an automatic update: start the new version again
Filename: "{app}\{#MyAppExe}"; Flags: nowait; Check: IsAutoUpdate


[UninstallRun]
Filename: "{cmd}"; Parameters: "/C taskkill /IM {#MyAppExe} /F"; Flags: runhidden; RunOnceId: "StopAknan"
Filename: "{cmd}"; Parameters: "/C taskkill /IM aknan-host-* /F"; Flags: runhidden; RunOnceId: "StopAknanHost"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\extension"
Type: filesandordirs; Name: "{app}\extension-firefox"
Type: filesandordirs; Name: "{app}\tools"
Type: filesandordirs; Name: "{userappdata}\Aknan\AknanDownload\data\host"

#include "code.iss"
