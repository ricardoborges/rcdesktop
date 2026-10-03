; RC Desktop installer (Inno Setup 6.3+).
;
; One setup for x64 and ARM64: it installs the binaries matching the machine.
; rcompose.exe goes next to rcdesktop.exe, which is where the app looks first.
; Per-user install, no administrator rights needed.
;
; Build:
;   iscc /DAppVersion=0.1.0 /DSrcX64=<dir> /DSrcArm64=<dir> installer\rcdesktop.iss
; Each Src dir holds rcdesktop.exe and rcompose.exe for that architecture.
; Either one can be left out to build a single-architecture setup.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#if !Defined(SrcX64) && !Defined(SrcArm64)
  #error Define SrcX64 and/or SrcArm64
#endif

#define AppName "RC Desktop"
#define AppExe "rcdesktop.exe"

[Setup]
AppId={{6F1D2C3B-8A47-4E59-9B0E-2D7C5A1F4E83}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Ricardo Borges
AppPublisherURL=https://github.com/ricardoborges/rcdesktop
AppSupportURL=https://github.com/ricardoborges/rcdesktop/issues
AppUpdatesURL=https://github.com/ricardoborges/rcdesktop/releases
DefaultDirName={autopf}\RC Desktop
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
#if Defined(SrcX64) && Defined(SrcArm64)
ArchitecturesAllowed=x64compatible or arm64
ArchitecturesInstallIn64BitMode=x64compatible or arm64
#elif Defined(SrcArm64)
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0.22000
LicenseFile=..\LICENSE
SetupIconFile=..\assets\rcdesktop.ico
OutputDir=..\dist
OutputBaseFilename=rcdesktop-setup
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
; A running RC Desktop hides to the tray instead of closing, so the Restart
; Manager can't shut it down; it's ended in PrepareToInstall instead
CloseApplications=no
ChangesEnvironment=yes
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[CustomMessages]
english.AddToPath=Add rcompose to PATH (use it from the terminal)
brazilianportuguese.AddToPath=Adicionar o rcompose ao PATH (usar pelo terminal)
english.Autostart=Start RC Desktop with Windows (in the notification area)
brazilianportuguese.Autostart=Iniciar o RC Desktop com o Windows (na área de notificação)

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "{cm:Autostart}"; Flags: unchecked
Name: "addtopath"; Description: "{cm:AddToPath}"

[Files]
#ifdef SrcX64
Source: "{#SrcX64}\rcdesktop.exe"; DestDir: "{app}"; Check: not IsArm64; Flags: ignoreversion
Source: "{#SrcX64}\rcompose.exe"; DestDir: "{app}"; Check: not IsArm64; Flags: ignoreversion
#endif
#ifdef SrcArm64
Source: "{#SrcArm64}\rcdesktop.exe"; DestDir: "{app}"; Check: IsArm64; Flags: ignoreversion
Source: "{#SrcArm64}\rcompose.exe"; DestDir: "{app}"; Check: IsArm64; Flags: ignoreversion
#endif
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
    ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))
; Same entry the app's Settings page writes (src/settings.rs), so both stay in sync
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
    ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"" --autostart"; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
function PathContains(Paths, Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

function NeedsAddPath(Dir: string): Boolean;
var
  Paths: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Paths) then
    Paths := '';
  Result := not PathContains(Paths, Dir);
end;

// Ends a running RC Desktop (it may be hidden in the tray) so its files can
// be replaced or removed
procedure StopRunningApp();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  // Give Windows a moment to release the executable
  Sleep(500);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningApp();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopRunningApp();
  Result := True;
end;

// On uninstall: removes the start-with-Windows entry (whether the installer
// or the app's Settings page created it) and takes the install folder back
// out of the user PATH
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Paths, Dir: string;
  P: Integer;
begin
  if CurUninstallStep = usUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#AppName}');
  if CurUninstallStep <> usPostUninstall then
    exit;
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Paths) then
    exit;
  Dir := ExpandConstant('{app}');
  P := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then
    exit;
  // P is 1-based in ';' + Paths + ';', so it lines up with the entry in Paths
  Delete(Paths, P, Length(Dir) + 1);
  if (Length(Paths) > 0) and (Paths[Length(Paths)] = ';') then
    Delete(Paths, Length(Paths), 1);
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Paths);
end;
