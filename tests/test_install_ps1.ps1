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
$oldClaudeConfig = $env:CLAUDE_CONFIG_DIR
$oldCodexHome = $env:CODEX_HOME
$oldSkipSetup = $env:CODEUNLIMITED_SKIP_SETUP

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
    $env:CLAUDE_CONFIG_DIR = Join-Path $temp 'claude'
    $env:CODEX_HOME = Join-Path $temp 'codex'
    $env:CODEUNLIMITED_SKIP_SETUP = '0'
    if ((Invoke-Installer) -ne 0) { throw 'Valid installer run failed' }
    if ((Invoke-Installer) -ne 0) { throw 'Idempotent installer rerun failed' }
    $status = & (Join-Path $dest 'codeunlimited.exe') setup --status --json | ConvertFrom-Json
    if (-not $status.enabled) { throw 'Installer did not activate global defaults' }
    if ($status.codex_tool_output_token_limit -ne 4000) { throw 'Missing automatic tool-output cap' }
    $version = & (Join-Path $dest 'codeunlimited.exe') --version
    $manifest = Get-Content (Join-Path $PSScriptRoot '..\Cargo.toml') -Raw
    if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Could not read version from Cargo.toml' }
    $expected = "codeunlimited $($Matches[1])"
    if ($version -ne $expected) { throw "Unexpected installed version: $version (expected $expected)" }
    $pathEntries = [Environment]::GetEnvironmentVariable('Path', 'User') -split ';'
    if (($pathEntries | Where-Object { $_ -eq $dest }).Count -ne 1) {
        throw 'Installer did not add exactly one user PATH entry'
    }

    $env:CLAUDE_CONFIG_DIR = Join-Path $temp 'skip-claude'
    $env:CODEX_HOME = Join-Path $temp 'skip-codex'
    $env:CODEUNLIMITED_SKIP_SETUP = '1'
    if ((Invoke-Installer) -ne 0) { throw 'Binary-only installation failed' }
    if ((Test-Path $env:CLAUDE_CONFIG_DIR) -or (Test-Path $env:CODEX_HOME)) {
        throw 'Binary-only installer changed provider homes'
    }
    $env:CODEUNLIMITED_SKIP_SETUP = '0'
    New-Item -ItemType Directory -Force -Path $env:CODEX_HOME | Out-Null
    [IO.File]::WriteAllText((Join-Path $env:CODEX_HOME 'config.toml'), 'invalid = [')
    if ((Invoke-Installer) -eq 0) { throw 'Activation failure was reported as success' }
    if (Test-Path (Join-Path $env:CLAUDE_CONFIG_DIR 'CLAUDE.md')) {
        throw 'Failed preflight changed Claude instructions'
    }
    if ((Get-FileHash -LiteralPath (Join-Path $dest 'codeunlimited.exe') -Algorithm SHA256).Hash.ToLower() -ne $digest) {
        throw 'Activation failure damaged the verified binary'
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
    $env:CLAUDE_CONFIG_DIR = $oldClaudeConfig
    $env:CODEX_HOME = $oldCodexHome
    $env:CODEUNLIMITED_SKIP_SETUP = $oldSkipSetup
    Remove-Item Env:CODEUNLIMITED_DOWNLOAD_BASE_URL -ErrorAction SilentlyContinue
    Remove-Item Env:CODEUNLIMITED_INSTALL_DIR -ErrorAction SilentlyContinue
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host 'PowerShell installer integration tests passed.'
