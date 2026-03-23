param(
  [Parameter(Mandatory = $true)]
  [string]$ServerHost,

  [Parameter(Mandatory = $true)]
  [string]$User,

  [int]$Port = 22,
  [string]$PrivateKeyPath = '',
  [string]$RemoteBaseDir = '/tmp',
  [string]$OutputDir = 'artifacts/sftp-perf-suite',
  [int]$SmallFileKB = 128,
  [int]$LargeFileMB = 512,
  [string]$ApplyLowRttCommand = '',
  [string]$ApplyHighRttCommand = '',
  [string]$ClearRttCommand = '',
  [switch]$SkipUpload,
  [switch]$SkipDownload,
  [int]$FairnessFileCount = 40,
  [int]$FairnessFileSizeKB = 64,
  [double]$FairnessThreshold = 0.7,
  [switch]$SkipBaseline,
  [switch]$SkipFairness
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Ensure-Directory {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -Path $Path -ItemType Directory -Force | Out-Null
  }
}

function Get-LatestRunDirectory {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    return $null
  }
  return Get-ChildItem -LiteralPath $Path -Directory |
    Sort-Object -Property Name -Descending |
    Select-Object -First 1
}

function Parse-Number {
  param([object]$Value)
  $text = [string]$Value
  if ([string]::IsNullOrWhiteSpace($text)) {
    return 0.0
  }
  return [double]::Parse($text, [System.Globalization.CultureInfo]::InvariantCulture)
}

if ($SkipBaseline -and $SkipFairness) {
  throw 'Both baseline and fairness are skipped. Keep at least one suite enabled.'
}

$baselineScript = Join-Path $PSScriptRoot 'sftp_baseline_matrix.ps1'
$fairnessScript = Join-Path $PSScriptRoot 'sftp_fairness_regression.ps1'

if (-not (Test-Path -LiteralPath $baselineScript)) {
  throw "Missing baseline script: $baselineScript"
}
if (-not (Test-Path -LiteralPath $fairnessScript)) {
  throw "Missing fairness script: $fairnessScript"
}

$runId = Get-Date -Format 'yyyyMMdd-HHmmss'
$runRoot = Join-Path $OutputDir $runId
Ensure-Directory -Path $runRoot

$baselineResult = $null
$fairnessResult = $null

if (-not $SkipBaseline) {
  $baselineOutputDir = Join-Path $runRoot 'baseline'
  Ensure-Directory -Path $baselineOutputDir

  Write-Host '[run] baseline matrix suite'
  $baselineParams = @{
    ServerHost = $ServerHost
    User = $User
    Port = $Port
    PrivateKeyPath = $PrivateKeyPath
    RemoteBaseDir = $RemoteBaseDir
    OutputDir = $baselineOutputDir
    SmallFileKB = $SmallFileKB
    LargeFileMB = $LargeFileMB
    ApplyLowRttCommand = $ApplyLowRttCommand
    ApplyHighRttCommand = $ApplyHighRttCommand
    ClearRttCommand = $ClearRttCommand
  }
  if ($SkipUpload) {
    $baselineParams.SkipUpload = $true
  }
  if ($SkipDownload) {
    $baselineParams.SkipDownload = $true
  }

  & $baselineScript @baselineParams

  $baselineRunDir = Get-LatestRunDirectory -Path $baselineOutputDir
  if (-not $baselineRunDir) {
    throw "Baseline run directory not found in $baselineOutputDir"
  }

  $baselineCsvPath = Join-Path $baselineRunDir.FullName 'results.csv'
  if (-not (Test-Path -LiteralPath $baselineCsvPath)) {
    throw "Baseline results.csv not found: $baselineCsvPath"
  }

  $baselineRows = Import-Csv -LiteralPath $baselineCsvPath
  if ($baselineRows.Count -eq 0) {
    throw "Baseline results.csv is empty: $baselineCsvPath"
  }

  $baselineFailed = ($baselineRows | Where-Object { $_.Status -ne 'ok' }).Count
  $baselineOverallAvg = [Math]::Round(
    (($baselineRows | Measure-Object -Property ThroughputMBps -Average).Average),
    3
  )

  $directionRows = New-Object System.Collections.Generic.List[object]
  foreach ($group in ($baselineRows | Group-Object Direction | Sort-Object -Property Name)) {
    $avgThroughput = [Math]::Round(
      (($group.Group | Measure-Object -Property ThroughputMBps -Average).Average),
      3
    )
    $failed = ($group.Group | Where-Object { $_.Status -ne 'ok' }).Count
    $directionRows.Add([PSCustomObject]@{
      Direction = $group.Name
      Cases = $group.Count
      Failed = $failed
      AvgThroughputMBps = $avgThroughput
    }) | Out-Null
  }

  $baselineResult = [PSCustomObject]@{
    OutputDir = $baselineOutputDir
    RunDir = $baselineRunDir.FullName
    CsvPath = $baselineCsvPath
    Cases = $baselineRows.Count
    Failed = $baselineFailed
    OverallAvgThroughputMBps = $baselineOverallAvg
    DirectionRows = [object[]]$directionRows
  }
}

