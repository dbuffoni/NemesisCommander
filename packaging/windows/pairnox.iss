; Inno Setup script of the Windows installer. Built by build-setup.ps1, which
; passes the version (/DAppVersion=...), the folder holding the program files
; (/DSourceDir=...) and the output folder and name (/DOutDir, /DOutName).
;
; It installs for the current user: no administrator needed, a folder under
; %LOCALAPPDATA%\Programs, a Start menu entry, an entry in "Apps" to remove
; it, and (when ticked) the folder on the user's PATH so `pnxr` runs from any
; terminal.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{B6F1C0A4-5C1E-4A3B-9B52-7E0D6A5E1F27}
AppName=Pairnox
AppVersion={#AppVersion}
AppPublisher=Pairnox
AppPublisherURL=https://github.com/dbuffoni/Pairnox
DefaultDirName={autopf}\Pairnox
DefaultGroupName=Pairnox
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
OutputDir={#OutDir}
OutputBaseFilename={#OutName}
SetupIconFile={#SourceDir}\pairnox.ico
UninstallDisplayIcon={app}\pnxr-gui.exe
LicenseFile={#SourceDir}\LICENSE.txt
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked
Name: "addtopath"; Description: "Add Pairnox to the &PATH (to run `pnxr` in a terminal)"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\Pairnox"; Filename: "{app}\pnxr-gui.exe"; WorkingDir: "{%USERPROFILE}"
Name: "{group}\Uninstall Pairnox"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Pairnox"; Filename: "{app}\pnxr-gui.exe"; WorkingDir: "{%USERPROFILE}"; Tasks: desktopicon

[Registry]
; The user's PATH: the folder is added once and taken out when uninstalling.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Tasks: addtopath; Check: NeedsPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\pnxr-gui.exe"; Description: "Start Pairnox"; Flags: nowait postinstall skipifsilent

[Code]
function NeedsPath(Folder: string): Boolean;
var
  Existing: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Existing) then
    Existing := '';
  Result := Pos(';' + Lowercase(Folder) + ';', ';' + Lowercase(Existing) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Existing, Folder: string;
  P: Integer;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Existing) then
    Exit;
  Folder := ExpandConstant('{app}');
  P := Pos(';' + Lowercase(Folder) + ';', ';' + Lowercase(Existing) + ';');
  if P > 0 then
  begin
    Delete(Existing, P, Length(Folder) + 1);
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Existing);
  end;
end;
