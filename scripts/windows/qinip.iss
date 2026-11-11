; Inno Setup script: builds qinip-setup-x64.exe from the release build.
; Compiled by CI on the Windows runner (and by scripts/package-windows.sh when
; Inno Setup is installed): ISCC /DAppVersion=1.2.0 scripts\windows\qinip.iss
;
; Installs per user (no admin prompt) unless the user chooses "all users";
; adds a Start menu entry and an uninstaller in Settings > Apps.

#define MyAppName "Qinip"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define MyAppExeName "Qinip.exe"

[Setup]
AppId={{35439FF1-92C9-4D57-956B-A6D0C40A604B}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppVerName={#MyAppName} {#AppVersion}
AppPublisher=Hiren Budhrani
AppPublisherURL=https://github.com/Hersh3yy/qinip
AppSupportURL=https://github.com/Hersh3yy/qinip/issues
AppUpdatesURL=https://github.com/Hersh3yy/qinip/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\target\release
OutputBaseFilename=qinip-setup-x64
SetupIconFile=..\..\assets\qinip.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\qinip.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\assets\THIRD_PARTY_LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
