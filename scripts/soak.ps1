param(
    [string]$Executable = (Join-Path $PSScriptRoot '..\..\TimeWrapped.exe'),
    [ValidateRange(0.01,168)][double]$Hours = 24,
    [string]$DataDirectory = (Join-Path $PSScriptRoot '..\work\soak-data'),
    [string]$ReportDirectory = (Join-Path $PSScriptRoot '..\work\soak-report')
)
$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$DataDirectory = [IO.Path]::GetFullPath($DataDirectory)
$ReportDirectory = [IO.Path]::GetFullPath($ReportDirectory)
New-Item -ItemType Directory -Force -Path $DataDirectory,$ReportDirectory | Out-Null
$env:TIMEWRAPPED_DATA_DIR = $DataDirectory
if (Test-Path (Join-Path $DataDirectory 'agent-diagnostics.json')) {
    throw 'Выберите новую папку DataDirectory: тест не должен смешиваться с предыдущим запуском.'
}
$collector = Start-Process -FilePath $Executable -ArgumentList '--background' -WindowStyle Hidden -PassThru
$samples = [Collections.Generic.List[object]]::new()
$timer = [Diagnostics.Stopwatch]::StartNew()
$previousCpu = 0.0
$previousTime = 0.0
try {
    while ($timer.Elapsed.TotalHours -lt $Hours) {
        Start-Sleep -Seconds 15
        $collector.Refresh()
        if ($collector.HasExited) { throw "Сборщик завершился с кодом $($collector.ExitCode)" }
        $elapsed = $timer.Elapsed.TotalSeconds
        $cpu = $collector.TotalProcessorTime.TotalSeconds
        $cpuPercent = 100 * ($cpu-$previousCpu) / ($elapsed-$previousTime) / [Environment]::ProcessorCount
        $metricsPath = Join-Path $DataDirectory 'agent-diagnostics.json'
        $metrics = if (Test-Path $metricsPath) { Get-Content -Raw -LiteralPath $metricsPath | ConvertFrom-Json } else { $null }
        $samples.Add([PSCustomObject]@{
            timestamp = (Get-Date).ToString('o'); elapsed_seconds = [math]::Round($elapsed,3)
            cpu_percent = [math]::Round($cpuPercent,6); working_set_bytes = $collector.WorkingSet64
            private_bytes = $collector.PrivateMemorySize64; write_operations = $metrics.write_operations
            write_bytes = $metrics.write_bytes; db_transactions = $metrics.db_transactions
            pending_segments = $metrics.pending_segments; flush_failures = $metrics.flush_failures
        })
        $previousCpu=$cpu; $previousTime=$elapsed
        $samples | Export-Csv -LiteralPath (Join-Path $ReportDirectory 'samples.csv') -NoTypeInformation -Encoding UTF8
        Write-Host ('{0:n1} h | CPU {1:n4}% | RAM {2:n2} MiB' -f ($elapsed/3600),$cpuPercent,($collector.WorkingSet64/1MB))
    }
} finally {
    Start-Process -FilePath $Executable -ArgumentList '--diagnostics' -WindowStyle Hidden -Wait
    $finalMetricsPath=Join-Path $DataDirectory 'agent-diagnostics.json'
    $finalMetrics=if(Test-Path $finalMetricsPath){Get-Content -Raw -LiteralPath $finalMetricsPath | ConvertFrom-Json}else{$null}
    if($finalMetrics){Copy-Item -LiteralPath $finalMetricsPath -Destination (Join-Path $ReportDirectory 'agent-final.json')}
    Start-Process -FilePath $Executable -ArgumentList '--quit' -WindowStyle Hidden -Wait
    $collector.WaitForExit(10000) | Out-Null
    $summary = [ordered]@{
        completed = ($timer.Elapsed.TotalHours -ge $Hours); requested_hours=$Hours
        measured_seconds=$timer.Elapsed.TotalSeconds; sample_count=$samples.Count
        mean_cpu_percent=($samples | Measure-Object cpu_percent -Average).Average
        peak_working_set_bytes=($samples | Measure-Object working_set_bytes -Maximum).Maximum
        peak_private_bytes=($samples | Measure-Object private_bytes -Maximum).Maximum
        final_write_operations=$finalMetrics.write_operations; final_write_bytes=$finalMetrics.write_bytes
        final_db_transactions=$finalMetrics.db_transactions; final_flush_failures=$finalMetrics.flush_failures
        notes='CPU normalized by logical processors. OS process I/O is not a physical SSD write counter. GPU/FPS and real sleep/lock require separate manual verification.'
    }
    $summary | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $ReportDirectory 'summary.json') -Encoding UTF8
    Write-Host "Отчёт: $ReportDirectory"
}
