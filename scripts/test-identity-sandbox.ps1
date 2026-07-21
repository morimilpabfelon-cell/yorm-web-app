param(
    [string]$BaseUrl = "http://127.0.0.1:8787"
)

$ErrorActionPreference = "Stop"

$email = "sandbox-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())@yorm.local"

$identity = Invoke-RestMethod `
    -Method Post `
    -Uri "$BaseUrl/v1/sandbox/identities" `
    -ContentType "application/json" `
    -Body (@{
        email = $email
        display_name = "Yorm Sandbox"
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

$profileBeforePin = Invoke-RestMethod `
    -Method Get `
    -Uri "$BaseUrl/v1/me" `
    -Headers $headers

Invoke-RestMethod `
    -Method Put `
    -Uri "$BaseUrl/v1/me/pin" `
    -Headers $headers `
    -ContentType "application/json" `
    -Body (@{
        pin = "4096"
    } | ConvertTo-Json) | Out-Null

$pinVerification = Invoke-RestMethod `
    -Method Post `
    -Uri "$BaseUrl/v1/me/pin/verify" `
    -Headers $headers `
    -ContentType "application/json" `
    -Body (@{
        pin = "4096"
    } | ConvertTo-Json)

$profileAfterPin = Invoke-RestMethod `
    -Method Get `
    -Uri "$BaseUrl/v1/me" `
    -Headers $headers

$limits = Invoke-RestMethod `
    -Method Get `
    -Uri "$BaseUrl/v1/me/limits" `
    -Headers $headers

Invoke-RestMethod `
    -Method Delete `
    -Uri "$BaseUrl/v1/me/session" `
    -Headers $headers | Out-Null

$revocationConfirmed = $false
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

    $revocationConfirmed = $true
}

if (-not $revocationConfirmed) {
    throw "The revoked session was unexpectedly accepted."
}

[PSCustomObject]@{
    identity_id = $identity.id
    email = $identity.email
    profile_before_pin = $profileBeforePin.pin_configured
    profile_after_pin = $profileAfterPin.pin_configured
    pin_verified = $pinVerification.verified
    limits_module = $limits.module
    limits_currency = $limits.currency
    payments_enabled = $limits.payments_enabled
    transfers_enabled = $limits.transfers_enabled
    session_revoked = $revocationConfirmed
} | Format-List
