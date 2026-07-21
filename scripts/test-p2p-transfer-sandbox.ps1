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

function New-SandboxActor {
    param(
        [Parameter(Mandatory = $true)][string]$Suffix,
        [Parameter(Mandatory = $true)][string]$Role,
        [bool]$ConfigurePin = $false
    )

    $identity = Invoke-RestMethod `
        -Method Post `
        -Uri "$apiBase/v1/sandbox/identities" `
        -ContentType "application/json" `
        -Body (@{
            email = "p2p-$Role-$Suffix@yorm.local"
            display_name = "P2P $Role"
            country_code = "PE"
        } | ConvertTo-Json -Compress)

    $session = Invoke-RestMethod `
        -Method Post `
        -Uri "$apiBase/v1/sandbox/sessions" `
        -ContentType "application/json" `
        -Body (@{ identity_id = $identity.id } | ConvertTo-Json -Compress)

    $token = [string]$session.access_token
    if ($ConfigurePin) {
        Invoke-AuthenticatedJson `
            -Method Put `
            -Path "/v1/me/pin" `
            -Token $token `
            -Body @{ pin = "4096" } | Out-Null
    }

    $wallet = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/me/wallet" `
        -Token $token

    [PSCustomObject]@{
        IdentityId = [string]$identity.id
        Token = $token
        WalletId = [string]$wallet.id
        InitialBalance = [string]$wallet.balance_minor_units
    }
}

try {
    docker compose -f $composeFile up -d postgres | Out-Host
    docker compose -f $composeFile exec -T postgres pg_isready -U yorm -d yorm_pay | Out-Host

    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    cargo build --workspace | Out-Host

    Start-YormApi

    $suffix = [Guid]::NewGuid().ToString("N")
    $sender = New-SandboxActor -Suffix $suffix -Role "sender" -ConfigurePin $true
    $recipient = New-SandboxActor -Suffix $suffix -Role "recipient"

    $credit = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/sandbox/wallet/credits" `
        -Token $sender.Token `
        -IdempotencyKey "p2p-credit-$suffix" `
        -Body @{ amount_minor_units = "2000" }

    $transferKey = "p2p-transfer-$suffix"
    $transfer = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/sandbox/transfers" `
        -Token $sender.Token `
        -IdempotencyKey $transferKey `
        -Body @{
            recipient_identity_id = $recipient.IdentityId
            amount_minor_units = "750"
        }

    Stop-YormApi
    Start-YormApi

    $senderWallet = Invoke-AuthenticatedJson `
        -Method Get `
        -Path "/v1/me/wallet" `
        -Token $sender.Token
    $recipientWallet = Invoke-AuthenticatedJson `
        -Method Get `
        -Path "/v1/me/wallet" `
        -Token $recipient.Token

    $replay = Invoke-AuthenticatedJson `
        -Method Post `
        -Path "/v1/sandbox/transfers" `
        -Token $sender.Token `
        -IdempotencyKey $transferKey `
        -Body @{
            recipient_identity_id = $recipient.IdentityId
            amount_minor_units = "750"
        }

    $conflictDetected = $false
    try {
        Invoke-AuthenticatedJson `
            -Method Post `
            -Path "/v1/sandbox/transfers" `
            -Token $sender.Token `
            -IdempotencyKey $transferKey `
            -Body @{
                recipient_identity_id = $recipient.IdentityId
                amount_minor_units = "751"
            } | Out-Null
    }
    catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 409) {
            $conflictDetected = $true
        }
        else {
            throw
        }
    }

    $insufficientFundsDetected = $false
    try {
        Invoke-AuthenticatedJson `
            -Method Post `
            -Path "/v1/sandbox/transfers" `
            -Token $sender.Token `
            -IdempotencyKey "p2p-insufficient-$suffix" `
            -Body @{
                recipient_identity_id = $recipient.IdentityId
                amount_minor_units = "2000"
            } | Out-Null
    }
    catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 409) {
            $insufficientFundsDetected = $true
        }
        else {
            throw
        }
    }

    $transactionId = [string]$transfer.transaction_id
    $ledgerTotals = docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -At -F ',' -c `
        "SELECT COUNT(*), COALESCE(SUM(amount_minor) FILTER (WHERE entry_side='debit'),0), COALESCE(SUM(amount_minor) FILTER (WHERE entry_side='credit'),0) FROM ledger_entries WHERE transaction_id='$transactionId';"
    $ledgerParts = ($ledgerTotals.Trim() -split ',')

    $metadata = docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -At -F ',' -c `
        "SELECT sender_wallet_id, recipient_wallet_id, amount_minor, sender_balance_after_minor, recipient_balance_after_minor FROM sandbox_p2p_transfers WHERE transaction_id='$transactionId';"
    $metadataParts = ($metadata.Trim() -split ',')

    $transferCount = docker compose -f $composeFile exec -T postgres `
        psql -U yorm -d yorm_pay -At -c `
        "SELECT COUNT(*) FROM sandbox_p2p_transfers WHERE sender_wallet_id='$($sender.WalletId)';"

    [PSCustomObject]@{
        database_backend = "postgres"
        transaction_kind = [string]$transfer.transaction_kind
        currency = [string]$transfer.currency
        sender_initial_balance_minor_units = [string]$sender.InitialBalance
        credited_amount_minor_units = [string]$credit.amount_minor_units
        transferred_amount_minor_units = [string]$transfer.amount_minor_units
        sender_balance_after_minor_units = [string]$transfer.sender_balance_after_minor_units
        recipient_balance_after_minor_units = [string]$transfer.recipient_balance_after_minor_units
        sender_balance_persisted = ([string]$senderWallet.balance_minor_units -eq "1250")
        recipient_balance_persisted = ([string]$recipientWallet.balance_minor_units -eq "750")
        idempotent_transaction_reused = ([string]$replay.transaction_id -eq $transactionId)
        idempotency_conflict_detected = $conflictDetected
        insufficient_funds_detected = $insufficientFundsDetected
        ledger_entry_count = [int64]$ledgerParts[0]
        ledger_debit_total = [int64]$ledgerParts[1]
        ledger_credit_total = [int64]$ledgerParts[2]
        metadata_sender_wallet_matches = ($metadataParts[0] -eq $sender.WalletId)
        metadata_recipient_wallet_matches = ($metadataParts[1] -eq $recipient.WalletId)
        metadata_amount_minor = [int64]$metadataParts[2]
        metadata_sender_balance_after = [int64]$metadataParts[3]
        metadata_recipient_balance_after = [int64]$metadataParts[4]
        sender_transfer_count = [int64]$transferCount.Trim()
        real_money_enabled = $false
        external_providers_enabled = $false
    } | Format-List
}
finally {
    Stop-YormApi
}
