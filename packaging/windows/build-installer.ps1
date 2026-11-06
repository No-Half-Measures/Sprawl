param(
    [string]$Version,
    [string]$BinDir = 'target\release',
    [string]$OutDir = 'target\installer',
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Defines = @()
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
Push-Location $root
try {
    if (!$Version) {
        $env:CARGO_NET_OFFLINE = 'true'
        $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
        if ($LASTEXITCODE) { throw 'cargo metadata failed' }
        $Version = ($metadata.packages | Where-Object name -eq 'sprawl').version
    }
    $command = Get-Command makensis.exe -ErrorAction SilentlyContinue
    $compiler = if ($command) { $command.Source } else {
        @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe") |
            Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
    if (!$compiler) { throw 'NSIS 3 makensis.exe was not found' }
    $binaryPath = (Resolve-Path -LiteralPath $BinDir).Path
    $outputPath = if ([IO.Path]::IsPathRooted($OutDir)) {
        [IO.Path]::GetFullPath($OutDir)
    } else {
        [IO.Path]::GetFullPath((Join-Path $root $OutDir))
    }
    New-Item -ItemType Directory -Force $outputPath | Out-Null
    $arguments = @("/DVERSION=$Version", "/DBINDIR=$binaryPath", "/DOUTDIR=$outputPath")
    $arguments += $Defines | ForEach-Object { if ($_ -match '^/D') { $_ } else { "/D$_" } }
    & $compiler @arguments "$PSScriptRoot\installer.nsi"
    if ($LASTEXITCODE) { throw "makensis failed with exit code $LASTEXITCODE" }
} catch {
    Write-Error $_ -ErrorAction Continue
    exit 1
} finally {
    Pop-Location
}
