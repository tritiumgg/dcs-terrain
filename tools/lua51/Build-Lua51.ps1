#Requires -Version 7.0

<#
.SYNOPSIS
    Build the Lua 5.1.5 interpreter for Windows with MSVC.

.DESCRIPTION
    Downloads the official Lua 5.1.5 source release from lua.org, verifies its
    published SHA-256, compiles it with the MSVC toolset, and writes lua.exe
    into an output directory.

    The offline extractor tests need Lua 5.1 specifically, because that is the
    version the DCS hook state runs. mise provisions it on macOS and Linux from
    source; its plugin builds with make, which Windows does not have, so this
    script covers the Windows case.

    Requires Visual Studio Build Tools with the C++ workload, found through
    vswhere, and tar.exe, which ships with Windows 10 and later.

.PARAMETER OutputDirectory
    Directory to write lua.exe into. Defaults to .tools\bin at the repository
    root, which is the directory mise puts on PATH.

.PARAMETER Force
    Rebuild and overwrite an existing lua.exe. Without it an existing binary of
    the right version is left alone.

.PARAMETER Quiet
    Suppress narration. Warnings and errors are still shown.

.EXAMPLE
    pwsh.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\lua51\Build-Lua51.ps1

    Build into .tools\bin at the repository root.

.EXAMPLE
    mise run lua51

    The same build, through the task that wraps it.

.EXAMPLE
    pwsh.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\lua51\Build-Lua51.ps1 -Force -Verbose

    Rebuild over an existing binary, with timestamps and compiler detail.

