# codeunlimited installer (Windows x86_64)
#   irm https://raw.githubusercontent.com/jimbokl/codeunlimited/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'

$repo = 'jimbokl/codeunlimited'
$asset = 'codeunlimited-windows-x86_64.exe'
$dest = if ($env:CODEUNLIMITED_INSTALL_DIR) { $env:CODEUNLIMITED_INSTALL_DIR }
        else { Join-Path $env:LOCALAPPDATA 'Programs\codeunlimited' }
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$exe = Join-Path $dest 'codeunlimited.exe'
$baseUrl = if ($env:CODEUNLIMITED_DOWNLOAD_BASE_URL) {
    $env:CODEUNLIMITED_DOWNLOAD_BASE_URL.TrimEnd('/')
} else {
    "https://github.com/$repo/releases/latest/download"
}
$url = "$baseUrl/$asset"
$temp = Join-Path $dest ('.codeunlimited-download-' + [Guid]::NewGuid())
$download = Join-Path $temp $asset
$sumFile = "$download.sha256"
$staged = Join-Path $dest ('.codeunlimited-install-' + [Guid]::NewGuid() + '.exe')

try {
    New-Item -ItemType Directory -Force -Path $temp | Out-Null
    Write-Host "Downloading $asset ..."
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $download
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile $sumFile
    } catch {
        throw 'Checksum download failed - preserving the existing installation.'
    }

    $sum = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($sumFile))
    $match = [Regex]::Match($sum, '^\s*([0-9a-fA-F]{64})(?:\s|$)')
    if (-not $match.Success) {
        throw 'Malformed checksum - preserving the existing installation.'
    }
    $expected = $match.Groups[1].Value.ToLower()
    $actual = (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) {
        throw 'Checksum mismatch - preserving the existing installation.'
    }

    $versionOutput = & $download --version 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw 'Downloaded binary failed its smoke test - preserving the existing installation.'
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $pathEntries = @($userPath -split ';' | Where-Object { $_ })
    if ($pathEntries -notcontains $dest) {
        $nextUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
            $dest
        } else {
            "$userPath;$dest"
        }
        [Environment]::SetEnvironmentVariable('Path', $nextUserPath, 'User')
        $env:Path = "$env:Path;$dest"
        Write-Host "Added to user PATH (new terminals pick it up automatically)."
    }

    Copy-Item -LiteralPath $download -Destination $staged
    if (Test-Path -LiteralPath $exe -PathType Leaf) {
        [IO.File]::Replace($staged, $exe, $null)
    } else {
        [IO.File]::Move($staged, $exe)
    }

    Write-Host "Installed: $exe"
    Write-Host $versionOutput
    Write-Host 'Next: codeunlimited audit'
} finally {
    Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
