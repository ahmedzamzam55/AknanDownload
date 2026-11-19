[Code]
const
  PROCESS_TERMINATE = $0001;
  TH32CS_SNAPPROCESS = $00000002;
  INVALID_HANDLE_VALUE = $FFFFFFFF;

type
  TProcessEntry32W = record
    dwSize: Cardinal;
    cntUsage: Cardinal;
    th32ProcessID: Cardinal;
    th32DefaultHeapID: Cardinal;
    th32ModuleID: Cardinal;
    cntThreads: Cardinal;
    th32ParentProcessID: Cardinal;
    pcPriClassBase: Longint;
    dwFlags: Cardinal;
    szExeFile: array[0..259] of Char;
  end;

function CreateToolhelp32Snapshot(dwFlags, th32ProcessID: Cardinal): THandle;
  external 'CreateToolhelp32Snapshot@kernel32.dll stdcall';
function Process32FirstW(hSnapshot: THandle; var lppe: TProcessEntry32W): LongBool;
  external 'Process32FirstW@kernel32.dll stdcall';
function Process32NextW(hSnapshot: THandle; var lppe: TProcessEntry32W): LongBool;
  external 'Process32NextW@kernel32.dll stdcall';
function OpenProcess(dwDesiredAccess: Cardinal; bInheritHandle: LongBool; dwProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function TerminateProcess(hProcess: THandle; uExitCode: Cardinal): LongBool;
  external 'TerminateProcess@kernel32.dll stdcall';
function CloseHandle(hObject: THandle): LongBool;
  external 'CloseHandle@kernel32.dll stdcall';

function IsAutoUpdate: Boolean;
begin
  Result := ExpandConstant('{param:AUTOUPDATE|0}') = '1';
end;

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
  if WizardSilent then exit;
  try
    ExtractTemporaryFile('vc_redist.x64.exe');
  except
    exit;
  end;
  WizardForm.StatusLabel.Caption := CustomMessage('SystemFiles');
  ShellExec('runas', ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /quiet /norestart', '', SW_HIDE, ewWaitUntilTerminated, Code);
#endif
end;

procedure AllowTorrents;
var
  Policy, Rule: Variant;
  Tool, RuleName: String;
begin
  if WizardSilent then exit;
  Tool := ExpandConstant('{app}\{#MyAppExe}');
  RuleName := 'Aknan Download torrents';
  try
    Policy := CreateOleObject('HNetCfg.FwPolicy2');
    try
      Policy.Rules.Remove(RuleName);
    except
    end;
    Rule := CreateOleObject('HNetCfg.FWRule');
    Rule.Name := RuleName;
    Rule.ApplicationName := Tool;
    Rule.Protocol := 256;
    Rule.Direction := 1;
    Rule.Action := 1;
    Rule.Profiles := $7FFFFFFF;
    Rule.Enabled := True;
    Policy.Rules.Add(Rule);
  except
  end;
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

function ExeNameOf(const Entry: TProcessEntry32W): String;
var
  I: Integer;
begin
  Result := '';
  for I := 0 to 259 do
  begin
    if Entry.szExeFile[I] = #0 then break;
    Result := Result + Entry.szExeFile[I];
  end;
  Result := Lowercase(Result);
end;

function ScanProcesses(const Pattern: String; PrefixMatch, TerminateMatch: Boolean): Boolean;
var
  Snap, Proc: THandle;
  Entry: TProcessEntry32W;
  Exe, Want: String;
  Matched: Boolean;
begin
  Result := False;
  Want := Lowercase(Pattern);
  Snap := CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if Snap = INVALID_HANDLE_VALUE then exit;
  try
    Entry.dwSize := SizeOf(Entry);
    if Process32FirstW(Snap, Entry) then
    begin
      repeat
        Exe := ExeNameOf(Entry);
        if PrefixMatch then
          Matched := Pos(Want, Exe) = 1
        else
          Matched := Exe = Want;
        if Matched then
        begin
          Result := True;
          if TerminateMatch and (Entry.th32ProcessID <> 0) then
          begin
            Proc := OpenProcess(PROCESS_TERMINATE, False, Entry.th32ProcessID);
            if Proc <> 0 then
            begin
              TerminateProcess(Proc, 1);
              CloseHandle(Proc);
            end;
          end;
        end;
      until not Process32NextW(Snap, Entry);
    end;
  finally
    CloseHandle(Snap);
  end;
end;

function AppRunning: Boolean;
begin
  Result := CheckForMutexes('AknanDownloadRunning') or ScanProcesses('{#MyAppExe}', False, False);
end;

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

procedure StopRunningInstances;
var
  I: Integer;
begin
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
    ScanProcesses('{#MyAppExe}', False, True);
    Sleep(1000);
  end;
  ScanProcesses('aknan-host-', True, True);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningInstances;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopRunningInstances;
end;
