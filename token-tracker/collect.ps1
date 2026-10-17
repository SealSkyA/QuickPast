# =============================================================================
# Token 用量采集脚本（零依赖，Windows PowerShell 5.1 兼容）
#
# 功能：
#   1. 读取 OhMyAgent 会话数据 sessions/*/messages.jsonl（UTF-8）
#   2. 从 context_tokens 记录提取每次请求的「输入 token（实际/估算）」
#   3. 从 assistant 消息内容估算每次请求的「输出 token」
#   4. 聚合生成 data/usage_stats.json + data/usage_detail.csv
#   5. 用 dashboard.template.html 渲染生成自包含仪表盘 dashboard.html
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File collect.ps1
#   可选参数：
#     -DataDir <路径>   指定 OhMyAgent 数据目录（默认 %APPDATA%\com.chaitin.baizhi.monkeycode\ohmyagent）
#                       也支持环境变量 OHMYAGENT_DATA_DIR 覆盖
#     -OutDir  <路径>   指定输出目录（默认 脚本目录\data）
# =============================================================================

[CmdletBinding()]
param(
    [string]$DataDir = '',
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# 0. 路径解析
# ---------------------------------------------------------------------------
if (-not $DataDir) {
    if ($env:OHMYAGENT_DATA_DIR) { $DataDir = $env:OHMYAGENT_DATA_DIR }
    else { $DataDir = Join-Path $env:APPDATA 'com.chaitin.baizhi.monkeycode\ohmyagent' }
}
if (-not $OutDir) { $OutDir = Join-Path $PSScriptRoot 'data' }

$sessionsDir = Join-Path $DataDir 'sessions'
if (-not (Test-Path $sessionsDir)) {
    throw "找不到 OhMyAgent 会话目录：$sessionsDir`n请用 -DataDir 指定数据目录。"
}
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

$outJson = Join-Path $OutDir 'usage_stats.json'
$outCsv  = Join-Path $OutDir 'usage_detail.csv'
$outHtml = Join-Path $PSScriptRoot 'dashboard.html'
$tmpl    = Join-Path $PSScriptRoot 'dashboard.template.html'
if (-not (Test-Path $tmpl)) { throw "缺少模板文件：$tmpl" }

# ---------------------------------------------------------------------------
# 1. 工具函数
# ---------------------------------------------------------------------------

# 读取 JSONL 文件（显式 UTF-8），返回记录数组。
# 逐字符扫描大括号深度 + 字符串状态，可容忍字符串内出现换行的脏数据；
# 单条记录解析失败时跳过并给出提示，不影响整体。
function Read-JsonlRecords {
    param([string]$Path)
    $records = New-Object System.Collections.ArrayList
    if (-not (Test-Path $Path)) { return $records }
    $raw = [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
    if ([string]::IsNullOrEmpty($raw)) { return $records }

    $sb    = New-Object System.Text.StringBuilder
    $inStr = $false
    $esc   = $false
    $depth = 0

    foreach ($ch in $raw.ToCharArray()) {
        if ($inStr) {
            [void]$sb.Append($ch)
            if ($esc) { $esc = $false }
            elseif ($ch -eq '\') { $esc = $true }
            elseif ($ch -eq '"') { $inStr = $false }
            continue
        }
        switch ($ch) {
            '"' {
                $inStr = $true
                [void]$sb.Append($ch)
            }
            '{' {
                $depth++
                [void]$sb.Append($ch)
            }
            '}' {
                if ($depth -gt 0) {
                    $depth--
                    [void]$sb.Append($ch)
                    if ($depth -eq 0) {
                        $json = $sb.ToString()
                        [void]$sb.Clear()
                        try {
                            $obj = $json | ConvertFrom-Json
                            if ($null -ne $obj) { [void]$records.Add($obj) }
                        }
                        catch {
                            Write-Verbose "跳过无法解析的记录：$($_.Exception.Message)"
                        }
                    }
                }
            }
            default { [void]$sb.Append($ch) }
        }
    }
    return $records
}

# 估算文本 token 数：
#   CJK（中文/日文/全角）约 1.5 字符/token，其余（英文/数字/符号）约 4 字符/token
function Estimate-Tokens {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return 0 }
    $cjk   = 0
    $other = 0
    foreach ($ch in $Text.ToCharArray()) {
        $cp = [int]$ch
        if (($cp -ge 0x3400 -and $cp -le 0x4DBF) -or   # CJK 扩展 A
            ($cp -ge 0x4E00 -and $cp -le 0x9FFF) -or   # CJK 统一表意文字
            ($cp -ge 0x3000 -and $cp -le 0x30FF) -or   # CJK 标点 / 日文假名
            ($cp -ge 0xFF00 -and $cp -le 0xFFEF))      # 全角字符
        { $cjk++ }
        else { $other++ }
    }
    return [int]([math]::Ceiling($cjk / 1.5) + [math]::Ceiling($other / 4.0))
}

# 提取 assistant 消息的全部输出文本（thinking + text + tool_use 入参）
function Get-AssistantText {
    param([object]$Msg)
    $parts = New-Object System.Collections.ArrayList
    if ($null -eq $Msg.content_v2) { return '' }
    foreach ($c in $Msg.content_v2) {
        if ($c.type -eq 'text' -or $c.type -eq 'thinking') {
            if ($null -ne $c.text -and "$($c.text)" -ne '') { [void]$parts.Add([string]$c.text) }
        }
        elseif ($c.type -eq 'tool_use') {
            if ($null -ne $c.input) { [void]$parts.Add(($c.input | ConvertTo-Json -Compress -Depth 20)) }
        }
    }
    return ($parts -join "`n")
}

# 统计 assistant 消息中的工具调用次数
function Get-AssistantToolCalls {
    param([object]$Msg)
    $n = 0
    if ($null -ne $Msg.content_v2) {
        foreach ($c in $Msg.content_v2) { if ($c.type -eq 'tool_use') { $n++ } }
    }
    return $n
}

function Get-LocalTime {
    param([int64]$UnixTs)
    try { return [DateTimeOffset]::FromUnixTimeSeconds($UnixTs).LocalDateTime }
    catch { return Get-Date }
}

# 按属性名累加数值（兼容 OrderedDictionary，规避 Measure-Object -Property 的解析问题）
function Get-Sum {
    param($Rows, [string]$Prop)
    $sum = [int64]0
    foreach ($r in $Rows) {
        $v = $r.$Prop
        if ($null -ne $v) { $sum += [int64]$v }
    }
    return $sum
}

# CSV 单元格转义
function Get-CsvCell {
    param([object]$Value)
    $s = [string]$Value
    if ($s -match '[,"\r\n]') { return '"' + ($s -replace '"', '""') + '"' }
    return $s
}

# ---------------------------------------------------------------------------
# 2. 扫描所有会话
# ---------------------------------------------------------------------------
Write-Host "会话目录：$sessionsDir"
$allDetail  = New-Object System.Collections.ArrayList
$sessionInfos = New-Object System.Collections.ArrayList

Get-ChildItem $sessionsDir -Directory | Sort-Object Name | ForEach-Object {
    $sid     = $_.Name
    $mf      = Join-Path $_.FullName 'messages.jsonl'
    if (-not (Test-Path $mf)) { return }

    $recs = Read-JsonlRecords $mf
    if ($recs.Count -eq 0) { return }
    Write-Host ("  解析会话 {0}（{1} 条记录）" -f $sid, $recs.Count)

    $metaRec   = @($recs | Where-Object { $_.type -eq 'meta' } | Sort-Object { $_.ts })[0]
    $titleRecs = @($recs | Where-Object { $_.type -eq 'title' } | Sort-Object { $_.ts })
    $sessionModel = ''
    if ($metaRec) { $sessionModel = [string]$metaRec.model }
    $sessionTitle = ''
    if ($titleRecs.Count -gt 0) {
        $sessionTitle = [string]$titleRecs[$titleRecs.Count - 1].title
    }
    if (-not $sessionTitle) { $sessionTitle = "(未命名会话)" }

    # 上下文 token 记录（一次真实请求 = 一条记录，retry 不新增，无需去重）
    $ctxList = @($recs | Where-Object { $_.type -eq 'context_tokens' } | Sort-Object { $_.ts })
    # assistant 消息
    $asstList = @($recs | Where-Object { $_.type -eq 'message' -and $_.role -eq 'assistant' } | Sort-Object { $_.ts })

    if ($ctxList.Count -eq 0) { return }

    # 将每条 assistant 消息归属到「最近的前一条请求」（ctx 时间 <= 消息时间）
    $asstOwner = New-Object int[] $asstList.Count
    $ci = 0
    for ($i = 0; $i -lt $asstList.Count; $i++) {
        while (($ci + 1) -lt $ctxList.Count -and $ctxList[$ci + 1].ts -le $asstList[$i].ts) { $ci++ }
        if ($ctxList[$ci].ts -le $asstList[$i].ts) { $asstOwner[$i] = $ci } else { $asstOwner[$i] = -1 }
    }

    $groups = @{}
    for ($i = 0; $i -lt $asstList.Count; $i++) {
        if ($asstOwner[$i] -lt 0) { continue }
        $key = $asstOwner[$i].ToString()
        if (-not $groups.ContainsKey($key)) { $groups[$key] = New-Object System.Collections.ArrayList }
        [void]$groups[$key].Add($asstList[$i])
    }

    $modelCounts = @{}
    for ($k = 0; $k -lt $ctxList.Count; $k++) {
        $ctxRec = $ctxList[$k]
        $mdl = ''
        if ($ctxRec.context_tokens -and $ctxRec.context_tokens.model_id) { $mdl = [string]$ctxRec.context_tokens.model_id }
        if (-not $mdl) { $mdl = $sessionModel }
        if (-not $mdl) { $mdl = '(未知)' }

        $dt = Get-LocalTime ([int64]$ctxRec.ts)
        $key = $k.ToString()

        $outText   = ''
        $outChars  = 0
        $toolCalls = 0
        if ($groups.ContainsKey($key)) {
            foreach ($m in $groups[$key]) {
                $t = Get-AssistantText $m
                $outText   += $t
                $outChars  += $t.Length
                $toolCalls += Get-AssistantToolCalls $m
            }
        }
        $outEst = Estimate-Tokens $outText

        $inpActual    = [int64]0
        $inpEstimated = [int64]0
        $msgCount     = 0
        $fp           = ''
        if ($ctxRec.context_tokens) {
            if ($null -ne $ctxRec.context_tokens.baseline_actual_tokens)    { $inpActual    = [int64]$ctxRec.context_tokens.baseline_actual_tokens }
            if ($null -ne $ctxRec.context_tokens.baseline_estimated_tokens) { $inpEstimated = [int64]$ctxRec.context_tokens.baseline_estimated_tokens }
            if ($null -ne $ctxRec.context_tokens.message_count)             { $msgCount     = [int64]$ctxRec.context_tokens.message_count }
            if ($ctxRec.context_tokens.request_fingerprint)                 { $fp = [string]$ctxRec.context_tokens.request_fingerprint }
        }

        if ($modelCounts.ContainsKey($mdl)) { $modelCounts[$mdl]++ } else { $modelCounts[$mdl] = 1 }

        [void]$allDetail.Add([ordered]@{
            ts              = $dt.ToString('yyyy-MM-dd HH:mm:ss')
            date            = $dt.ToString('yyyy-MM-dd')
            time            = $dt.ToString('HH:mm:ss')
            session_id      = $sid
            session_title   = $sessionTitle
            model           = $mdl
            input_actual    = $inpActual
            input_estimated = $inpEstimated
            output_estimated = $outEst
            output_chars    = $outChars
            tool_calls      = $toolCalls
            message_count   = $msgCount
            fingerprint     = $fp
        })
    }

    # 会话汇总
    if ($allDetail.Count -gt 0) {
        $sessRows = @($allDetail | Where-Object { $_.session_id -eq $sid })
        $firstDt = $sessRows[0].ts
        $lastDt  = $sessRows[$sessRows.Count - 1].ts
        [void]$sessionInfos.Add([ordered]@{
            id               = $sid
            title            = $sessionTitle
            model            = $sessionModel
            requests         = $sessRows.Count
            input_actual     = (Get-Sum $sessRows 'input_actual')
            input_estimated  = (Get-Sum $sessRows 'input_estimated')
            output_estimated = (Get-Sum $sessRows 'output_estimated')
            total_estimated  = (Get-Sum $sessRows 'input_actual') + (Get-Sum $sessRows 'output_estimated')
            first_ts         = $firstDt
            last_ts          = $lastDt
        })
    }
}

if ($allDetail.Count -eq 0) {
    Write-Host '未找到任何包含 context_tokens 记录的会话。' -ForegroundColor Yellow
    exit 1
}

# ---------------------------------------------------------------------------
# 3. 聚合
# ---------------------------------------------------------------------------
$reqTotal        = $allDetail.Count
$inpActualSum    = Get-Sum $allDetail 'input_actual'
$inpEstSum       = Get-Sum $allDetail 'input_estimated'
$outEstSum       = Get-Sum $allDetail 'output_estimated'
$outCharSum      = Get-Sum $allDetail 'output_chars'
$toolCallSum     = Get-Sum $allDetail 'tool_calls'
$totalEstSum     = $inpActualSum + $outEstSum

# 按日
$byDay = @($allDetail | Group-Object { $_.date } | Sort-Object { [datetime]$_.Name } | ForEach-Object {
    $rows = @($_.Group)
    [ordered]@{
        date             = $_.Name
        requests         = $rows.Count
        input_actual     = (Get-Sum $rows 'input_actual')
        input_estimated  = (Get-Sum $rows 'input_estimated')
        output_estimated = (Get-Sum $rows 'output_estimated')
        total_estimated  = (Get-Sum $rows 'input_actual') + (Get-Sum $rows 'output_estimated')
    }
})

# 按模型
$byModel = @($allDetail | Group-Object { $_.model } | Sort-Object Name | ForEach-Object {
    $rows = @($_.Group)
    [ordered]@{
        model            = $_.Name
        requests         = $rows.Count
        input_actual     = (Get-Sum $rows 'input_actual')
        output_estimated = (Get-Sum $rows 'output_estimated')
        total_estimated  = (Get-Sum $rows 'input_actual') + (Get-Sum $rows 'output_estimated')
    }
})

# 按会话（倒序，最新在前）
$bySession = @($sessionInfos | Sort-Object { [datetime]($_.last_ts) } -Descending | ForEach-Object { $_ })

$stats = [ordered]@{
    generated_at = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    data_dir     = $DataDir
    summary      = [ordered]@{
        requests          = $reqTotal
        input_actual      = $inpActualSum
        input_estimated   = $inpEstSum
        output_estimated  = $outEstSum
        output_chars      = $outCharSum
        total_estimated   = $totalEstSum
        tool_calls        = $toolCallSum
        sessions          = $sessionInfos.Count
    }
    by_day       = $byDay
    by_model     = $byModel
    by_session   = $bySession
    detail       = $allDetail
}

# ---------------------------------------------------------------------------
# 4. 输出 JSON / CSV
# ---------------------------------------------------------------------------
$json = $stats | ConvertTo-Json -Depth 8 -Compress
[System.IO.File]::WriteAllText($outJson, $json, (New-Object System.Text.UTF8Encoding $false))

$csvLines = New-Object System.Collections.ArrayList
[void]$csvLines.Add('时间,会话ID,会话标题,模型,输入token(实际),输入token(估算),输出token(估算),输出字符数,工具调用次数,上下文消息数')
foreach ($r in $allDetail) {
    [void]$csvLines.Add((
        @(Get-CsvCell $r.ts; Get-CsvCell $r.session_id; Get-CsvCell $r.session_title; Get-CsvCell $r.model;
          Get-CsvCell $r.input_actual; Get-CsvCell $r.input_estimated; Get-CsvCell $r.output_estimated;
          Get-CsvCell $r.output_chars; Get-CsvCell $r.tool_calls; Get-CsvCell $r.message_count) -join ','))
}
[System.IO.File]::WriteAllText($outCsv, ($csvLines -join "`r`n"), (New-Object System.Text.UTF8Encoding $true))

# ---------------------------------------------------------------------------
# 5. 生成仪表盘 HTML
# ---------------------------------------------------------------------------
$tmplText   = [System.IO.File]::ReadAllText($tmpl, [System.Text.Encoding]::UTF8)
$jsonEsc    = $json -replace '<', '\u003c'   # 防止 </script> 等破坏内嵌脚本块
$html       = $tmplText.Replace('__STATS_JSON__', $jsonEsc).Replace('__GENERATED_AT__', (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))
[System.IO.File]::WriteAllText($outHtml, $html, (New-Object System.Text.UTF8Encoding $true))

# ---------------------------------------------------------------------------
# 6. 控制台汇总
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '==================== Token 用量汇总 ====================' -ForegroundColor Cyan
Write-Host ('  会话数            : {0}' -f $sessionInfos.Count)
Write-Host ('  请求次数          : {0}' -f $reqTotal)
Write-Host ('  输入 token(实际)  : {0:N0}' -f $inpActualSum)
Write-Host ('  输入 token(估算)  : {0:N0}' -f $inpEstSum)
Write-Host ('  输出 token(估算)  : {0:N0}  （输出字符 {1:N0}）' -f $outEstSum, $outCharSum)
Write-Host ('  合计 token(估算)  : {0:N0}' -f $totalEstSum)
Write-Host ('  工具调用次数      : {0:N0}' -f $toolCallSum)
Write-Host '--------------------------------------------------------'
Write-Host ('  明细记录          : {0} 条' -f $allDetail.Count)
Write-Host ('  JSON              : {0}' -f $outJson)
Write-Host ('  CSV               : {0}' -f $outCsv)
Write-Host ('  仪表盘            : {0}' -f $outHtml)
Write-Host ('  采集时间          : {0}' -f (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))
Write-Host '========================================================' -ForegroundColor Cyan
