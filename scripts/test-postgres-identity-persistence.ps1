param(
    [string]$BaseUrl = "http://127.0.0.1:8787",
    [string]$DatabaseUrl = "postgres://yorm:yorm_local_only@127.0.0.1:5432/yorm_pay?sslmode=disable"
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$composeFile = Join-Path $repoRoot "infra/docker/compose.yml"
$apiExecutable = Join-Path $repoRoot "target/debug/yorm-api.exe"
$baseUri = [Uri]$BaseUrl
$apiAddress = "$($baseUri.Host):$($baseUri.Port)"
$apiProcess = $null
$stdoutLog = Join-Path $env:TEMP "yorm-api-foundation1b.stdout.log"
$stderrLog = Join-Path $env:TEMP "yorm-api-foundation1b.stderr.log"

function Stop-YormApi {
    param([System.Diagnostics.Process]$Process)

    if ($null -ne $Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        Wait-Process -Id $Process.Id -ErrorAction SilentlyContinue
    }
}

function Wait-YormDatabase {
    for ($attempt = 1; $attempt -le 30; $attempt++) {
        & docker compose -f $composeFile exec -T postgres `
            pg_isready -U yorm -d yorm_pay *> $null

        if ($LASTEXITCODE -eq 0) {
            return
        }

        Start-Sleep -Seconds 2
    }

    throw "PostgreSQL did not become ready within 60 seconds."
}

function Start-YormApi {
    Remove-Item $stdoutLog, $stderrLog -Force -ErrorAction SilentlyContinue

    $process = Start-Process `
        -FilePath $apiExecutable `
        -WorkingDirectory $repoRoot `
        -RedirectStandardOutput $stdoutLog `
        -RedirectStandardError $stderrLog `
        -PassThru

    for ($attempt = 1; $attempt -le 40; $attempt++) {
        if ($process.HasExited) {
            $stderr = Get-Content $stderrLog -Raw -ErrorAction SilentlyContinue
            throw "Yorm API exited before becoming ready. $stderr"
        }

        try {
            $health = Invoke-RestMethod -Method Get -Uri "$BaseUrl/health/database"
            if ($health.status -eq "ok" -and $health.backend -eq "postgres") {
                return $process
            }
        } catch {
            Start-Sleep -Milliseconds 500
        }
    }

    Stop-YormApi -Process $process
    throw "Yorm API did not become ready within 20 seconds."
}

try {
    try {
        Invoke-RestMethod -Method Get -Uri "$BaseUrl/health" | Out-Null
        throw "Port $($baseUri.Port) is already in use. Stop the existing API before running this script."
    } catch {
        if ($_.Exception.Message -like "Port * is already in use*") {
            throw
        }
    }

    & docker compose -f $composeFile up -d postgres
    if ($LASTEXITCODE -ne 0) {
        throw "Docker Compose could not start PostgreSQL."
    }
    Wait-YormDatabase

    Push-Location $repoRoot
    try {
        cargo build -p yorm-api
        if ($LASTEXITCODE -ne 0) {
            throw "The Yorm API build failed."
        }
    } finally {
        Pop-Location
    }

    $env:DATABASE_URL = $DatabaseUrl
    $env:YORM_API_ADDR = $apiAddress

    $apiProcess = Start-YormApi

    $email = "persist-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())@yorm.local"
    $identity = Invoke-RestMethod `
        -Method Post `
        -Uri "$BaseUrl/v1/sandbox/identities" `
        -ContentType "application/json" `
        -Body (@{
            email = $email
            display_name = "Persistent Sandbox"
            country_code = "PE"
        } | ConvertTo-Json)

    $session = Invoke-RestMethod `
        -Method Post `
        -Uri "$BaseUrl/v1/sandbox/sessions" `
        -ContentType "application/json" `
        -Body (@{
            identity_id = $identity.id
        } | ConvertTo-Json)

    $headers = @{
        Authorization = "Bearer $($session.access_token)"
    }

    Invoke-RestMethod `
        -Method Put `
        -Uri "$BaseUrl/v1/me/pin" `
        -Headers $headers `
        -ContentType "application/json" `
        -Body (@{
            pin = "4096"
        } | ConvertTo-Json) | Out-Null

    Stop-YormApi -Process $apiProcess
    $apiProcess = Start-YormApi

    $profileAfterRestart = Invoke-RestMethod `
        -Method Get `
        -Uri "$BaseUrl/v1/me" `
        -Headers $headers

    $pinAfterRestart = Invoke-RestMethod `
        -Method Post `
        -Uri "$BaseUrl/v1/me/pin/verify" `
        -Headers $headers `
        -ContentType "application/json" `
        -Body (@{
            pin = "4096"
        } | ConvertTo-Json)

    $limits = Invoke-RestMethod `
        -Method Get `
        -Uri "$BaseUrl/v1/me/limits" `
        -Headers $headers

    $pinPrefix = (& docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -tAc `
        "SELECT left(pin_hash, 7) FROM sandbox_identities WHERE id = '$($identity.id)';").Trim()

    $tokenDigestLength = (& docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -tAc `
        "SELECT char_length(token_digest) FROM sandbox_sessions WHERE identity_id = '$($identity.id)';").Trim()

    $rawTokenColumnCount = (& docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -tAc `
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'sandbox_sessions' AND column_name IN ('access_token', 'token');").Trim()

    Invoke-RestMethod `
        -Method Delete `
        -Uri "$BaseUrl/v1/me/session" `
        -Headers $headers | Out-Null

    Stop-YormApi -Process $apiProcess
    $apiProcess = Start-YormApi

    $revocationPersisted = $false
    try {
        Invoke-RestMethod `
            -Method Get `
            -Uri "$BaseUrl/v1/me" `
            -Headers $headers | Out-Null
    } catch {
        $statusCode = [int]$_.Exception.Response.StatusCode
        if ($statusCode -ne 401) {
            throw
        }

        $revocationPersisted = $true
    }

    if (-not $revocationPersisted) {
        throw "The revoked session was accepted after restarting the API."
    }

    [PSCustomObject]@{
        database_backend = "postgres"
        identity_persisted = ($profileAfterRestart.id -eq $identity.id)
        session_persisted = $true
        pin_persisted = $pinAfterRestart.verified
        pin_hash_argon2 = ($pinPrefix -eq '$argon2')
        session_digest_length = [int]$tokenDigestLength
        raw_token_columns = [int]$rawTokenColumnCount
        limits_currency = $limits.currency
        payments_enabled = $limits.payments_enabled
        transfers_enabled = $limits.transfers_enabled
        session_revocation_persisted = $revocationPersisted
    } | Format-List

    & docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -c `
        "DELETE FROM sandbox_identities WHERE id = '$($identity.id)';" *> $null
} finally {
    Stop-YormApi -Process $apiProcess
    Remove-Item $stdoutLog, $stderrLog -Force -ErrorAction SilentlyContinue
}
