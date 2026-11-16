[Code]
function IsAutoUpdate: Boolean;
begin
  Result := ExpandConstant('{param:AUTOUPDATE|0}') = '1';
end;

{ the Visual C++ 2015-2022 runtime (vcruntime140.dll) }
function RuntimeMissing: Boolean;
var
  Installed: Cardinal;
begin
  Result := True;
  if RegQueryDWordValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) and (Installed = 1)
     and FileExists(ExpandConstant('{sys}\vcruntime140.dll'))
     and FileExists(ExpandConstant('{sys}\vcruntime140_1.dll'))
     and FileExists(ExpandConstant('{sys}\msvcp140.dll')) then
    Result := False;
end;

procedure InstallRuntime;
var
  Code: Integer;
begin
#ifdef HaveRedist
  if not RuntimeMissing then exit;
  { an automatic update must not show any window }
  if WizardSilent then exit;
  try
    ExtractTemporaryFile('vc_redist.x64.exe');
  except
    exit;
  end;
  WizardForm.StatusLabel.Caption := CustomMessage('SystemFiles');
  { needs administrator rights: Windows asks once. If that is refused the
    program still works, it carries its own copy of these files. }
  ShellExec('runas', ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /quiet /norestart', '', SW_HIDE, ewWaitUntilTerminated, Code);
#endif
end;

{ the program takes people's connections for torrents: Windows' firewall
  lets it (needs administrator rights: Windows asks once; refused = torrents
  still work, a little slower) }
procedure AllowTorrents;
var
  Code: Integer;
  Tool: String;
begin
  if WizardSilent then exit;
  Tool := ExpandConstant('{app}\{#MyAppExe}');
  ShellExec('runas', ExpandConstant('{sys}\cmd.exe'),
    '/C netsh advfirewall firewall delete rule name="Aknan Download torrents" >nul 2>&1 & ' +
    'netsh advfirewall firewall add rule name="Aknan Download torrents" dir=in action=allow program="' + Tool + '" enable=yes profile=any >nul 2>&1',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    InstallRuntime;
    if WizardIsTaskSelected('firewall') then
      AllowTorrents;
  end;
end;

{ true while AknanDownload.exe still runs }
function AppRunning: Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C tasklist /NH /FI "IMAGENAME eq {#MyAppExe}" | find /I "{#MyAppExe}" >nul', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
end;

{ ask the open program to close by itself: it saves the list and the
  downloads (they go on after the update). Versions before 1.0.8 don't
  know this; they are closed below. }
procedure AskAppToQuit;
var
  Http: Variant;
begin
  try
    Http := CreateOleObject('WinHttp.WinHttpRequest.5.1');
    Http.SetTimeouts(1000, 1000, 2000, 2000);
    Http.Open('POST', 'http://127.0.0.1:9614/quit', False);
    Http.SetRequestHeader('X-SwiftGet', '1');
    Http.Send('');
  except
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code, I: Integer;
begin
  { the running program locks its files: close it before copying }
  if AppRunning then
  begin
    AskAppToQuit;
    for I := 1 to 20 do
    begin
      if not AppRunning then break;
      Sleep(500);
    end;
  end;
  if AppRunning then
  begin
    Exec(ExpandConstant('{cmd}'), '/C taskkill /IM {#MyAppExe} >nul 2>&1', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(2000);
  end;
  if AppRunning then
  begin
    Exec(ExpandConstant('{cmd}'), '/C taskkill /F /IM {#MyAppExe} >nul 2>&1', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(800);
  end;
  Result := '';
end;
