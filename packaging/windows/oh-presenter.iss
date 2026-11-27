; Windows installer (Inno Setup 6, https://jrsoftware.org/isinfo.php).
; Build the program first:  cargo build --release
;   (or cross-compiled: cargo build --release --target x86_64-pc-windows-gnullvm)
; then:                      iscc packaging\windows\oh-presenter.iss
; The installer is written to target\installer\.
; Video support: put ffmpeg.exe and ffplay.exe into packaging\windows\ffmpeg\ first
; (get-ffmpeg.ps1 downloads them); without them the installer is built without video.

#define AppName "oh! Presenter"
#define AppVersion "0.3.0"
#define AppExe "oh-presenter-rs.exe"

; where cargo put the exe: native Windows build, or cross build with --target
#if FileExists(AddBackslash(SourcePath) + "..\..\target\release\" + AppExe)
  #define ExeDir "..\..\target\release"
#elif FileExists(AddBackslash(SourcePath) + "..\..\target\x86_64-pc-windows-gnullvm\release\" + AppExe)
  #define ExeDir "..\..\target\x86_64-pc-windows-gnullvm\release"
#elif FileExists(AddBackslash(SourcePath) + "..\..\target\x86_64-pc-windows-msvc\release\" + AppExe)
  #define ExeDir "..\..\target\x86_64-pc-windows-msvc\release"
#elif FileExists(AddBackslash(SourcePath) + "..\..\target\x86_64-pc-windows-gnu\release\" + AppExe)
  #define ExeDir "..\..\target\x86_64-pc-windows-gnu\release"
#else
  #error oh-presenter-rs.exe not found - run "cargo build --release" first
#endif

[Setup]
AppId={{6F1B6C2E-4E0B-4C8E-9B0A-0E5D3B7A9C41}
AppName={#AppName}
AppVersion={#AppVersion}
DefaultDirName={autopf}\oh! Presenter
DefaultGroupName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
SetupIconFile=..\..\assets\icon.ico
LicenseFile=..\..\LICENSE
OutputDir=..\..\target\installer
OutputBaseFilename=oh-presenter-rs-setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Windows 10 and 11, 64-bit
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; installing only for the current user needs no administrator rights
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; a new version installs over the old one (same AppId, same folder, same install mode);
; a running oh! Presenter is closed first (its files could not be replaced otherwise)
; and not started again. Songs, settings etc. are in %APPDATA% / %LOCALAPPDATA%:
; neither installing nor uninstalling touches them.
UsePreviousAppDir=yes
UsePreviousPrivileges=yes
CloseApplications=force
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "cs"; MessagesFile: "compiler:Languages\Czech.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[InstallDelete]
; the program's files of the old version go first, so nothing stale stays behind
; (e.g. libunwind.dll of a cross build, or ffmpeg when the new installer has no video)
Type: files; Name: "{app}\oh-presenter-rs.exe"
Type: files; Name: "{app}\libunwind.dll"
Type: files; Name: "{app}\LICENSE.txt"
Type: files; Name: "{app}\ffmpeg.exe"
Type: files; Name: "{app}\ffplay.exe"
Type: files; Name: "{app}\ffmpeg-LICENSE.txt"

[Files]
Source: "{#ExeDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
; runtime of the gnullvm cross build (not needed by MSVC builds)
Source: "{#ExeDir}\libunwind.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; videos (optional, see the top of this file)
Source: "ffmpeg\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "ffmpeg\ffplay.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "ffmpeg\LICENSE.txt"; DestDir: "{app}"; DestName: "ffmpeg-LICENSE.txt"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{#AppName} - output window (network)"; Filename: "{app}\{#AppExe}"; Parameters: "canvas"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; network outputs (off by default, Settings -> Network outputs): let the Windows firewall
; accept them. Only possible when installed for all users (with administrator rights);
; otherwise Windows asks the first time the network is switched on.
; (the old version's rule is deleted first, so upgrades do not add copies of it)
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#AppName}"" program=""{app}\{#AppExe}"""; Flags: runhidden; Check: IsAdminInstallMode
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""{#AppName}"" dir=in action=allow program=""{app}\{#AppExe}"" enable=yes profile=private,domain"; Flags: runhidden; Check: IsAdminInstallMode
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#AppName}"" program=""{app}\{#AppExe}"""; Flags: runhidden; Check: IsAdminInstallMode; RunOnceId: "DelFirewallRule"
