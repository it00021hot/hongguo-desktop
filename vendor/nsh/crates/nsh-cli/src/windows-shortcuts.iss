const
  NshShellDelete = $00000004;
  NshShellUpdateItem = $00002000;
  NshShellPathW = $0005;

procedure NshShellNotify(EventId: LongInt; Flags: DWORD; const Path: String; Unused: DWORD);
  external 'SHChangeNotify@shell32.dll stdcall';

function NshShortcutPath(const Directory, Name: String): String;
begin
  Result := ExpandConstant('{' + Directory + '}') + '\' + Name + '.lnk';
end;

function NshOwnsShortcut(const Path: String; var Link: Variant): Boolean;
var
  Shell: Variant;
  Target: String;
begin
  Result := False;
  if not FileExists(Path) then
    Exit;
  try
    Shell := CreateOleObject('WScript.Shell');
    Link := Shell.CreateShortcut(Path);
    Target := Link.TargetPath;
    Result := CompareText(NshExpand(Target), ExpandConstant('{app}') + NshExePath) = 0;
  except
    Log('Cannot read shortcut ' + Path + ': ' + GetExceptionMessage);
  end;
end;

function NshCanCreateShortcut(const Directory: String): Boolean;
var
  Path: String;
  Link: Variant;
begin
  Path := NshShortcutPath(Directory, NshShortcutName);
  Result := not FileExists(Path) or NshOwnsShortcut(Path, Link);
end;

procedure NshRefreshShortcut(const Path: String);
var
  Link: Variant;
  Icon, Previous: String;
begin
  if not NshOwnsShortcut(Path, Link) then
    Exit;
  try
    Icon := ExpandConstant('{app}') + NshIconPath + ',0';
    Previous := Link.IconLocation;
    if CompareText(Previous, Icon) <> 0 then begin
      Link.IconLocation := Icon;
      Link.Save;
    end;
    NshShellNotify(NshShellUpdateItem, NshShellPathW, Path, 0);
  except
    Log('Cannot refresh shortcut ' + Path + ': ' + GetExceptionMessage);
  end;
end;

procedure NshRefreshShellFolder(const Directory: String);
var
  Legacy: String;
  Link: Variant;
begin
  if CompareText(NshShortcutName, NshLegacyShortcutName) <> 0 then begin
    Legacy := NshShortcutPath(Directory, NshLegacyShortcutName);
    if NshOwnsShortcut(Legacy, Link) then begin
      if DeleteFile(Legacy) then
        NshShellNotify(NshShellDelete, NshShellPathW, Legacy, 0)
      else
        Log('Cannot remove legacy shortcut ' + Legacy);
    end;
  end;
  NshRefreshShortcut(NshShortcutPath(Directory, NshShortcutName));
end;

procedure NshRefreshPinnedShortcuts;
var
  Directory: String;
  Entry: TFindRec;
begin
  Directory := ExpandConstant('{userappdata}\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\');
  if not FindFirst(Directory + '*.lnk', Entry) then
    Exit;
  try
    repeat
      NshRefreshShortcut(Directory + Entry.Name);
    until not FindNext(Entry);
  finally
    FindClose(Entry);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  InstallDir: String;
begin
  if CurStep <> ssPostInstall then
    Exit;
  InstallDir := ExpandConstant('{app}');
  NshShellNotify(NshShellUpdateItem, NshShellPathW, InstallDir + NshExePath, 0);
  if NshIconPath <> NshExePath then
    NshShellNotify(NshShellUpdateItem, NshShellPathW, InstallDir + NshIconPath, 0);
  NshRefreshShellFolder('autoprograms');
  NshRefreshShellFolder('autodesktop');
  NshRefreshPinnedShortcuts;
end;

