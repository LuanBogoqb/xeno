# SMC capture

Homebrew de console que consulta a versão do SMC pela mailbox do kernel
(`HalSendSMCMessage`, comando `0x12`) três vezes e grava `SMC-nn.TXT` ao lado do
XEX. O registro tem os 16 bytes de resposta, o retorno bruto da função e se o
primeiro byte corresponde ao comando.

O log é aberto antes da consulta e sincronizado depois de cada escrita: se o
console travar, o TXT parcial termina no último registro bom. A resposta neste
console é `12 62 02 05…` (versão 2.05), guardada em `dumps/smc/`.

## Build

```powershell
./tools/smc-capture/build.ps1 -InstallToolchain   # primeira vez
./tools/smc-capture/build.ps1
```

Usa Rust 1.97.1 com `rust-src` e a toolchain pública
[rust-xbox360](https://github.com/landaire/rust-xbox360), fixada no commit
`7b323d1733994add0a89956219ae7b3d830aa7eb`, sem o XDK. Saída em
`dist/SMC-Capture/` (`default.xex` e ZIP), fora do git.

## No console

1. Extraia `dist/SMC-Capture.zip` em um pendrive FAT32.
2. Pelo Aurora, execute `default.xex` na pasta.
3. A tela pode ficar preta; o programa retorna ao dashboard.
4. Recolha `SMC-00.TXT` (ou `01`, `02`…) da mesma pasta.

## Limites

A consulta acontece com o kernel já carregado. Não mede o handshake do boot, o
estado inicial das filas nem os tempos do SMC; uma resposta medida não valida
todo o modelo de mailbox. Não lê CPU key, KV ou fuses.

`link.x` e `powerpc-unknown-xbox360.json` vêm de `rust-xbox360` (MIT ou
Apache-2.0); o aviso está em `THIRD-PARTY-LICENSE.txt`. O XEX usa `xbox360-sys`
0.2.0 sob a mesma licença.
