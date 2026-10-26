# Status

O console RGH real é o oráculo: o mesmo teste roda nos dois lados e os
registradores/memória são comparados. Um marco só fecha quando isso bate.

| | Marco | Estado | Onde |
|---|---|---|---|
| M0 | Dumps do RGH: flash, KV, fuses, CPU key | feito | [`research/flash/`](../research/flash/README.md) |
| M1 | Interpretador PPC executa CB_A → CB_B → CD, CE, CF/CG até o `rfid` final do CD | feito | [`research/cb/`](../research/cb/README.md) |
| M2 | HV + kernel virtual, 6 threads, SMC, MMU, security engine | em curso | [`research/hv/`](../research/hv/README.md) |
| M3 | `xboxkrnl` real + SMC completo | | |
| M4 | Southbridge: SATA/XTAF, USB HID, ethernet | | |
| M5 | GPU Xenos (command processor do Xenia como referência) | | |
| M6 | `xam` + dash; carregar Proto | | [`research/proto/`](../research/proto/README.md) |
| M7 | Live em bridge; resposta XOSC válida | | [`research/hv/security_engine_research.md`](../research/hv/security_engine_research.md) |

## M1 — fechado

O CD termina no emulador com **271.049.701 instruções** (271.049.695 antes da
resposta real do SMC `0x01`, que muda um ramo do boot). Decifra/descomprime CE,
autentica e executa CF, aplica CG e os 57 descritores de patch da NAND,
transporta os argumentos de boot e executa o `rfid` final.

Estado de saída: **PC `0x100`, MSR `0x1000000000000000`, HRMOR
`0x10000000000`**. Kernel-base e kernel 17559 conferidos byte a byte; fuses
originais, sem override.

## M2 — o que já está validado

M2 fecha pelo lado do emulador; o que sobra depende de hardware ou do M7.

- HV executando o kernel virtual, com os handlers de exceção/syscall do guest
  executados de verdade (sem HLE de retorno).
- Barreira multicore: o modo de seis threads (`--hv-threads 6`) passa a
  barreira e chega ao idle guest; o default segue uma CPU.
- SMC: comandos `0x01/0x12/0x07/0x0A/0x0F/0x17` batem com respostas reais.
- MMU: page walker SDR1 (PTEG primário/secundário), invalidação seletiva de TLB,
  encodings de `PP0`, HID6 (`SPR 0x3F9`, `DLP`/`LB`) e `LPCR[SF]`.
- Security engine: cifra XEX no caminho de dados das regiões 1/3, ligada por
  padrão — chaves reais quando `dumps/console/sec_keys_v25.txt` está presente,
  senão o modelo `generic` all-`0x55`; `--security-engine off` desliga e o
  caminho usado sai em `HV security path: ...` no log. A tripla de chaves do
  boot, lida da RAM do HV, decifra um par real de 4 KiB byte a byte. Tags de
  integridade em AES-CBC-MAC (modelo). Ver [security-engine.md](security-engine.md).

## M2 — o que falta

- XOSC real: o modelo usa AES-CBC-MAC sobre nonce/flags/CPU key, mas ainda não
  é uma resposta válida de Live (depende do M7).
- Chave por boot: o runner usa as chaves capturadas ou o modelo `generic`;
  gerar a chave do próprio boot segue em aberto.
- `xboxkrnl` real (M3) e southbridge (M4). O SMC atual é um mailbox validado
  contra o console, não o microcontrolador completo; o firmware 2.05 está em
  `dumps/smc-firmware/`.

## Próximo

M3 está em andamento: o kernel chega ao idle guest (seis threads, barreira
passada), mas não sai do idle sem eventos de dispositivo que ainda não estão
modelados. SMC completo, timer/IRQ e M4 ficam para quando o console estiver
disponível; nesse intervalo o trabalho offline é tooling e captura.