.NOTES
    Execution policy
    ----------------
    Windows blocks unsigned scripts by default. To run this script without
    changing any persistent setting, use a per-process bypass:

        pwsh.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\lua51\Build-Lua51.ps1

    To bypass for the current session only:

        Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

    To allow local scripts for the current user, which needs no administrator
    rights:

        Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned

    To allow local scripts for every user on the machine, from an elevated
    session:

        Set-ExecutionPolicy -Scope LocalMachine -ExecutionPolicy RemoteSigned

    Run Get-ExecutionPolicy -List to see what is set at every scope. Group
    Policy outranks all of the above and can only be changed by an
    administrator.

    If this file arrived by download, email or network share, also clear the
    mark of the web, which blocks execution independently of the policy:

        Unblock-File -Path .\tools\lua51\Build-Lua51.ps1

    Execution policy guards against accidental execution; it is not a security
    boundary. It applies on Windows only.
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter()]
    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory,

    [Parameter()]
    [switch]$Force,

    [Parameter()]
    [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# -WhatIf:$false on each: Set-Variable honours ShouldProcess, and a constant
# skipped under -WhatIf leaves every later reference undefined.
Set-Variable -Name LuaVersion -Option Constant -Value '5.1.5' -WhatIf:$false
# Published beside the tarball at https://www.lua.org/ftp/
Set-Variable -Name LuaSha256 -Option Constant -WhatIf:$false `
    -Value '2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333'
Set-Variable -Name LuaUrl -Option Constant -WhatIf:$false `
    -Value 'https://www.lua.org/ftp/lua-5.1.5.tar.gz'

# luac.c and print.c belong to the compiler, not the interpreter, and luac.c
# carries a second main().
Set-Variable -Name NonInterpreterObjects -Option Constant -WhatIf:$false `
    -Value @('luac.obj', 'print.obj')

$script:LogLevel = 1
$script:TempDir = $null

function Get-LogPrefix {
    if ($script:LogLevel -ge 2) { return "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] " }
    return ''
}

function Write-LogError {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Message)
    # -ErrorAction Continue so logging does not terminate under
    # $ErrorActionPreference = 'Stop'.
    Write-Error -Message "$(Get-LogPrefix)$Message" -ErrorAction Continue
}

function Write-LogWarning {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Message)
    Write-Warning "$(Get-LogPrefix)$Message"
}

function Write-LogInfo {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Message)
    if ($script:LogLevel -lt 1) { return }
    if ($script:LogLevel -ge 2) {
        Write-Host "$(Get-LogPrefix)INFO: $Message"
    } else {
        Write-Host $Message
    }
}

function Write-LogDebug {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Message)
    Write-Verbose "$(Get-LogPrefix)$Message"
}

function Get-RepositoryRoot {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    # tools\lua51\Build-Lua51.ps1, so the root is two levels up.
    return (Get-Item -LiteralPath (Join-Path $PSScriptRoot '..' '..')).FullName
}

function Get-VcVarsPath {
    [CmdletBinding()]
    [OutputType([string])]
    param()

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio' 'Installer' 'vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        throw 'vswhere.exe not found: install Visual Studio Build Tools with the C++ workload'
    }

    # -products *: match Build Tools as well as the full editions.
    # -requires ...VC.Tools.x86.x64: the workload that supplies cl.exe.
    # -property installationPath: print the install root and nothing else.
    $installPath = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installPath)) {
        throw 'No Visual Studio install with the C++ workload was found'
    }

    $vcvars = Join-Path $installPath.Trim() 'VC' 'Auxiliary' 'Build' 'vcvars64.bat'
    if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
        throw "vcvars64.bat not found under $installPath"
    }

    return $vcvars
}

function Get-LuaSource {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$WorkingDirectory
    )

    $archive = Join-Path $WorkingDirectory "lua-$LuaVersion.tar.gz"

    Write-LogInfo "Downloading $LuaUrl"
    Invoke-WebRequest -Uri $LuaUrl -OutFile $archive -MaximumRetryCount 3 -RetryIntervalSec 2

    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $LuaSha256) {
        throw "Checksum mismatch for lua-${LuaVersion}.tar.gz: expected $LuaSha256, got $actual"
    }
    Write-LogDebug "SHA-256 verified: $actual"

    # -x extract, -z gzip, -f archive, -C into. Windows tar.exe is bsdtar.
    Invoke-NativeCommand -FilePath 'tar.exe' -ArgumentList @('-xzf', $archive, '-C', $WorkingDirectory)

    $sourceDir = Join-Path $WorkingDirectory "lua-$LuaVersion" 'src'
    if (-not (Test-Path -LiteralPath $sourceDir -PathType Container)) {
        throw "Extracted archive has no src directory at $sourceDir"
    }

    return $sourceDir
}

function Invoke-NativeCommand {
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter()][string[]]$ArgumentList = @()
    )

    $display = "$FilePath $($ArgumentList -join ' ')"
    if (-not $PSCmdlet.ShouldProcess($display, 'Execute')) { return }

    Write-LogDebug "Executing: $display"
    # Captured rather than emitted: a tool's chatter on the success stream
    # becomes the caller's return value. cl.exe echoes every source file name,
    # which is how that goes wrong quietly.
    $output = & $FilePath @ArgumentList 2>&1

    foreach ($line in $output) { Write-LogDebug $line }

    if ($LASTEXITCODE -ne 0) {
        foreach ($line in $output) { Write-LogWarning $line }
        throw "Command failed with exit code ${LASTEXITCODE}: $display"
    }
}

function Invoke-MsvcBuild {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$SourceDirectory,
        [Parameter(Mandatory)][string]$VcVarsPath
    )

    # cl.exe needs the environment vcvars64.bat sets, and that environment does
    # not survive back into PowerShell. A batch file keeps the call and the
    # compile in one cmd.exe session, and keeps the quoting in one place.
    #
    # /nologo: no banner. /O2: optimise for speed. /MD: dynamic CRT, so the
    # binary needs only the redistributable. /W3: default warning level.
    # /D_CRT_SECURE_NO_WARNINGS: Lua 5.1 predates the _s string functions.
    # /c: compile only, link separately below.
    # vcvars64.bat calls vswhere.exe by bare name on one of its paths, so the
    # Installer directory goes on PATH first or the call prints a not-found
    # line and carries on.
    $batch = @"
@echo off
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer;%PATH%"
call "$VcVarsPath" >nul || exit /b 1
cd /d "$SourceDirectory" || exit /b 1
cl /nologo /O2 /MD /W3 /D_CRT_SECURE_NO_WARNINGS /c *.c || exit /b 1
del $($NonInterpreterObjects -join ' ') || exit /b 1
link /nologo /out:lua.exe *.obj || exit /b 1
"@

    $batchPath = Join-Path $script:TempDir 'build-lua.bat'
    # WriteAllText rather than Set-Content so no byte order mark reaches
    # cmd.exe, which would read it as part of the first command.
    [System.IO.File]::WriteAllText($batchPath, $batch, [System.Text.UTF8Encoding]::new($false))

    Invoke-NativeCommand -FilePath 'cmd.exe' -ArgumentList @('/c', $batchPath)

    $binary = Join-Path $SourceDirectory 'lua.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Build reported success but produced no lua.exe in $SourceDirectory"
    }

    return $binary
}

function Test-ExistingBuild {
    [CmdletBinding()]
    [OutputType([bool])]
    param(
        [Parameter(Mandatory)][string]$BinaryPath
    )

    if (-not (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) { return $false }

    # The version banner is the whole test. $LASTEXITCODE is not consulted
    # because it is unset until the session's first native command, which
    # strict mode turns into an error rather than a zero.
    $reported = & $BinaryPath -v 2>&1 | Select-Object -First 1

    # "Lua 5.1.5  Copyright (C) 1994-2012 Lua.org, PUC-Rio"
    return ("$reported" -match [regex]::Escape("Lua $LuaVersion"))
}

try {
    if ($Quiet) { $script:LogLevel = 0 }
    if ($VerbosePreference -ne 'SilentlyContinue') { $script:LogLevel = 2 }

    if (-not $IsWindows) {
        Write-LogError 'This script builds the Windows interpreter; use mise on macOS and Linux'
        exit 2
    }

    if (-not $PSBoundParameters.ContainsKey('OutputDirectory')) {
        $OutputDirectory = Join-Path (Get-RepositoryRoot) '.tools' 'bin'
    }
    $OutputDirectory =
        $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)

    $target = Join-Path $OutputDirectory 'lua.exe'

    if (-not $Force -and (Test-ExistingBuild -BinaryPath $target)) {
        Write-LogInfo "Lua $LuaVersion already built at $target"
        exit 0
    }

    if ($WhatIfPreference) {
        # Nothing below previews: the build downloads a tarball and runs a
        # compiler, and neither has a dry form.
        Write-LogInfo "What if: build Lua $LuaVersion from $LuaUrl and write $target"
        exit 0
    }

    foreach ($tool in @('tar.exe', 'cmd.exe')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
            Write-LogError "$tool not found on PATH"
            exit 2
        }
    }

    $vcvars = Get-VcVarsPath
    Write-LogDebug "Using $vcvars"

    $script:TempDir = Join-Path ([System.IO.Path]::GetTempPath()) "lua51-$([guid]::NewGuid())"
    $null = New-Item -ItemType Directory -Path $script:TempDir

    $sourceDir = Get-LuaSource -WorkingDirectory $script:TempDir

    Write-LogInfo "Compiling Lua $LuaVersion"
    $built = Invoke-MsvcBuild -SourceDirectory $sourceDir -VcVarsPath $vcvars

    if ($PSCmdlet.ShouldProcess($target, 'Install lua.exe')) {
        $null = New-Item -ItemType Directory -Path $OutputDirectory -Force
        Copy-Item -LiteralPath $built -Destination $target -Force
        Write-LogInfo "Wrote $target"
    }
}
catch {
    Write-LogError $_.Exception.Message
    Write-LogDebug $_.ScriptStackTrace
    exit 1
}
finally {
    if ($script:TempDir -and (Test-Path -LiteralPath $script:TempDir)) {
        Remove-Item -LiteralPath $script:TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
