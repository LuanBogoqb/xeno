# Checklist

Antes do commit, no diretório do repositório. Todo comando lista arquivos fora de
`docs/padroes/`. Saída vazia significa aprovado.

```powershell
$md = git ls-files | Where-Object { $_ -like '*.md' -and $_ -notlike 'docs/padroes/*' }
```

## 1. Termos proibidos

```powershell
$pat = 'agente|agentic|sub-agente|subagente|assistente|\bIA\b|\bprompt|skill|superpowers|image_gen|self-review|sessão|memória do (projeto|dono|agente|assistente)'
$md | ForEach-Object { Select-String -Path $_ -Pattern $pat -Encoding UTF8 }
```

No código e nos últimos commits:

```powershell
git ls-files | Where-Object { $_ -like '*.rs' -or $_ -like '*.py' -or $_ -like '*.c' -or $_ -like '*.cpp' } |
  ForEach-Object { Select-String -Path $_ -Pattern $pat -Encoding UTF8 }
git log -20 --format='%B' | Select-String -Pattern 'Co-Authored-By'
```

## 2. Rastro de máquina

```powershell
git ls-files | Where-Object { $_ -notlike 'docs/padroes/*' } |
  ForEach-Object { Select-String -Path $_ -Pattern 'AppData|/tmp/|Temp\\|opencode' -Encoding UTF8 }
```

## 3. Diário e edição por remendo

```powershell
$pat = '~~|Corrigido em|Correção:|Atualizado em|Obs\.:|Update:'
$md | ForEach-Object { Select-String -Path $_ -Pattern $pat -Encoding UTF8 }

$md | ForEach-Object { Select-String -Path $_ -Pattern '^#+ .*20[0-9][0-9]-' -Encoding UTF8 }
```

## 4. Emoji, e contagem que apodrece

```powershell
$pat = '[\u2600-\u27BF\uFE0F]|\b[0-9]+ testes'
$md | ForEach-Object { Select-String -Path $_ -Pattern $pat -Encoding UTF8 }
```

## 5. Encoding

```powershell
$strict = New-Object System.Text.UTF8Encoding($false, $true)
$md | ForEach-Object {
  $b = [System.IO.File]::ReadAllBytes($_)
  try { $s = $strict.GetString($b) } catch { "UTF-8 inválido: $_"; return }
  if ($b[0] -eq 0xEF) { "BOM:          $_" }
  if ($s -match "`r`n") { "CRLF:         $_" }
  if (-not $s.EndsWith("`n")) { "sem LF final: $_" }
}
```

A decodificação estrita é o teste de mojibake: byte que não forma sequência
UTF-8 válida indica arquivo salvo em Latin-1 ou texto UTF-8 repassado por uma
conversão de página de código.

## 6. Link e referência

Link relativo em Markdown tem de existir:

```powershell
$root = (Get-Location).Path
$md | ForEach-Object {
  $f = $_
  $dir = [System.IO.Path]::GetDirectoryName((Join-Path $root $f))
  $txt = Get-Content -Raw -Encoding UTF8 $f
  foreach ($m in [regex]::Matches($txt, '\]\(([^)\s]+)\)')) {
    $link = ($m.Groups[1].Value -split '#')[0]
    if ($link -match '^(https?:|mailto:|$)') { continue }
    $target = [System.IO.Path]::GetFullPath((Join-Path $dir $link))
    if (-not (Test-Path -LiteralPath $target)) { "link quebrado em $f -> $link" }
  }
}
```

Nome de arquivo em `code span` não é verificado por ninguém, e é onde o erro
costuma ficar. O comando abaixo resolve o nome contra o diretório do documento,
o `tools/`, o `research/` e a raiz:

```powershell
$root = (Get-Location).Path
$md | ForEach-Object {
  $f = $_
  $dir = [System.IO.Path]::GetDirectoryName((Join-Path $root $f))
  $txt = Get-Content -Raw -Encoding UTF8 $f
  foreach ($m in [regex]::Matches($txt, '`([A-Za-z0-9_.\-/]+\.(md|rs|py|bat|cpp|c|bin|toml|yml))`')) {
    $p = $m.Groups[1].Value
    if ($p -match '^(https?:|[A-Za-z]:)') { continue }
    $cand = @($dir, (Join-Path $root 'tools'), (Join-Path $root 'research'), $root)
    if (-not ($cand | Where-Object { Test-Path -LiteralPath (Join-Path $_ $p) })) {
      "referência não resolvida: $f -> $p"
    }
  }
}
```

## 7. Contra o código

Flag, default e nome de binário citados em texto têm de bater com o runner:

```powershell
Select-String -Path crates/xenon-cpu/src/bin/runcb.rs -Pattern 'usage:' | ForEach-Object { $_.Line }
```

Se o texto diz "default desligado" e o código diz `auto`, o texto está errado.

## 8. Leitura do diff

```powershell
git diff --stat
git diff
```

Procure, no diff: linha que conta o que o texto dizia antes, número solto sem
comando que o produza, e afirmação nova que contradiz outro arquivo.

## 9. Segredos e dados do console

Nada de dado do console versionado:

```powershell
git ls-files | Select-String -Pattern '(^dumps/|^samples/|^scratch/|\.bin$|\.xex$|\.profile$|sec_keys|^\.remember/|^target/)'
```

Hex grande em texto é chave ou hash. Hash pode; chave, não. Revise cada linha:

```powershell
git ls-files | Where-Object { $_ -like '*.md' -or $_ -like '*.txt' -or $_ -like '*.rs' } |
  ForEach-Object { Select-String -Path $_ -Pattern '\b[0-9a-fA-F]{32}\b' -Encoding UTF8 } |
  ForEach-Object { "{0}:{1}: {2}" -f $_.Path, $_.LineNumber, $_.Line.Trim() }
```

Aprovado quando cada ocorrência é hash, chave pública, a 1BL key ou uma chave
mascarada. O `.gitignore` tem de cobrir toda pasta local nova:

```powershell
git status --ignored --short | Select-String -Pattern '^!!'
```

## 10. Organização

Arquivo novo tem de estar no índice do diretório:

```powershell
# docs e research listam tudo que existe?
$doc = Get-Content -Raw docs/README.md
Get-ChildItem docs -File -Filter *.md | Where-Object { $_.Name -ne 'README.md' } |
  Where-Object { $doc -notmatch [regex]::Escape($_.Name) } | ForEach-Object { "fora do índice: docs/$($_.Name)" }

$res = Get-Content -Raw research/README.md
Get-ChildItem research -Directory | Where-Object { $res -notmatch [regex]::Escape($_.Name) } | ForEach-Object { "fora do índice: research/$($_.Name)" }
```

Nome proibido:

```powershell
git ls-files | Select-String -Pattern '(^|/)(utils|helpers|common|misc|temp|tmp|old|new|final|copy|backup)(\.|/|$)'
```

E o básico: `git status --short` sem surpresa antes do commit, um assunto por
commit, build e testes de unidade passando.

