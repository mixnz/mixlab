# T197: the Windows install script's choices, checked without an installer. Runs on Windows
# PowerShell 5.1 and on PowerShell 7, including pwsh on Linux for the parts that are not Windows'.
# Helper names use approved verbs, because PSScriptAnalyzer checks this file too.
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
$env:MIXLAB_INSTALL_SOURCED = '1'
. (Join-Path $root 'packaging/install/install.ps1')

$script:failures = 0
function Assert-Same([string]$What, $Expected, $Actual) {
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL $What`n  expected: $Expected`n  actual:   $Actual"
        $script:failures++
    }
}
function Test-Refused([scriptblock]$Block) {
    try { & $Block | Out-Null; 'accepted' } catch { 'refused' }
}

$pub = (Get-Content (Join-Path $root 'packaging/updates.pub'))[1].Trim()
Assert-Same 'the embedded key is packaging/updates.pub''s' $pub (Get-MixLabConfig).PubKey

Assert-Same 'a plain version' '0.0.13' (ConvertTo-MixLabVersion -Raw '0.0.13')
Assert-Same 'a tag' '0.0.13' (ConvertTo-MixLabVersion -Raw 'v0.0.13')
Assert-Same 'two numbers' 'refused' (Test-Refused { ConvertTo-MixLabVersion -Raw '0.14' })

Assert-Same 'X64' 'x86_64' (Get-MixLabArch -Raw 'X64')
Assert-Same 'Arm64' 'aarch64' (Get-MixLabArch -Raw 'Arm64')
Assert-Same 'X86 is refused' 'refused' (Test-Refused { Get-MixLabArch -Raw 'X86' })
Assert-Same 'no answer is refused' 'refused' (Test-Refused { Get-MixLabArch -Raw '' })

# Windows 10 on .NET before 4.7.1: RuntimeInformation answers nothing, and the environment decides.
Assert-Same 'an x64 process' 'X64' (ConvertFrom-MixLabProcessorArch -Native '' -Process 'AMD64')
Assert-Same 'an arm64 process' 'Arm64' (ConvertFrom-MixLabProcessorArch -Native '' -Process 'ARM64')
Assert-Same 'a 32-bit process on x64' 'X64' (ConvertFrom-MixLabProcessorArch -Native 'AMD64' -Process 'x86')
Assert-Same 'a 32-bit windows' 'X86' (ConvertFrom-MixLabProcessorArch -Native '' -Process 'x86')
Assert-Same 'nothing set' '' (ConvertFrom-MixLabProcessorArch -Native '' -Process '')
if ($IsWindows -or $PSVersionTable.PSVersion.Major -lt 6) {
    Assert-Same 'this machine has an answer' $true ([bool](Get-MixLabMachineArch))
}

Assert-Same 'window' 'mixlab-windows-x86_64-setup.exe' (Get-MixLabArtifactName -Arch x86_64 -Flavour window -Version '')
Assert-Same 'headless arm' 'mixengine-windows-aarch64-headless-setup.exe' (Get-MixLabArtifactName -Arch aarch64 -Flavour headless -Version '')
Assert-Same 'window, versioned' 'mixlab-0.0.13-windows-x86_64-setup.exe' (Get-MixLabArtifactName -Arch x86_64 -Flavour window -Version '0.0.13')
Assert-Same 'headless, versioned' 'mixengine-0.0.13-windows-aarch64-headless-setup.exe' (Get-MixLabArtifactName -Arch aarch64 -Flavour headless -Version '0.0.13')

Assert-Same 'latest' 'https://github.com/mixnz/mixlab/releases/latest/download' (Get-MixLabReleaseBase -Version '')
Assert-Same 'a version' 'https://github.com/mixnz/mixlab/releases/download/v0.0.13' (Get-MixLabReleaseBase -Version '0.0.13')

# -PrintNames through @args, the way `& ([scriptblock]::Create(...)) -PrintNames` passes it.
function Invoke-Relay { Install-MixLab @args }
Assert-Same 'switches survive @args' 4 (@(Invoke-Relay -PrintNames -Version '0.0.13')).Count

# Off Windows only a dry run makes sense: a real run would try to start setup.exe.
if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
    $why = try { Install-MixLab -Headless | Out-Null; 'accepted' } catch { $_.Exception.Message }
    Assert-Same 'a real run off windows is refused before downloading' $true ($why -like '*is for windows*')
}

$work = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    # A minisign that refuses, writing to stderr: Windows PowerShell 5.1 turns that stderr into a
    # terminating NativeCommandError under -ErrorAction Stop, and PowerShell 7 does the same with
    # $PSNativeCommandUseErrorActionPreference. Either way the script's own message must come out.
    if ($IsWindows -or $PSVersionTable.PSVersion.Major -lt 6) {
        $fake = Join-Path $work 'fake-minisign.cmd'
        "@echo Signature verification failed 1>&2`r`n@exit /b 1" | Set-Content -Encoding ASCII $fake
    } else {
        $fake = Join-Path $work 'fake-minisign'
        "#!/bin/sh`necho 'Signature verification failed' >&2`nexit 1" | Set-Content $fake
        chmod +x $fake
    }
    $ErrorActionPreference = 'Stop'
    $PSNativeCommandUseErrorActionPreference = $true
    $why = try { Test-MixLabSignature -Minisign $fake -Key 'k' -File $fake -Name 'x' -Version '' | Out-Null; 'accepted' } catch { $_.Exception.Message }
    $PSNativeCommandUseErrorActionPreference = $false
    # The fake exits 1 on purpose, and the runner's PowerShell wrapper ends with
    # `exit $LASTEXITCODE`: left as it is, a passing test fails the step.
    $global:LASTEXITCODE = 0
    Assert-Same 'a refused signature says so in the script''s words' $true ($why -like 'the signature does not match*')

    'a package' | Set-Content -NoNewline (Join-Path $work 'x.exe')
    $hash = (Get-FileHash -Algorithm SHA256 (Join-Path $work 'x.exe')).Hash.ToLowerInvariant()
    "$hash  x.exe" | Set-Content (Join-Path $work 'x.exe.sha256')
    Assert-Same 'the checksum matches' $true (Test-MixLabSha256 -File (Join-Path $work 'x.exe') -SumFile (Join-Path $work 'x.exe.sha256'))
    'changed' | Add-Content (Join-Path $work 'x.exe')
    Assert-Same 'a changed file fails' $false (Test-MixLabSha256 -File (Join-Path $work 'x.exe') -SumFile (Join-Path $work 'x.exe.sha256'))
} finally {
    Remove-Item -Recurse -Force $work
}

if ($script:failures -ne 0) { throw "$($script:failures) check(s) failed" }
Write-Host 'install.ps1: every check passed'
