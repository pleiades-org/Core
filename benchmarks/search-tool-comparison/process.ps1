function Invoke-SearchProcess {
    param($Tool, [string]$Directory, [string]$Query, [switch]$ProfileMemory)
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new($Tool.Executable)
    $startInfo.WorkingDirectory = $Directory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false, $true)
    $startInfo.StandardErrorEncoding = [System.Text.UTF8Encoding]::new($false, $true)
    $startInfo.Environment['LC_ALL'] = 'C'
    $startInfo.Environment.Remove('RIPGREP_CONFIG_PATH') | Out-Null
    foreach ($argument in (& $Tool.Arguments $Directory $Query)) { $startInfo.ArgumentList.Add($argument) }
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $process = [System.Diagnostics.Process]::Start($startInfo)
    try {
        $processHandle = $process.Handle # Retain the process handle for post-exit CPU accounting.
        $outputTask = $process.StandardOutput.ReadToEndAsync()
        $errorTask = $process.StandardError.ReadToEndAsync()
        $observedPeak = 0L
        if ($ProfileMemory) {
            while (-not $process.WaitForExit(1)) {
                $process.Refresh()
                try { $observedPeak = [Math]::Max($observedPeak, $process.PeakWorkingSet64) }
                catch [System.InvalidOperationException] { break }
            }
        }
        $process.WaitForExit()
        $output = $outputTask.GetAwaiter().GetResult()
        $errors = $errorTask.GetAwaiter().GetResult()
        $timer.Stop()
        if ($process.ExitCode -notin $Tool.ExitCodes) { throw "$($Tool.Name) exited $($process.ExitCode): $errors" }
        [pscustomobject]@{
            WallMs = $timer.Elapsed.TotalMilliseconds
            CpuMs = $process.TotalProcessorTime.TotalMilliseconds
            ObservedPeakWorkingSetBytes = $observedPeak
            Output = $output; Errors = $errors; ExitCode = $process.ExitCode
        }
    } finally { $process.Dispose() }
}

function Get-ResultSignature {
    param([string]$Output, [string]$ToolName)
    [ResultSignature]::Parse($Output, $ToolName -eq 'search-original')
}

function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    $ordered = @($Values | Sort-Object)
    $ordered[[Math]::Max(0, [Math]::Ceiling($ordered.Count * $Percentile) - 1)]
}
