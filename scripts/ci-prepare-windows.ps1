$ErrorActionPreference = 'Stop'
$compilerDirectory = Join-Path $env:RUNNER_TEMP 'nsh-inno'
$installer = Join-Path $env:RUNNER_TEMP 'innosetup-6.7.4.exe'
$installerHash = 'a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c'
$translationHash = 'e0b0b350e2245f3c5e65586dfe43d574f6e7f06f2261149aba284954b3fc9a8d'
Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_4/innosetup-6.7.4.exe' -OutFile $installer
if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $installerHash) {
    throw 'Inno Setup installer checksum mismatch'
}
$process = Start-Process -FilePath $installer -Wait -PassThru -ArgumentList @(
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/DIR=`"$compilerDirectory`""
)
if ($process.ExitCode -ne 0) { throw "Inno Setup installation failed: $($process.ExitCode)" }
if (!(Test-Path (Join-Path $compilerDirectory 'ISCC.exe'))) { throw 'Missing ISCC.exe' }
$translation = Join-Path $compilerDirectory 'Languages/ChineseSimplified.isl'
Invoke-WebRequest 'https://raw.githubusercontent.com/jrsoftware/issrc/6ef32198ef1f7b7b375cd4b6b90896c2a58eb4c2/Files/Languages/ChineseSimplified.isl' -OutFile $translation
if ((Get-FileHash -LiteralPath $translation -Algorithm SHA256).Hash.ToLowerInvariant() -ne $translationHash) {
    throw 'Inno Setup Chinese translation checksum mismatch'
}
Add-Content -LiteralPath $env:GITHUB_PATH -Value $compilerDirectory
