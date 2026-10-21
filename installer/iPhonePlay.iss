#ifndef PayloadDir
  #error PayloadDir must be supplied by scripts/package.ps1
#endif
#ifndef OutputDir
  #error OutputDir must be supplied by scripts/package.ps1
#endif
#ifndef AppVersion
  #define AppVersion "0.1.1"
#endif
#ifndef ChineseLanguageFile
  #define ChineseLanguageFile "compiler:ChineseSimplified.isl"
#endif

[Setup]
AppId={{DCC85D6F-BF84-4DB0-9AC7-7706BDAD81C6}
AppName=iPhonePlay
AppVersion={#AppVersion}
AppVerName=iPhonePlay {#AppVersion}
AppPublisher=iPhonePlay contributors
VersionInfoVersion={#AppVersion}.0
VersionInfoProductVersion={#AppVersion}.0
VersionInfoDescription=iPhonePlay Windows x64 Setup
VersionInfoProductName=iPhonePlay
DefaultDirName={autopf}\iPhonePlay
DefaultGroupName=iPhonePlay
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=iPhonePlay-{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\iPhonePlay.exe
UninstallDisplayName=iPhonePlay
LicenseFile=..\LICENSE
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19045
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
LZMAUseSeparateProcess=yes
CloseApplications=yes
CloseApplicationsFilter=iPhonePlay.exe,iPhoneMirror.WirelessHost.exe,iPhoneMirror.Driver.exe
RestartApplications=no
SetupLogging=yes
ChangesEnvironment=no

[Languages]
Name: "chinesesimp"; MessagesFile: "{#ChineseLanguageFile}"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; Flags: unchecked
Name: "airplayfirewall"; Description: "允许 AirPlay 接收服务通过 Windows 防火墙（仅专用网络）"

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\iPhonePlay"; Filename: "{app}\iPhonePlay.exe"; WorkingDir: "{app}"; AppUserModelID: "iPhonePlay.Desktop"
Name: "{autodesktop}\iPhonePlay"; Filename: "{app}\iPhonePlay.exe"; WorkingDir: "{app}"; Tasks: desktopicon; AppUserModelID: "iPhonePlay.Desktop"
Name: "{group}\卸载 iPhonePlay"; Filename: "{uninstallexe}"

[Registry]
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\iPhonePlay.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\iPhonePlay.exe"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\iPhonePlay.exe"; ValueType: string; ValueName: "Path"; ValueData: "{app}"; Flags: uninsdeletekey
Root: HKLM; Subkey: "Software\Classes\AppUserModelId\iPhonePlay.Desktop"; ValueType: string; ValueName: "DisplayName"; ValueData: "iPhonePlay"; Flags: uninsdeletekey

[Run]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""iPhonePlay AirPlay (Private)"" program=""{app}\Wireless\iPhoneMirror.WirelessHost.exe"""; Flags: runhidden waituntilterminated; Tasks: airplayfirewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""iPhonePlay AirPlay (Private)"" dir=in action=allow program=""{app}\Wireless\iPhoneMirror.WirelessHost.exe"" enable=yes profile=private"; Flags: runhidden waituntilterminated; Tasks: airplayfirewall
Filename: "{app}\iPhonePlay.exe"; Description: "启动 iPhonePlay"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""iPhonePlay AirPlay (Private)"" program=""{app}\Wireless\iPhoneMirror.WirelessHost.exe"""; Flags: runhidden waituntilterminated; RunOnceId: "Remove-iPhonePlay-AirPlay-Private-Rule"

; Intentionally no [UninstallDelete] entries. Pictures, Videos, logs and
; %LOCALAPPDATA%\iPhonePlay are user data and survive uninstall. Shared Apple
; drivers and the per-device USB filter are only managed by the driver tool.
