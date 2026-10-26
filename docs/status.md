# Status

O console RGH real é o oráculo: o mesmo teste roda nos dois lados e os
registradores/memória são comparados. Um marco só fecha quando isso bate.

| | Marco | Estado | Onde |
|---|---|---|---|
| M0 | Dumps do RGH: flash, KV, fusos, CPU key | ✅ | [`research/flash/`](../research/flash/README.md) |
| M1 | Interpretador PPC executa CB_A → CB_B → CD, CE, CF/CG até o `rfid` final do CD | ✅ | [`research/cb/`](../research/cb/README.md) |
| M2 | HV + kernel virtual, 6 threads, SMC, MMU, security engine | 🚧 | [`research/hv/`](../research/hv/README.md) |
| M3 | `xboxkrnl` real + SMC completo | | |
| M4 | Southbridge: SATA/XTAF, USB HID, ethernet | | |
| M5 | GPU Xenos (command processor do Xenia como referência) | | |
| M6 | `xam` + dash; carregar Proto e GoldSpoof | | [`research/proto/`](../research/proto/README.md), [`research/goldspoof/`](../research/goldspoof/README.md) |
| M7 | Live em bridge; resposta XOSC válida | | [`research/hv/security_engine_research.md`](../research/hv/security_engine_research.md) |

## M1 — fechado

O CD termina no emulador com **271.049.701 instruções** (271.049.695 antes da
resposta real do SMC `0x01`, que muda um ramo do boot). Decifra/descomprime CE,
autentica e executa CF, aplica CG e os 57 descritores de patch da NAND,
transporta os argumentos de boot e executa o `rfid` final.

Estado de saída: **PC `0x100`, MSR `0x1000000000000000`, HRMOR
`0x10000000000`**. Kernel-base e kernel 17559 conferidos byte a byte; fusos
originais, sem override.

## M2 — o que já está validado

- HV executando o kernel virtual, com os handlers de exceção/syscall do guest
  executados de verdade (sem HLE de retorno).
- Barreira multicore: o modo de seis threads (`--hv-threads 6`) passa a
  barreira e chega ao idle guest; o default segue uma CPU.
- SMC: comandos `0x01/0x12/0x07/0x0A/0x0F/0x17` batem com respostas reais.
- MMU: page walker SDR1 (PTEG primário/secundário), invalidação seletiva de TLB,
  encodings de `PP0` e HID6 (`SPR 0x3F9`, `DLP`/`LB`).
- **Security engine validado em hardware**: a tripla de chaves de DRAM do boot
  (lida da RAM do HV) decifra um par real de 4 KiB byte a byte. Ver
  [security-engine.md](security-engine.md).

## M2 — o que falta

- XOSC real e o caminho de Live (dependem de M7; hoje há só um modelo
  experimental em `xosc.rs`).
- Modelo de chave por boot no runner (hoje o `--security-engine` é opt-in).
- `xboxkrnl` de verdade (M3) e os dispositivos de M4; o SMC atual é um mailbox,
  não o microcontrolador completo.

## Próximo passo sugerido

M2 fechado pelo lado do emulador (2026-10-26): o security engine agora liga por
padrão - modo `real` quando `dumps/console/sec_keys_v25.txt` (ou `--security-keys`)
está disponível, senão `generic` (all-`0x55`, distribuição segura); `--security-engine off`
desliga. O caminho usado é impresso em `HV security path: ...` no log do run.

M2 encerrada no escopo atual (2026-10-26): cifra XEX validada e no caminho de
dados; PP0 estendido, HID6 (DLP) e LPCR[SF] modelados; tags de integridade em
AES-CBC-MAC (modelo); XOSC refinado (AES-CBC-MAC, ainda não Live válido).

M3 (`xboxkrnl` real): em andamento. O kernel chega ao idle guest (6 threads)
com a barreira multicore passada, mas não sai do idle sem eventos de dispositivo
que não estão modelados. SMC permanece mailbox validado (6 comandos capturados).
SMC completo e M4 (southbridge) ficam para quando o console estiver disponível.

Próximo: retomar M3/M4 com o console (captura de SMC completo, timer/IRQ,
dispositivos). Enquanto isso, trabalho offline pode focar em tooling/docs.
