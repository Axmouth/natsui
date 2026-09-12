param([int]$Port = 4321)
$ErrorActionPreference = 'Stop'
$projectDir = Split-Path $PSScriptRoot -Parent
$keys = @('NATSUI_URL','NATSUI_PROFILE','NATSUI_DATA_DIR','NATSUI_PORT','NATSUI_CREDS','NATSUI_DOMAIN','NATSUI_MONITOR_URLS','NATSUI_TLS_REQUIRED','NATSUI_TLS_CA','NATSUI_TLS_CERT','NATSUI_TLS_KEY','NATSUI_CONTAINER')
$previous = @{}
foreach ($key in $keys) { $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
Push-Location $projectDir
try {
    docker compose -f demo/compose.yaml up --build -d --wait
    if ($LASTEXITCODE -ne 0) { throw 'Demo cluster failed to start. Inspect docker compose -f demo/compose.yaml logs.' }
    $env:NATSUI_URL = 'nats://127.0.0.1:14222'
    $env:NATSUI_MONITOR_URLS = 'http://127.0.0.1:18222,http://127.0.0.1:18223,http://127.0.0.1:18224'
    $env:NATSUI_PROFILE = 'Demo cluster / real NATS'
    $env:NATSUI_DATA_DIR = Join-Path $projectDir 'data/live-demo'
    $env:NATSUI_PORT = "$Port"
    Remove-Item Env:NATSUI_CREDS -ErrorAction SilentlyContinue
    Remove-Item Env:NATSUI_DOMAIN -ErrorAction SilentlyContinue
    foreach ($key in @('NATSUI_TLS_REQUIRED','NATSUI_TLS_CA','NATSUI_TLS_CERT','NATSUI_TLS_KEY','NATSUI_CONTAINER')) { Remove-Item "Env:$key" -ErrorAction SilentlyContinue }
    cargo run --locked
    if ($LASTEXITCODE -ne 0) { throw 'Dashboard exited with an error. Demo containers remain running.' }
} finally {
    foreach ($key in $keys) { [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process') }
    Pop-Location
}
