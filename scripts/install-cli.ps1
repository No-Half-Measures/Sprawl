# Installs sprawl-cli for the current user and puts it on PATH. No admin needed.
#
#   irm https://github.com/No-Half-Measures/Sprawl/releases/latest/download/sprawl-cli-install.ps1 | iex
#
# Run the file with -Uninstall to remove it again.
param([switch]$Uninstall)

$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\sprawl-cli'
$exe = Join-Path $dir 'sprawl-cli.exe'
$url = 'https://github.com/No-Half-Measures/Sprawl/releases/latest/download/sprawl-cli-windows-x64.exe'

if ($Uninstall) {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $dir
} else {
    New-Item -ItemType Directory -Force $dir | Out-Null
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 'Tls12'
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -UseBasicParsing $url -OutFile "$exe.download"
    Move-Item -Force "$exe.download" $exe
}

# Edit the raw registry value so entries such as %USERPROFILE%\bin stay unexpanded.
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$path = @([string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' |
    Where-Object { $_ -and $_.TrimEnd('\') -ne $dir })
if (-not $Uninstall) { $path += $dir }
if ($path) { $key.SetValue('Path', $path -join ';', 'ExpandString') } else { $key.DeleteValue('Path', $false) }
$key.Close()
# Setting a user variable through .NET broadcasts the change to open programs.
[Environment]::SetEnvironmentVariable('SPRAWL_CLI_INSTALL', $null, 'User')

$session = @($env:Path -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $dir })
if (-not $Uninstall) { $session += $dir }
$env:Path = $session -join ';'

if ($Uninstall) {
    Write-Host 'Removed sprawl-cli.'
} else {
    Write-Host "Installed $(& $exe --version) to $dir"
    Write-Host 'Open a new terminal to use sprawl-cli from anywhere.'
}
