$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\process.ps1"
. "$PSScriptRoot\tools.ps1"
Add-Type -Path "$PSScriptRoot\ResultSignature.cs"
$root = Join-Path $PSScriptRoot 'fixtures\edge'
New-Item -ItemType Directory -Force (Join-Path $root 'data') | Out-Null
[IO.File]::WriteAllText((Join-Path $root 'data\edge.txt'), "CamelCase`r`ncamelcase`r`na.b a.b`r`naxb`r`ncafé 東京`r`nlast_literal", [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $root 'data\empty.txt'), '', [Text.UTF8Encoding]::new($false))
$tools = Get-ComparisonTools
$checks = foreach ($query in @('CamelCase','camelcase','a.b','café','東京','last_literal','missing_literal')) {
    $expected = [ResultSignature]::Oracle((Join-Path $root 'data'), $query)
    foreach ($tool in $tools) {
        $actual = Get-ResultSignature (Invoke-SearchProcess $tool $root $query).Output $tool.Name
        $correct = $actual.Hash -eq $expected.Hash
        $knownCaseBug = $tool.Name -eq 'search-original' -and $query -eq 'CamelCase'
        if (-not $correct -and -not $knownCaseBug) { throw "Unexpected correctness failure: $($tool.Name) / $query" }
        [pscustomobject]@{ Tool=$tool.Name; Query=$query; Correct=$correct; Count=$actual.Count; ExpectedCount=$expected.Count; Hash=$actual.Hash; ExpectedHash=$expected.Hash; KnownCaseBug=$knownCaseBug }
    }
}
$checks | Export-Csv "$PSScriptRoot\results\edge-validation.csv" -NoTypeInformation
# Independently confirm every main benchmark result against a simple line-by-line oracle.
$validation = Import-Csv "$PSScriptRoot\results\validation.csv"
foreach ($group in ($validation | Group-Object Corpus,Query)) {
    $case = $group.Group[0]
    $expected = [ResultSignature]::Oracle("$PSScriptRoot\fixtures\$($case.Corpus)\data", $case.Query)
    foreach ($row in $group.Group) {
        if ($row.Hash -ne $expected.Hash -or [int]$row.Count -ne $expected.Count) { throw "Main result mismatch: $($group.Name) / $($row.Tool)" }
    }
}
Write-Output "Independent oracle agrees with all $($validation.Count) main cases. Edge probes recorded."
