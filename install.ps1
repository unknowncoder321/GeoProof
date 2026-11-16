# GeoProof no-admin installer.
# Downloads portable git + the latest GeoProof build. No admin, no winget, no compiler.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Join-Path $env:LOCALAPPDATA 'GeoProof'
if (-not (Test-Path $root)) { New-Item -ItemType Directory -Path $root | Out-Null }

# --- portable git so we can read git credentials for the private repo ---
$gitDir = Join-Path $root 'PortableGit'
$git = Join-Path $gitDir 'cmd\git.exe'
if (-not (Test-Path $git)) {
    Write-Host 'Downloading portable git (one-time, ~50 MB)...'
    $gitUrl = 'https://github.com/git-for-windows/git/releases/download/v2.47.0.windows.2/PortableGit-2.47.0.2-64-bit.7z.exe'
    $gitExe = Join-Path $env:TEMP 'PortableGit.exe'
    Invoke-WebRequest -Uri $gitUrl -OutFile $gitExe
    New-Item -ItemType Directory -Path $gitDir -Force | Out-Null
    & $gitExe -o"$gitDir" -y | Out-Null
    Remove-Item $gitExe -Force
}
$env:Path = "$gitDir\cmd;$gitDir\bin;" + $env:Path

# --- get a token from the stored credential manager ---
$repo = 'unknowncoder321/GeoProof'
$cred = "protocol=https`nhost=github.com`n`n" | & $git credential fill 2>$null
$token = ($cred -split "`n" | Where-Object { $_ -like 'password=*' } | Select-Object -First 1) -replace '^password=', ''
if (-not $token) {
    Write-Host ''
    Write-Host 'GitHub login needed. A browser window will open — sign in with your GitHub account.'
    Write-Host 'After signing in, close the browser and run this script again.'
    & $git ls-remote "https://github.com/$repo.git" HEAD | Out-Null
    exit 0
}

$h = @{
    Authorization = "Bearer $token"
    'User-Agent' = 'geoproof-install'
    Accept = 'application/vnd.github+json'
}

# --- find the latest build that has the bundle artifact ---
$list = Invoke-RestMethod -Headers $h -Uri "https://api.github.com/repos/$repo/actions/artifacts?name=geoproof-bundle&per_page=1"
$art = $list.artifacts | Select-Object -First 1
if (-not $art) { throw 'No build found. Open GitHub Actions and check if the latest run succeeded.' }
Write-Host ("Downloading GeoProof {0} ({1} MB)..." -f $art.created_at.Substring(0,10), [math]::Round($art.size_in_bytes/1MB))

# GitHub redirects artifact downloads to a signed S3 URL.
$loc = $null
try {
    Invoke-WebRequest -Headers $h -Uri $art.archive_download_url -MaximumRedirection 0 -ErrorAction Stop | Out-Null
} catch {
    $r = $_.Exception.Response
    if ($r) {
        try { $loc = [string]$r.Headers.Location } catch {}
        if (-not $loc) { try { $loc = [string]($r.Headers['Location']) } catch {} }
        if (-not $loc -and $r.Headers.Contains('Location')) { $loc = [string]($r.Headers.GetValues('Location')[0]) }
    }
}
if (-not $loc) {
    # Fallback: try the API without following redirects via WebClient
    try {
        $wc = New-Object System.Net.WebClient
        foreach ($k in $h.Keys) { $wc.Headers.Add($k, $h[$k]) }
        $wc.DownloadData($art.archive_download_url) | Out-Null
    } catch [System.Net.WebException] {
        $resp = $_.Exception.Response
        if ($resp) { $loc = [string]$resp.Headers['Location'] }
    } finally { $wc.Dispose() }
}
if (-not $loc) { throw 'Could not get download link from GitHub. Try again in a minute.' }

$zip = Join-Path $env:TEMP 'geoproof-bundle.zip'
Invoke-WebRequest -Uri $loc -OutFile $zip

$appDir = Join-Path $root 'App'
if (Test-Path $appDir) { Remove-Item $appDir -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $appDir -Force
Remove-Item $zip -Force

$exe = Join-Path $appDir 'geoproof.exe'
if (-not (Test-Path $exe)) { throw "geoproof.exe not found in $appDir — the build may be incomplete." }

$lnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'GeoProof.lnk'
$sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$sc.TargetPath = $exe
$sc.WorkingDirectory = $appDir
$sc.Save()

Write-Host 'Done. Starting GeoProof...'
Start-Process -FilePath $exe -WorkingDirectory $appDir
