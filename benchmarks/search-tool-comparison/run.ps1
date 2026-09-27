param([int]$Samples = 21, [switch]$ValidateOnly, [switch]$ProfileMemory, [switch]$Tuning)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\process.ps1"
Add-Type -Path "$PSScriptRoot\ResultSignature.cs"
$resultsRoot = Join-Path $PSScriptRoot 'results'
New-Item -ItemType Directory -Force -Path $resultsRoot | Out-Null
$searchExecutable = Join-Path $PSScriptRoot 'search-target\release\codesearch.exe'
$prototypeExecutable = [IO.Path]::GetFullPath("$PSScriptRoot\..\search-alternatives\target\release\tool-search.exe")
. "$PSScriptRoot\tools.ps1"
$tools = Get-ComparisonTools
$queries = if ($ProfileMemory) { @('zzqxyw_no_such_content', 'return') } else { @('needle_tag_7fd9', 'zzqxyw_no_such_content', 'abcd', 'return', '::', 'café', 'normalize_search_text') }
$resultPrefix = if ($ProfileMemory) { 'memory-' } else { '' }
if ($Tuning) {
    $queries = @('needle_tag_7fd9', 'zzqxyw_no_such_content', 'abcd', 'return')
    $resultPrefix = 'tuning-'
}
$measurements = [Collections.Generic.List[object]]::new()
$validation = [Collections.Generic.List[object]]::new()
$preparation = [Collections.Generic.List[object]]::new()
foreach ($corpus in @('source','many','single')) {
    $root = Join-Path $PSScriptRoot "fixtures\$corpus"
    $activeTools = $tools
    if ($Tuning) { $activeTools = Get-TuningTools $tools $root }
    if ($ProfileMemory) {
        $nativeGit = (Get-TuningTools $tools $root)[-1]
        $activeTools = @($tools[0..3]) + @($nativeGit, $tools[-1])
    }
    foreach ($query in $queries) {
        $expected = [ResultSignature]::Oracle((Join-Path $root 'data'), $query)
        foreach ($tool in $activeTools) {
            $directory = $root
            $first = Invoke-SearchProcess $tool $directory $query
            $actual = Get-ResultSignature $first.Output $tool.Name
            $correct = $actual.Hash -eq $expected.Hash
            $validation.Add([pscustomobject]@{ Corpus=$corpus; Query=$query; Tool=$tool.Name; Correct=$correct; Count=$actual.Count; ExpectedCount=$expected.Count; Hash=$actual.Hash; ExpectedHash=$expected.Hash })
            $preparation.Add([pscustomobject]@{ Corpus=$corpus; Query=$query; Tool=$tool.Name; WallMs=$first.WallMs; CpuMs=$first.CpuMs; Stderr=$first.Errors.Trim() })
        }
        $validation | Export-Csv "$resultsRoot\${resultPrefix}validation.csv" -NoTypeInformation
        $preparation | Export-Csv "$resultsRoot\${resultPrefix}preparation.csv" -NoTypeInformation
        if ($validation | Where-Object { -not $_.Correct }) { throw 'Result validation failed; see validation.csv.' }
        Write-Output "Validated $corpus / $query"
        if ($ValidateOnly) { continue }
        foreach ($sample in 1..$Samples) {
            for ($offset = 0; $offset -lt $activeTools.Count; $offset++) {
                $tool = $activeTools[($sample + $offset) % $activeTools.Count]
                $directory = $root
                $run = Invoke-SearchProcess $tool $directory $query -ProfileMemory:$ProfileMemory
                $actual = Get-ResultSignature $run.Output $tool.Name
                if ($actual.Hash -ne $expected.Hash) { throw "Incorrect measured results: $corpus / $query / $($tool.Name)" }
                $measurements.Add([pscustomobject]@{ Corpus=$corpus; Query=$query; Tool=$tool.Name; Sample=$sample; WallMs=$run.WallMs; CpuMs=$run.CpuMs; ObservedPeakWorkingSetBytes=$run.ObservedPeakWorkingSetBytes; Matches=$actual.Count; OutputBytes=[Text.Encoding]::UTF8.GetByteCount($run.Output) })
            }
        }
        $measurements | Export-Csv "$resultsRoot\${resultPrefix}samples.csv" -NoTypeInformation
    }
}
$validation | Export-Csv "$resultsRoot\${resultPrefix}validation.csv" -NoTypeInformation
$preparation | Export-Csv "$resultsRoot\${resultPrefix}preparation.csv" -NoTypeInformation
if ($ValidateOnly) { return }
$summary = foreach ($group in ($measurements | Group-Object Corpus,Query,Tool)) {
    $rows = $group.Group
    [pscustomobject]@{
        Corpus=$rows[0].Corpus; Query=$rows[0].Query; Tool=$rows[0].Tool; Samples=$rows.Count; Matches=$rows[0].Matches
        MedianMs=Get-Percentile $rows.WallMs 0.5; P95Ms=Get-Percentile $rows.WallMs 0.95
        MedianCpuMs=Get-Percentile $rows.CpuMs 0.5; OutputBytes=$rows[0].OutputBytes
        MedianObservedPeakWorkingSetBytes=Get-Percentile $rows.ObservedPeakWorkingSetBytes 0.5
    }
}
$summary | Export-Csv "$resultsRoot\${resultPrefix}summary.csv" -NoTypeInformation


