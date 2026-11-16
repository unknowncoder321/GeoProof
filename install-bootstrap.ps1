# GeoProof — no-admin, no-git, no-compiler installer.
# Downloads everything it needs itself.
# Run this in PowerShell (any version, no admin):
#
#   powershell -ExecutionPolicy Bypass -Command "iwr https://raw.githubusercontent.com/unknowncoder321/GeoProof/main/install-bootstrap.ps1 | iex"
#
# Because the repo is private the script clones with the portable git and your stored GitHub login.
# If this is a new PC with no stored login, follow the prompt to sign in once.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Join-Path $env:LOCALAPPDATA 'GeoProof'
if (-not (Test-Path $root)) { New-Item -ItemType Directory -Path $root | Out-Null }

# Download install.ps1 from main branch
$h = @{ 'User-Agent' = 'geoproof-bootstrap' }
$raw = 'https://raw.githubusercontent.com/unknowncoder321/GeoProof/main/install.ps1'
$dst = Join-Path $root 'install.ps1'
Invoke-WebRequest -Headers $h -Uri $raw -OutFile $dst
& powershell -ExecutionPolicy Bypass -File $dst
