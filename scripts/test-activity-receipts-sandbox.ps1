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
    $script:apiProcess = Start-Process -FilePath $apiExecutable -WorkingDirectory $repoRoot -PassThru -WindowStyle Hidden
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($script:apiProcess.HasExited) { throw "Yorm API exited before becoming ready." }
        try {
            $health = Invoke-RestMethod -Method Get -Uri "$apiBase/health/database" -TimeoutSec 2
            if ($health.status -eq "ok" -and $health.backend -eq "postgres") { return }
        }
        catch { Start-Sleep -Milliseconds 250 }
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

function Invoke-Json {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$Token,
        [object]$Body,
        [string]$IdempotencyKey
    )
    $headers = @{}
    if (-not [string]::IsNullOrWhiteSpace($Token)) { $headers["Authorization"] = "Bearer $Token" }
    if (-not [string]::IsNullOrWhiteSpace($IdempotencyKey)) { $headers["Idempotency-Key"] = $IdempotencyKey }
    $parameters = @{ Method = $Method; Uri = "$apiBase$Path"; Headers = $headers; TimeoutSec = 10 }
    if ($null -ne $Body) {
        $parameters["ContentType"] = "application/json"
        $parameters["Body"] = ($Body | ConvertTo-Json -Compress)
    }
    Invoke-RestMethod @parameters
}

function New-Actor {
    param([Parameter(Mandatory = $true)][string]$Name, [bool]$ConfigurePin)
    $suffix = [Guid]::NewGuid().ToString("N")
    $identity = Invoke-Json -Method Post -Path "/v1/sandbox/identities" -Body @{
        email = "activity-$suffix@yorm.local"
        display_name = $Name
        country_code = "PE"
    }
    $session = Invoke-Json -Method Post -Path "/v1/sandbox/sessions" -Body @{ identity_id = $identity.id }
    $token = [string]$session.access_token
    if ($ConfigurePin) {
        Invoke-Json -Method Put -Path "/v1/me/pin" -Token $token -Body @{ pin = "4096" } | Out-Null
    }
    $wallet = Invoke-Json -Method Post -Path "/v1/me/wallet" -Token $token
    [PSCustomObject]@{ identity = $identity; token = $token; wallet = $wallet }
}

try {
    docker compose -f $composeFile up -d postgres | Out-Host
    docker compose -f $composeFile exec -T postgres pg_isready -U yorm -d yorm_pay | Out-Host
    $env:DATABASE_URL = $databaseUrl
    $env:YORM_API_ADDR = $apiAddress
    cargo build --workspace | Out-Host
    Start-YormApi

    $sender = New-Actor -Name "Activity Sender" -ConfigurePin $true
    $recipient = New-Actor -Name "Activity Recipient" -ConfigurePin $false
    $outsider = New-Actor -Name "Activity Outsider" -ConfigurePin $false
    $suffix = [Guid]::NewGuid().ToString("N")

    $credit = Invoke-Json -Method Post -Path "/v1/sandbox/wallet/credits" -Token $sender.token `
        -IdempotencyKey "activity-credit-$suffix" -Body @{ amount_minor_units = "2000" }
    $transfer = Invoke-Json -Method Post -Path "/v1/sandbox/transfers" -Token $sender.token `
        -IdempotencyKey "activity-transfer-$suffix" -Body @{
            recipient_identity_id = $recipient.identity.id
            amount_minor_units = "750"
        }

    $pageOne = Invoke-Json -Method Get -Path "/v1/me/activity?limit=1" -Token $sender.token
    $pageTwo = Invoke-Json -Method Get -Path "/v1/me/activity?limit=1&cursor=$($pageOne.next_cursor)" -Token $sender.token
    $senderItems = @($pageOne.items) + @($pageTwo.items)
    $senderTransfer = $senderItems | Where-Object { $_.transaction_id -eq $transfer.transaction_id }
    $senderCredit = $senderItems | Where-Object { $_.transaction_id -eq $credit.transaction_id }

    $recipientActivity = Invoke-Json -Method Get -Path "/v1/me/activity" -Token $recipient.token
    $senderReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $sender.token
    $recipientReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $recipient.token

    $outsiderBlocked = $false
    try {
        Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $outsider.token | Out-Null
    }
    catch {
        if ($_.Exception.Response.StatusCode.value__ -eq 404) { $outsiderBlocked = $true } else { throw }
    }

    $transactionCountBefore = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_transactions;"
    $entryCountBefore = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_entries;"
    $projectionTableCount = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='public' AND table_name IN ('pay_activity','pay_receipts','activity','receipts');"

    Stop-YormApi
    Start-YormApi

    $persistedReceipt = Invoke-Json -Method Get -Path "/v1/me/receipts/$($transfer.transaction_id)" -Token $sender.token
    $persistedActivity = Invoke-Json -Method Get -Path "/v1/me/activity" -Token $sender.token
    $transactionCountAfter = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_transactions;"
    $entryCountAfter = docker compose -f $composeFile exec -T postgres psql -U yorm -d yorm_pay -At -c "SELECT COUNT(*) FROM ledger_entries;"

    [PSCustomObject]@{
        database_backend = "postgres"
        activity_module = [string]$pageOne.module
        receipt_module = [string]$senderReceipt.module
        activity_page_one_count = @($pageOne.items).Count
        activity_page_two_count = @($pageTwo.items).Count
        stable_cursor_present = (-not [string]::IsNullOrWhiteSpace([string]$pageOne.next_cursor))
        sender_credit_direction = [string]$senderCredit.direction
        sender_transfer_direction = [string]$senderTransfer.direction
        recipient_transfer_direction = [string]$recipientActivity.items[0].direction
        sender_balance_after_minor_units = [string]$senderTransfer.balance_after_minor_units
        recipient_balance_after_minor_units = [string]$recipientActivity.items[0].balance_after_minor_units
        counterparty_visible = ([string]$senderTransfer.counterparty.identity_id -eq [string]$recipient.identity.id)
        outsider_receipt_blocked = $outsiderBlocked
        receipt_reference_length = ([string]$senderReceipt.receipt_reference).Length
        receipt_perspective_distinct = ([string]$senderReceipt.receipt_reference -ne [string]$recipientReceipt.receipt_reference)
        ledger_entry_count = [int64]$senderReceipt.ledger_entry_count
        ledger_debit_total = [int64]$senderReceipt.ledger_debit_total_minor_units
        ledger_credit_total = [int64]$senderReceipt.ledger_credit_total_minor_units
        receipt_persisted = ([string]$persistedReceipt.receipt_reference -eq [string]$senderReceipt.receipt_reference)
        activity_persisted_count = @($persistedActivity.items).Count
        transaction_count_unchanged = ([int64]$transactionCountBefore.Trim() -eq [int64]$transactionCountAfter.Trim())
        entry_count_unchanged = ([int64]$entryCountBefore.Trim() -eq [int64]$entryCountAfter.Trim())
        projection_table_count = [int64]$projectionTableCount.Trim()
        real_money_enabled = $false
        external_providers_enabled = $false
    } | Format-List
}
finally {
    Stop-YormApi
}
