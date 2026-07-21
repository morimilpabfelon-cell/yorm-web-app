$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$composeFile = Join-Path $repoRoot "infra\docker\compose.yml"
$databaseUrl = "postgres://yorm:yorm_local_only@127.0.0.1:5432/yorm_pay?sslmode=disable"
$apiAddress = "127.0.0.1:8787"
$apiBase = "http://$apiAddress"
$apiExecutable = Join-Path $repoRoot "target\debug\yorm-api.exe"
$apiProcess = $null

function Start-YormApi {
    if (-not (Test-Path $apiExecutable)) {
        throw "Yorm API executable not found: $apiExecutable"
    }

    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    $script:apiProcess = Start-Process `
        -FilePath $apiExecutable `
        -WorkingDirectory $repoRoot `
        -PassThru `
        -WindowStyle Hidden

    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($script:apiProcess.HasExited) {
            throw "Yorm API exited before becoming ready."
        }

        try {
            $health = Invoke-RestMethod `
                -Method Get `
                -Uri "$apiBase/health/database" `
                -TimeoutSec 2
            if ($health.status -eq "ok" -and $health.backend -eq "postgres") {
                return
            }
        }
        catch {
            Start-Sleep -Milliseconds 250
        }
    }

    throw "Yorm API did not become ready at $apiBase."
}

function Stop-YormApi {
    if ($null -ne $script:apiProcess -and -not $script:apiProcess.HasExited) {
        Stop-Process -Id $script:apiProcess.Id -Force
        $script:apiProcess.WaitForExit()
    }
    $script:apiProcess = $null
}

function Invoke-AuthenticatedJson {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Token,
        [object]$Body,
        [string]$IdempotencyKey
    )

    $headers = @{ Authorization = "Bearer $Token" }
    if (-not [string]::IsNullOrWhiteSpace($IdempotencyKey)) {
        $headers["Idempotency-Key"] = $IdempotencyKey
    }

    $parameters = @{
        Method = $Method
        Uri = "$apiBase$Path"
        Headers = $headers
        TimeoutSec = 10
    }
    if ($null -ne $Body) {
        $parameters["ContentType"] = "application/json"
        $parameters["Body"] = ($Body | ConvertTo-Json -Compress)
    }

    Invoke-RestMethod @parameters
}

try {
    docker compose -f $composeFile up -d postgres | Out-Host
    docker compose -f $composeFile exec -T postgres pg_isready -U yorm -d yorm_pay | Out-Host

    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    cargo build --workspace | Out-Host

    Start-YormApi

    $suffix = [Guid]::NewGuid().ToString("N")
    $identity = Invoke-RestMethod `
        -Method Post `
        -Uri "$apiBase/v1/sandbox/identities" `
        -ContentType "application/json" `
        -Body (@{
            email = "wallet-$suffix@yorm.local"
            display_name = "Wallet Ledger Sandbox"
            country_code = "PE"
        } | ConvertTo-Json -Compress)

    $session = Invoke-RestMethod `
        -Method Post `
        -Uri "$apiBase/v1/sandbox/sessions" `
        -ContentType "application/json" `
        -Body (@{ identity_id = $identity.id } | ConvertTo-Json -Compress)

    $token = [string]$session.access_token
    Invoke-AuthenticatedJson `
        -Method Put `
        -Path "/v1/me/pin" `
        -Token $token `
        -Body @{ pin = "4096" } | Out-Null

    $wallet = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/me/wallet" `
        -Token $token

    $idempotencyKey = "wallet-credit-$suffix"
    $credit = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/sandbox/wallet/credits" `
        -Token $token `
        -IdempotencyKey $idempotencyKey `
        -Body @{ amount_minor_units = "1250" }

    Stop-YormApi
    Start-YormApi

    $persistedWallet = Invoke-AuthenticatedJson `
        -Method Get `
        -Path "/v1/me/wallet" `
        -Token $token

    $replay = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/sandbox/wallet/credits" `
        -Token $token `
        -IdempotencyKey $idempotencyKey `
        -Body @{ amount_minor_units = "1250" }

    $conflictDetected = $false
    try {
        Invoke-AuthenticatedJson `
            -Method Post `
            -Path "/v1/sandbox/wallet/credits" `
            -Token $token `
            -IdempotencyKey $idempotencyKey `
            -Body @{ amount_minor_units = "1300" } | Out-Null
    }
    catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 409) {
            $conflictDetected = $true
        }
        else {
            throw
        }
    }

    $transactionId = [string]$credit.transaction_id
    $ledgerTotals = docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -At -F ',' -c `
        "SELECT COUNT(*), COALESCE(SUM(amount_minor) FILTER (WHERE entry_side='debit'),0), COALESCE(SUM(amount_minor) FILTER (WHERE entry_side='credit'),0) FROM ledger_entries WHERE transaction_id='$transactionId';"
    $ledgerParts = ($ledgerTotals.Trim() -split ',')

    $balanceColumns = docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -At -c `
        "SELECT COUNT(*) FROM information_schema.columns WHERE table_schema='public' AND table_name='sandbox_wallets' AND column_name ILIKE '%balance%';"

    [PSCustomObject]@{
        database_backend = "postgres"
        wallet_id = [string]$wallet.id
        wallet_currency = [string]$wallet.currency
        initial_balance_minor_units = [string]$wallet.balance_minor_units
        credited_amount_minor_units = [string]$credit.amount_minor_units
        balance_after_minor_units = [string]$credit.balance_after_minor_units
        wallet_persisted = ([string]$persistedWallet.id -eq [string]$wallet.id)
        balance_persisted = ([string]$persistedWallet.balance_minor_units -eq "1250")
        idempotent_transaction_reused = ([string]$replay.transaction_id -eq $transactionId)
        idempotency_conflict_detected = $conflictDetected
        ledger_entry_count = [int64]$ledgerParts[0]
        ledger_debit_total = [int64]$ledgerParts[1]
        ledger_credit_total = [int64]$ledgerParts[2]
        wallet_balance_columns = [int64]$balanceColumns.Trim()
        real_money_enabled = $false
        transfers_enabled = $false
    } | Format-List
}
finally {
    Stop-YormApi
}
