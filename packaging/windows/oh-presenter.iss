; Windows installer (Inno Setup 6, https://jrsoftware.org/isinfo.php).
; Build the program first:  cargo build --release
; then:                      iscc packaging\windows\oh-presenter.iss
; The installer is written to target\installer\.
; Video support: put ffmpeg.exe and ffplay.exe into packaging\windows\ffmpeg\ first
; (get-ffmpeg.ps1 downloads them); without them the installer is built without video.

#define AppName "oh! Presenter"
#define AppVersion "0.1.0"
#define AppExe "oh-presenter-rs.exe"

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

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "cs"; MessagesFile: "compiler:Languages\Czech.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; videos (optional, see the top of this file)
Source: "ffmpeg\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "ffmpeg\ffplay.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "ffmpeg\LICENSE.txt"; DestDir: "{app}"; DestName: "ffmpeg-LICENSE.txt"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{#AppName} – output window (network)"; Filename: "{app}\{#AppExe}"; Parameters: "canvas"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
