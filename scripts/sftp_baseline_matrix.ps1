param(
  [Parameter(Mandatory = $true)]
  [string]$Host,

  [Parameter(Mandatory = $true)]
  [string]$User,

  [string]$RemoteBaseDir = '/tmp',
  [int]$Port = 22,
  [string]$PrivateKeyPath = '',
  [string]$OutputDir = 'artifacts/sftp-baseline',
  [int]$SmallFileKB = 128,
  [int]$LargeFileMB = 512,
  [string]$ApplyLowRttCommand = '',
  [string]$ApplyHighRttCommand = '',
  [string]$ClearRttCommand = '',
  [switch]$SkipUpload,
  [switch]$SkipDownload
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

    $normalized = $part.Replace('\', '/')
    $normalized = $normalized.Trim('/')
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

function Invoke-Hook {
  param(
    [string]$Command,
    [string]$Name
  )

  if ([string]::IsNullOrWhiteSpace($Command)) {
    return
  }

  Write-Host "[hook] $Name -> $Command"
  Invoke-Expression $Command
}

function Invoke-SftpBatch {
  param(
    [string]$CaseId,
    [string[]]$Commands,
    [string]$CaseLogPath
  )

  $batchPath = Join-Path (Split-Path -Parent $CaseLogPath) "$CaseId.batch"
  [System.IO.File]::WriteAllLines($batchPath, $Commands, [System.Text.UTF8Encoding]::new($false))

  $args = @('-P', $Port.ToString(), '-b', $batchPath)
  if (-not [string]::IsNullOrWhiteSpace($PrivateKeyPath)) {
    $args += @('-i', $PrivateKeyPath)
  }
  $args += "$User@$Host"

  $startedAt = Get-Date
  $rawOutput = & sftp @args 2>&1
  $exitCode = $LASTEXITCODE
  $endedAt = Get-Date
  $elapsed = $endedAt - $startedAt

  $outputText = ($rawOutput | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
  [System.IO.File]::WriteAllText($CaseLogPath, $outputText + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))

  return [PSCustomObject]@{
    ExitCode = $exitCode
    Elapsed = $elapsed
    BatchPath = $batchPath
    LogPath = $CaseLogPath
  }
}

function Write-ResultMarkdown {
  param(
    [string]$Path,
    [string]$RunId,
    [object[]]$Rows
  )

  $lines = @()
  $lines += '# SFTP Baseline Matrix Report'
  $lines += ''
  $lines += "run_id: $RunId"
  $lines += "generated_at: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')"
  $lines += ''
  $lines += '| Profile | Direction | Size | Status | Elapsed(s) | Throughput(MB/s) | Log |'
  $lines += '| --- | --- | --- | --- | ---: | ---: | --- |'

  foreach ($row in $Rows) {
    $lines += ('| {0} | {1} | {2} | {3} | {4:N3} | {5:N3} | {6} |' -f `
      $row.Profile, `
      $row.Direction, `
      $row.SizeLabel, `
      $row.Status, `
      $row.ElapsedSeconds, `
      $row.ThroughputMBps, `
      $row.LogFile)
  }

  $lines += ''
  $lines += 'Notes:'
  $lines += '- If RTT hooks are empty, `low-rtt`/`high-rtt` are labels only.'
  $lines += '- Use per-case logs to inspect sftp stderr/stdout and remote errors.'

  [System.IO.File]::WriteAllLines($Path, $lines, [System.Text.UTF8Encoding]::new($false))
}

$sftpCommand = Get-Command -Name sftp -ErrorAction SilentlyContinue
if (-not $sftpCommand) {
  throw 'sftp command not found in PATH. Install OpenSSH client first.'
}

$directions = @()
if (-not $SkipUpload) {
  $directions += 'upload'
}
if (-not $SkipDownload) {
  $directions += 'download'
}
if ($directions.Count -eq 0) {
  throw 'Both upload and download are skipped. Remove one skip flag.'
}

$runId = Get-Date -Format 'yyyyMMdd-HHmmss'
$runRoot = Join-Path $OutputDir $runId
$logDir = Join-Path $runRoot 'logs'
$localSeedDir = Join-Path $runRoot 'seed-files'
Ensure-Directory -Path $runRoot
Ensure-Directory -Path $logDir
Ensure-Directory -Path $localSeedDir

$remoteRunDir = Join-RemotePath -Parts @($RemoteBaseDir, "resh-speed-baseline-$runId")
$remoteSeedDir = Join-RemotePath -Parts @($remoteRunDir, 'seed')
$localDownloadDir = Join-Path $runRoot 'downloads'
Ensure-Directory -Path $localDownloadDir

$sizeCases = @(
  [PSCustomObject]@{ Name = 'small'; SizeBytes = [long]$SmallFileKB * 1KB; SizeLabel = "$SmallFileKB KB" },
  [PSCustomObject]@{ Name = 'large'; SizeBytes = [long]$LargeFileMB * 1MB; SizeLabel = "$LargeFileMB MB" }
)

$profiles = @(
  [PSCustomObject]@{ Name = 'low-rtt'; Apply = $ApplyLowRttCommand },
  [PSCustomObject]@{ Name = 'high-rtt'; Apply = $ApplyHighRttCommand }
)

Write-Host "[setup] remote run dir: $remoteRunDir"
Write-Host '[setup] preparing remote directories and download seed files'

$initCommands = @(
  ('mkdir "{0}"' -f $remoteRunDir),
  ('mkdir "{0}"' -f $remoteSeedDir)
)
$initLogPath = Join-Path $logDir '00-prepare-remote.log'
[void](Invoke-SftpBatch -CaseId '00-prepare-remote' -Commands $initCommands -CaseLogPath $initLogPath)

$seedMap = @{}
foreach ($sizeCase in $sizeCases) {
  $localSeedPath = Join-Path $localSeedDir ("seed-{0}.bin" -f $sizeCase.Name)
  New-ZeroFile -Path $localSeedPath -SizeBytes $sizeCase.SizeBytes

  $remoteSeedPath = Join-RemotePath -Parts @($remoteSeedDir, ("seed-{0}.bin" -f $sizeCase.Name))
  $seedMap[$sizeCase.Name] = [PSCustomObject]@{
    LocalSeedPath = $localSeedPath
    RemoteSeedPath = $remoteSeedPath
  }

  $prepareCommands = @(
    ('put "{0}" "{1}"' -f $localSeedPath, $remoteSeedPath)
  )
  $prepareLogPath = Join-Path $logDir ("01-seed-upload-{0}.log" -f $sizeCase.Name)
  $prepareResult = Invoke-SftpBatch -CaseId ("01-seed-upload-{0}" -f $sizeCase.Name) -Commands $prepareCommands -CaseLogPath $prepareLogPath
  if ($prepareResult.ExitCode -ne 0) {
    throw "Failed to upload seed file for $($sizeCase.Name). See $prepareLogPath"
  }
}

$results = New-Object System.Collections.Generic.List[object]

foreach ($profile in $profiles) {
  Invoke-Hook -Command $ClearRttCommand -Name 'clear-rtt'
  Invoke-Hook -Command $profile.Apply -Name $profile.Name

  foreach ($sizeCase in $sizeCases) {
    foreach ($direction in $directions) {
      $caseId = "$($profile.Name)-$direction-$($sizeCase.Name)"
      $caseLogPath = Join-Path $logDir ("$caseId.log")

      $commands = @()
      $transferBytes = $sizeCase.SizeBytes

      if ($direction -eq 'upload') {
        $remoteUploadPath = Join-RemotePath -Parts @($remoteRunDir, 'upload', ("$caseId.bin"))
        $commands += ('mkdir "{0}"' -f (Join-RemotePath -Parts @($remoteRunDir, 'upload')))
        $commands += ('put "{0}" "{1}"' -f $seedMap[$sizeCase.Name].LocalSeedPath, $remoteUploadPath)
      }
      else {
        $downloadPath = Join-Path $localDownloadDir ("$caseId.bin")
        if (Test-Path -LiteralPath $downloadPath) {
          Remove-Item -LiteralPath $downloadPath -Force
        }
        $commands += ('get "{0}" "{1}"' -f $seedMap[$sizeCase.Name].RemoteSeedPath, $downloadPath)
      }

      Write-Host "[run] $caseId"
      $result = Invoke-SftpBatch -CaseId $caseId -Commands $commands -CaseLogPath $caseLogPath
      $ok = $result.ExitCode -eq 0
      $elapsedSeconds = [Math]::Max($result.Elapsed.TotalSeconds, 0.000001)
      $throughput = if ($ok) { ($transferBytes / 1MB) / $elapsedSeconds } else { 0.0 }

      $results.Add([PSCustomObject]@{
        RunId = $runId
        Profile = $profile.Name
        Direction = $direction
        SizeLabel = $sizeCase.SizeLabel
        SizeBytes = $sizeCase.SizeBytes
        Status = if ($ok) { 'ok' } else { 'failed' }
        ElapsedSeconds = [Math]::Round($elapsedSeconds, 3)
        ThroughputMBps = [Math]::Round($throughput, 3)
        ExitCode = $result.ExitCode
        LogFile = "logs/$caseId.log"
      }) | Out-Null
    }
  }
}

Invoke-Hook -Command $ClearRttCommand -Name 'clear-rtt-final'

$csvPath = Join-Path $runRoot 'results.csv'
$mdPath = Join-Path $runRoot 'results.md'
$results | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding utf8
Write-ResultMarkdown -Path $mdPath -RunId $runId -Rows $results

Write-Host ''
Write-Host '[done] baseline matrix finished'
Write-Host "results_csv: $csvPath"
Write-Host "results_md : $mdPath"
