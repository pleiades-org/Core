function Get-ComparisonTools {
    $searchExecutable = Join-Path $PSScriptRoot 'search-target\release\codesearch.exe'
    $prototypeExecutable = [IO.Path]::GetFullPath("$PSScriptRoot\..\search-alternatives\target\release\tool-search.exe")
$tools = @(
    @{ Name = 'search-original'; Executable = $searchExecutable; ExitCodes = @(0); Arguments = { param($directory, $query) $root = 'data'; @($query, '-d', $root, '-c', '-l', '100000000') } },
    @{ Name = 'prototype-scan'; Executable = $prototypeExecutable; ExitCodes = @(0); Arguments = { param($directory, $query) $root = 'data'; @('scan', $root, $query) } },
    @{ Name = 'rg-auto'; Executable = (Get-Command rg).Source; ExitCodes = @(0,1); Arguments = { param($directory, $query) $root = 'data'; @('--no-config','--no-ignore','--hidden','--encoding','none','--color','never','--no-heading','-n','-H','-F','-a','-e',$query,'--',$root) } },
    @{ Name = 'rg-one'; Executable = (Get-Command rg).Source; ExitCodes = @(0,1); Arguments = { param($directory, $query) $root = 'data'; @('--no-config','--no-ignore','--hidden','--encoding','none','--color','never','--no-heading','-n','-H','-F','-a','-j','1','-e',$query,'--',$root) } },
    @{ Name = 'git-grep'; Executable = (Get-Command git).Source; ExitCodes = @(0,1); Arguments = { param($directory, $query) $root = 'data'; @('-c','color.ui=false','grep','--no-index','--no-exclude-standard','--threads','0','-n','-H','-F','-a','--no-column','--no-heading','--no-break','-e',$query,'--',$root) } },
    @{ Name = 'gnu-grep'; Executable = 'C:\Program Files\Git\usr\bin\grep.exe'; ExitCodes = @(0,1); Arguments = { param($directory, $query) $root = 'data'; @('-r','-n','-H','-F','-a','--color=never','-e',$query,'--',$root) } }
)

    return $tools
}

function Get-TuningTools {
    param($BaselineTools, [string]$Directory)
    $catalogPaths = @(Get-ChildItem -LiteralPath (Join-Path $Directory 'data') -File | Sort-Object Name | ForEach-Object { 'data/' + $_.Name })
    $catalog = @{ Name = 'rg-catalog'; Executable = (Get-Command rg).Source; ExitCodes = @(0,1); Arguments = {
        param($directory, $query)
        @('--no-config','--no-ignore','--hidden','--encoding','none','--color','never','--no-heading','-n','-H','-F','-a','-e',$query,'--') + $catalogPaths
    }.GetNewClosure() }
    $mapped = @{ Name = 'rg-mmap'; Executable = (Get-Command rg).Source; ExitCodes = @(0,1); Arguments = {
        param($directory, $query)
        @('--no-config','--no-ignore','--hidden','--mmap','--encoding','none','--color','never','--no-heading','-n','-H','-F','-a','-e',$query,'--','data')
    } }
    $nativeGit = $BaselineTools[4].Clone()
    $nativeGit.Name = 'git-grep-native'
    $nativeGit.Executable = 'C:\Program Files\Git\mingw64\bin\git.exe'
    @($BaselineTools[0], $BaselineTools[2], $catalog, $mapped, $nativeGit)
}

