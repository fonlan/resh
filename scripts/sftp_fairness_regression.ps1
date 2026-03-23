param(
  [Parameter(Mandatory = $true)]
  [string]$ServerHost,

  [Parameter(Mandatory = $true)]
  [string]$User,

  [int]$Port = 22,
  [string]$PrivateKeyPath = '',
  [string]$OutputDir = 'artifacts/sftp-fairness',
  [string]$RemoteBaseDir = '/tmp',
  [int]$FileCount = 40,
  [int]$FileSizeKB = 64,
  [double]$FairnessThreshold = 0.7
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Ensure-Directory {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -Path $Path -ItemType Directory -Force | Out-Null
  }
}

function Join-RemotePath {
  param([string[]]$Parts)
  $segments = @()
  foreach ($part in $Parts) {
    if ([string]::IsNullOrWhiteSpace($part)) {
      continue
    }
    $normalized = $part.Replace('\', '/').Trim('/')
    if ($normalized.Length -gt 0) {
      $segments += $normalized
    }
  }
  if ($segments.Count -eq 0) {
    return '/'
  }
  return '/' + ($segments -join '/')
}

function New-ZeroFile {
  param(
    [string]$Path,
    [long]$SizeBytes
  )
  if (-not (Test-Path -LiteralPath $Path)) {
    $dir = Split-Path -Parent $Path
    Ensure-Directory -Path $dir
    $stream = [System.IO.File]::Create($Path)
    try {
      $stream.SetLength($SizeBytes)
    }
    finally {
      $stream.Dispose()
    }
  }
}

function Invoke-SftpBatch {
  param(
    [string]$CaseId,
    [string[]]$Commands,
    [string]$LogDir
  )
  $batchPath = Join-Path $LogDir "$CaseId.batch"
  $logPath = Join-Path $LogDir "$CaseId.log"
  [System.IO.File]::WriteAllLines($batchPath, $Commands, [System.Text.UTF8Encoding]::new($false))

  $args = @('-P', $Port.ToString(), '-b', $batchPath)
  if (-not [string]::IsNullOrWhiteSpace($PrivateKeyPath)) {
    $args += @('-i', $PrivateKeyPath)
  }
  $args += "$User@$ServerHost"

  $start = Get-Date
  $rawOutput = & sftp @args 2>&1
  $exitCode = $LASTEXITCODE
  $elapsed = (Get-Date) - $start
  $outputText = ($rawOutput | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
  [System.IO.File]::WriteAllText($logPath, $outputText + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))

  return [PSCustomObject]@{
    CaseId = $CaseId
    ExitCode = $exitCode
    ElapsedSeconds = [Math]::Round($elapsed.TotalSeconds, 3)
    BatchPath = $batchPath
    LogPath = $logPath
  }
}

$sftpCommand = Get-Command -Name sftp -ErrorAction SilentlyContinue
if (-not $sftpCommand) {
  throw 'sftp command not found in PATH. Install OpenSSH client first.'
}
if ($FileCount -lt 1) {
  throw 'FileCount must be >= 1.'
}
if ($FileSizeKB -lt 1) {
  throw 'FileSizeKB must be >= 1.'
}

$runId = Get-Date -Format 'yyyyMMdd-HHmmss'
$runRoot = Join-Path $OutputDir $runId
$logDir = Join-Path $runRoot 'logs'
$seedDir = Join-Path $runRoot 'seed-files'
Ensure-Directory -Path $runRoot
Ensure-Directory -Path $logDir
Ensure-Directory -Path $seedDir

$remoteRunDir = Join-RemotePath -Parts @($RemoteBaseDir, "resh-fairness-$runId")
$remoteSessionADir = Join-RemotePath -Parts @($remoteRunDir, 'session-a')
$remoteSessionBDir = Join-RemotePath -Parts @($remoteRunDir, 'session-b')
$fileSizeBytes = [long]$FileSizeKB * 1KB

Write-Host "[setup] creating local seed files (count=$FileCount, size=${FileSizeKB}KB)"
$seedFiles = New-Object System.Collections.Generic.List[string]
for ($i = 1; $i -le $FileCount; $i++) {
  $name = ('seed-{0:D4}.bin' -f $i)
  $path = Join-Path $seedDir $name
  New-ZeroFile -Path $path -SizeBytes $fileSizeBytes
  $seedFiles.Add($path) | Out-Null
}

Write-Host '[setup] preparing remote directories'
$setupResult = Invoke-SftpBatch -CaseId '00-setup' -LogDir $logDir -Commands @(
  ('mkdir "{0}"' -f $remoteRunDir),
  ('mkdir "{0}"' -f $remoteSessionADir),
  ('mkdir "{0}"' -f $remoteSessionBDir)
)
if ($setupResult.ExitCode -ne 0) {
  throw "Failed to prepare remote directories. See $($setupResult.LogPath)"
}

$commandsA = New-Object System.Collections.Generic.List[string]
$commandsB = New-Object System.Collections.Generic.List[string]
foreach ($localPath in $seedFiles) {
  $name = Split-Path -Leaf $localPath
  $remoteAPath = Join-RemotePath -Parts @($remoteSessionADir, $name)
  $remoteBPath = Join-RemotePath -Parts @($remoteSessionBDir, $name)
  $commandsA.Add(('put "{0}" "{1}"' -f $localPath, $remoteAPath)) | Out-Null
  $commandsB.Add(('put "{0}" "{1}"' -f $localPath, $remoteBPath)) | Out-Null
}

$jobScript = {
  param(
    [string]$CaseId,
    [string[]]$Commands,
    [string]$ServerHost,
    [string]$User,
    [int]$Port,
    [string]$PrivateKeyPath,
    [string]$LogDir
  )
  $ErrorActionPreference = 'Stop'
  $batchPath = Join-Path $LogDir "$CaseId.batch"
  $logPath = Join-Path $LogDir "$CaseId.log"
  [System.IO.File]::WriteAllLines($batchPath, $Commands, [System.Text.UTF8Encoding]::new($false))
  $args = @('-P', $Port.ToString(), '-b', $batchPath)
  if (-not [string]::IsNullOrWhiteSpace($PrivateKeyPath)) {
    $args += @('-i', $PrivateKeyPath)
  }
  $args += "$User@$ServerHost"
  $start = Get-Date
  $rawOutput = & sftp @args 2>&1
  $exitCode = $LASTEXITCODE
  $elapsed = (Get-Date) - $start
  $outputText = ($rawOutput | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
  [System.IO.File]::WriteAllText($logPath, $outputText + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
  [PSCustomObject]@{
    CaseId = $CaseId
    ExitCode = $exitCode
    ElapsedSeconds = [Math]::Round($elapsed.TotalSeconds, 3)
    BatchPath = $batchPath
    LogPath = $logPath
  }
}

Write-Host '[run] executing concurrent session-a/session-b uploads'
$overallStart = Get-Date
$jobA = Start-Job -ScriptBlock $jobScript -ArgumentList @(
  'session-a-upload',
  [string[]]$commandsA.ToArray(),
  $ServerHost,
  $User,
  $Port,
  $PrivateKeyPath,
  $logDir
)
$jobB = Start-Job -ScriptBlock $jobScript -ArgumentList @(
  'session-b-upload',
  [string[]]$commandsB.ToArray(),
  $ServerHost,
  $User,
  $Port,
  $PrivateKeyPath,
  $logDir
)

Wait-Job -Job @($jobA, $jobB) | Out-Null
$resultA = Receive-Job -Job $jobA
$resultB = Receive-Job -Job $jobB
Remove-Job -Job @($jobA, $jobB) -Force
$overallElapsed = (Get-Date) - $overallStart

$maxElapsed = [Math]::Max($resultA.ElapsedSeconds, $resultB.ElapsedSeconds)
$minElapsed = [Math]::Min($resultA.ElapsedSeconds, $resultB.ElapsedSeconds)
$fairnessRatio = if ($maxElapsed -gt 0) {
  [Math]::Round(($minElapsed / $maxElapsed), 3)
}
else {
  1.0
}
$fairnessStatus = if (
  $resultA.ExitCode -eq 0 -and
  $resultB.ExitCode -eq 0 -and
  $fairnessRatio -ge $FairnessThreshold
) {
  'pass'
}
else {
  'review'
}

$csvPath = Join-Path $runRoot 'fairness-results.csv'
$mdPath = Join-Path $runRoot 'fairness-results.md'
$rows = @(
  [PSCustomObject]@{
    run_id = $runId
    case_id = $resultA.CaseId
    exit_code = $resultA.ExitCode
    elapsed_seconds = $resultA.ElapsedSeconds
    bytes_total = $FileCount * $fileSizeBytes
    log_file = (Split-Path -Leaf $resultA.LogPath)
  },
  [PSCustomObject]@{
    run_id = $runId
    case_id = $resultB.CaseId
    exit_code = $resultB.ExitCode
    elapsed_seconds = $resultB.ElapsedSeconds
    bytes_total = $FileCount * $fileSizeBytes
    log_file = (Split-Path -Leaf $resultB.LogPath)
  }
)
$rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding utf8

$mdLines = @()
$mdLines += '# SFTP Fairness Regression Report'
$mdLines += ''
$mdLines += "run_id: $runId"
$mdLines += "generated_at: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')"
$mdLines += "file_count: $FileCount"
$mdLines += "file_size_kb: $FileSizeKB"
$mdLines += "overall_elapsed_seconds: $([Math]::Round($overallElapsed.TotalSeconds, 3))"
$mdLines += "fairness_ratio: $fairnessRatio"
$mdLines += "fairness_threshold: $FairnessThreshold"
$mdLines += "fairness_status: $fairnessStatus"
$mdLines += ''
$mdLines += '| Case | Exit | Elapsed(s) | TotalBytes | Log |'
$mdLines += '| --- | ---: | ---: | ---: | --- |'
$mdLines += "| $($resultA.CaseId) | $($resultA.ExitCode) | $($resultA.ElapsedSeconds) | $($FileCount * $fileSizeBytes) | logs/$([System.IO.Path]::GetFileName($resultA.LogPath)) |"
$mdLines += "| $($resultB.CaseId) | $($resultB.ExitCode) | $($resultB.ElapsedSeconds) | $($FileCount * $fileSizeBytes) | logs/$([System.IO.Path]::GetFileName($resultB.LogPath)) |"
$mdLines += ''
$mdLines += 'Interpretation:'
$mdLines += '- fairness_ratio close to 1.0 means balanced completion time between sessions.'
$mdLines += '- fairness_status=review means check logs or queue quota settings.'
[System.IO.File]::WriteAllLines($mdPath, $mdLines, [System.Text.UTF8Encoding]::new($false))

Write-Host ''
Write-Host '[done] fairness regression scaffold finished'
Write-Host "results_csv: $csvPath"
Write-Host "results_md : $mdPath"
Write-Host "fairness_ratio: $fairnessRatio (threshold=$FairnessThreshold, status=$fairnessStatus)"