if (-not $SkipFairness) {
  $fairnessOutputDir = Join-Path $runRoot 'fairness'
  Ensure-Directory -Path $fairnessOutputDir

  Write-Host '[run] fairness regression suite'
  $fairnessParams = @{
    ServerHost = $ServerHost
    User = $User
    Port = $Port
    PrivateKeyPath = $PrivateKeyPath
    OutputDir = $fairnessOutputDir
    RemoteBaseDir = $RemoteBaseDir
    FileCount = $FairnessFileCount
    FileSizeKB = $FairnessFileSizeKB
    FairnessThreshold = $FairnessThreshold
  }

  & $fairnessScript @fairnessParams

  $fairnessRunDir = Get-LatestRunDirectory -Path $fairnessOutputDir
  if (-not $fairnessRunDir) {
    throw "Fairness run directory not found in $fairnessOutputDir"
  }

  $fairnessCsvPath = Join-Path $fairnessRunDir.FullName 'fairness-results.csv'
  if (-not (Test-Path -LiteralPath $fairnessCsvPath)) {
    throw "Fairness results csv not found: $fairnessCsvPath"
  }

  $fairnessRows = Import-Csv -LiteralPath $fairnessCsvPath
  if ($fairnessRows.Count -eq 0) {
    throw "Fairness csv is empty: $fairnessCsvPath"
  }

  $elapsedValues = @($fairnessRows | ForEach-Object { Parse-Number -Value $_.elapsed_seconds })
  $maxElapsed = ($elapsedValues | Measure-Object -Maximum).Maximum
  $minElapsed = ($elapsedValues | Measure-Object -Minimum).Minimum
  $ratio = if ($maxElapsed -gt 0) {
    [Math]::Round(($minElapsed / $maxElapsed), 3)
  }
  else {
    1.0
  }
  $failed = ($fairnessRows | Where-Object { Parse-Number -Value $_.exit_code -ne 0 }).Count
  $status = if ($failed -eq 0 -and $ratio -ge $FairnessThreshold) {
    'pass'
  }
  else {
    'review'
  }

  $fairnessResult = [PSCustomObject]@{
    OutputDir = $fairnessOutputDir
    RunDir = $fairnessRunDir.FullName
    CsvPath = $fairnessCsvPath
    Cases = $fairnessRows.Count
    Failed = $failed
    FairnessRatio = $ratio
    FairnessThreshold = $FairnessThreshold
    FairnessStatus = $status
  }
}

$summaryPath = Join-Path $runRoot 'suite-summary.md'
$summary = @()
$summary += '# SFTP Performance Suite Summary'
$summary += ''
$summary += "run_id: $runId"
$summary += "generated_at: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')"
$summary += "baseline_enabled: $(-not $SkipBaseline)"
$summary += "fairness_enabled: $(-not $SkipFairness)"
$summary += ''

if ($baselineResult) {
  $summary += '## Baseline Matrix'
  $summary += "- run_dir: $($baselineResult.RunDir)"
  $summary += "- csv: $($baselineResult.CsvPath)"
  $summary += "- cases: $($baselineResult.Cases)"
  $summary += "- failed: $($baselineResult.Failed)"
  $summary += "- avg_throughput_mbps: $($baselineResult.OverallAvgThroughputMBps)"
  $summary += ''
  $summary += '| Direction | Cases | Failed | Avg Throughput (MB/s) |'
  $summary += '| --- | ---: | ---: | ---: |'
  foreach ($row in $baselineResult.DirectionRows) {
    $summary += "| $($row.Direction) | $($row.Cases) | $($row.Failed) | $($row.AvgThroughputMBps) |"
  }
  $summary += ''
}

if ($fairnessResult) {
  $summary += '## Fairness Regression'
  $summary += "- run_dir: $($fairnessResult.RunDir)"
  $summary += "- csv: $($fairnessResult.CsvPath)"
  $summary += "- cases: $($fairnessResult.Cases)"
  $summary += "- failed: $($fairnessResult.Failed)"
  $summary += "- fairness_ratio: $($fairnessResult.FairnessRatio)"
  $summary += "- fairness_threshold: $($fairnessResult.FairnessThreshold)"
  $summary += "- fairness_status: $($fairnessResult.FairnessStatus)"
  $summary += ''
}

$summary += 'Interpretation:'
$summary += '- baseline failed > 0 means inspect per-case logs before comparing throughput.'
$summary += '- fairness_status=review means queue quota or transport conditions need investigation.'

[System.IO.File]::WriteAllLines($summaryPath, $summary, [System.Text.UTF8Encoding]::new($false))

Write-Host ''
Write-Host '[done] performance suite completed'
Write-Host "run_root    : $runRoot"
Write-Host "summary_md  : $summaryPath"
if ($baselineResult) {
  Write-Host "baseline_csv: $($baselineResult.CsvPath)"
}
if ($fairnessResult) {
  Write-Host "fairness_csv: $($fairnessResult.CsvPath)"
}
