# GeoProof installer: downloads the latest Windows build (ffmpeg included) and starts it.
# No Rust and no compiler needed. Uses the GitHub login that git already has.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'unknowncoder321/GeoProof'
$dest = Join-Path $env:USERPROFILE 'Desktop\GeoProof-App'

$cred = "protocol=https`nhost=github.com`n`n" | git credential fill
$token = ($cred | Where-Object { $_ -like 'password=*' } | Select-Object -First 1) -replace '^password=', ''
if (-not $token) { throw 'No GitHub login found. Run: git ls-remote https://github.com/unknowncoder321/GeoProof.git (sign in when asked), then retry.' }
$h = @{ Authorization = "Bearer $token"; 'User-Agent' = 'geoproof-install'; Accept = 'application/vnd.github+json' }

$list = Invoke-RestMethod -Headers $h -Uri "https://api.github.com/repos/$repo/actions/artifacts?name=geoproof-bundle&per_page=1"
$art = $list.artifacts | Select-Object -First 1
if (-not $art) { throw 'No build found yet. Open the Actions tab on GitHub and check the latest run.' }
Write-Host ("Downloading build from {0} ({1} MB)..." -f $art.created_at, [math]::Round($art.size_in_bytes / 1MB))

# GitHub answers with a redirect to a signed download link. Read it, then fetch it without the token.
$loc = $null
try {
    Invoke-WebRequest -Headers $h -Uri $art.archive_download_url -MaximumRedirection 0 -ErrorAction Stop | Out-Null
} catch {
    $r = $_.Exception.Response
    if ($r) { $loc = if ($r.Headers.Location) { [string]$r.Headers.Location } else { [string]$r.Headers['Location'] } }
}
if (-not $loc) { throw 'GitHub did not return a download link.' }
$zip = Join-Path $env:TEMP 'geoproof-bundle.zip'
Invoke-WebRequest -Uri $loc -OutFile $zip

if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $dest -Force
Remove-Item $zip -Force
$exe = Join-Path $dest 'geoproof.exe'
if (-not (Test-Path $exe)) { throw 'geoproof.exe is missing from the download.' }

$lnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'GeoProof.lnk'
$sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$sc.TargetPath = $exe
$sc.WorkingDirectory = $dest
$sc.Save()

Write-Host 'Starting GeoProof. A GeoProof shortcut is on your Desktop for next time.'
Start-Process -FilePath $exe -WorkingDirectory $dest
