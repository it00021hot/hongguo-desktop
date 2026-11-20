function NshShortcutPath(const Directory, Name: String): String;
begin
  Result := ExpandConstant('{' + Directory + '}') + '\' + Name + '.lnk';
end;

function NshOwnsShortcut(const Path: String): Boolean;
var
  Shell, Link: Variant;
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
begin
  Path := NshShortcutPath(Directory, NshShortcutName);
  Result := not FileExists(Path) or NshOwnsShortcut(Path);
end;

procedure NshRemoveLegacyShortcut(const Directory: String);
var
  Legacy: String;
begin
  if CompareText(NshShortcutName, NshLegacyShortcutName) = 0 then
    Exit;
  Legacy := NshShortcutPath(Directory, NshLegacyShortcutName);
  if not NshOwnsShortcut(Legacy) then
    Exit;
  if not DeleteFile(Legacy) then
    Log('Cannot remove legacy shortcut ' + Legacy);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then
    Exit;
  NshRemoveLegacyShortcut('autoprograms');
  NshRemoveLegacyShortcut('autodesktop');
end;

