# codeunlimited installer (Windows x86_64)
#   irm https://raw.githubusercontent.com/jimbokl/codeunlimited/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'

$repo = 'jimbokl/codeunlimited'
$asset = 'codeunlimited-windows-x86_64.exe'
$dest = if ($env:CODEUNLIMITED_INSTALL_DIR) { $env:CODEUNLIMITED_INSTALL_DIR }
        else { Join-Path $env:LOCALAPPDATA 'Programs\codeunlimited' }
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$exe = Join-Path $dest 'codeunlimited.exe'
$url = "https://github.com/$repo/releases/latest/download/$asset"

Write-Host "Downloading $asset ..."
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $exe

# Verify checksum when the release ships one.
try {
    $resp = Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256"
    # Windows PowerShell 5.1 returns unknown content types as raw bytes.
    $sum = if ($resp.Content -is [byte[]]) { [Text.Encoding]::ASCII.GetString($resp.Content) }
           else { [string]$resp.Content }
    $expected = ($sum.Trim() -split '\s+')[0].ToLower()
    if ($expected) {
        $actual = (Get-FileHash -Path $exe -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected) {
            Remove-Item $exe -Force
            throw "Checksum mismatch - aborting."
        }
    }
} catch {
    if ($_.Exception.Message -like '*Checksum mismatch*') { throw }
    # No checksum published or fetch failed - binary already downloaded over TLS.
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dest) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$dest", 'User')
    $env:Path = "$env:Path;$dest"
    Write-Host "Added to user PATH (new terminals pick it up automatically)."
}

Write-Host "Installed: $exe"
& $exe --version
Write-Host 'Next: codeunlimited audit'
