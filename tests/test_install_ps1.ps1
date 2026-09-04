$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$built = Join-Path $root 'target\release\codeunlimited.exe'
if (-not (Test-Path -LiteralPath $built -PathType Leaf)) {
    throw "Build the release binary before running this test: $built"
}

$temp = Join-Path ([IO.Path]::GetTempPath()) ("codeunlimited-installer-" + [Guid]::NewGuid())
$release = Join-Path $temp 'release'
$dest = Join-Path $temp 'bin'
New-Item -ItemType Directory -Force -Path $release, $dest | Out-Null
$asset = Join-Path $release 'codeunlimited-windows-x86_64.exe'
Copy-Item -LiteralPath $built -Destination $asset
$digest = (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath "$asset.sha256" -Encoding ASCII -NoNewline -Value "$digest  codeunlimited-windows-x86_64.exe`n"

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$server = Start-Process python -ArgumentList @('-m', 'http.server', "$port", '--bind', '127.0.0.1') -WorkingDirectory $release -WindowStyle Hidden -PassThru
$oldUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')

function Invoke-Installer {
    $process = Start-Process powershell -ArgumentList @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', (Join-Path $root 'install.ps1')
    ) -Wait -PassThru -NoNewWindow
    return $process.ExitCode
}

try {
    $ready = $false
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$port/codeunlimited-windows-x86_64.exe" -Method Head | Out-Null
            $ready = $true
            break
        } catch {
            Start-Sleep -Milliseconds 100
        }
    }
    if (-not $ready) { throw 'Local fixture server did not become ready' }

    $env:CODEUNLIMITED_DOWNLOAD_BASE_URL = "http://127.0.0.1:$port"
    $env:CODEUNLIMITED_INSTALL_DIR = $dest
    if ((Invoke-Installer) -ne 0) { throw 'Valid installer run failed' }
    if ((Invoke-Installer) -ne 0) { throw 'Idempotent installer rerun failed' }
    $version = & (Join-Path $dest 'codeunlimited.exe') --version
    if ($version -ne 'codeunlimited 2.0.0') { throw "Unexpected installed version: $version" }
    $pathEntries = [Environment]::GetEnvironmentVariable('Path', 'User') -split ';'
    if (($pathEntries | Where-Object { $_ -eq $dest }).Count -ne 1) {
        throw 'Installer did not add exactly one user PATH entry'
    }

    $failureDest = Join-Path $temp 'rollback-bin'
    New-Item -ItemType Directory -Force -Path $failureDest | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $failureDest 'codeunlimited.exe') | Out-Null
    $pathBeforeFailure = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:CODEUNLIMITED_INSTALL_DIR = $failureDest
    if ((Invoke-Installer) -eq 0) { throw 'Blocked replacement unexpectedly succeeded' }
    $pathAfterFailure = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($pathAfterFailure -ne $pathBeforeFailure) {
        throw 'User PATH changed after failed replacement'
    }
    $stagedFiles = @(Get-ChildItem -LiteralPath $failureDest -Filter '.codeunlimited-install-*')
    if ($stagedFiles.Count -ne 0) { throw 'Failed replacement left a staged binary' }

    $env:CODEUNLIMITED_INSTALL_DIR = $dest
    [IO.File]::WriteAllBytes((Join-Path $dest 'codeunlimited.exe'), [Text.Encoding]::ASCII.GetBytes("existing verified binary`n"))
    Remove-Item -LiteralPath "$asset.sha256" -Force
    if ((Invoke-Installer) -eq 0) { throw 'Missing checksum unexpectedly succeeded' }
    $installed = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $dest 'codeunlimited.exe')))
    if ($installed -ne "existing verified binary`n") {
        throw 'Failed install replaced the existing binary'
    }
} finally {
    [Environment]::SetEnvironmentVariable('Path', $oldUserPath, 'User')
    Remove-Item Env:CODEUNLIMITED_DOWNLOAD_BASE_URL -ErrorAction SilentlyContinue
    Remove-Item Env:CODEUNLIMITED_INSTALL_DIR -ErrorAction SilentlyContinue
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'PowerShell installer integration tests passed.'
