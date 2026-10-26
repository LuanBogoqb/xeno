<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/banner-dark.png">
    <img src="assets/brand/banner-light.png" alt="XENO" width="720">
  </picture>
</p>

# XENO

[![CI](https://github.com/LuanBogoqb/xb-emu/actions/workflows/ci.yml/badge.svg)](https://github.com/LuanBogoqb/xb-emu/actions/workflows/ci.yml)

Emulador de sistema completo (LLE) do Xbox 360: boot real da NAND, hypervisor,
kernel e dash originais, sem HLE do sistema operacional. Alvo final: uma ISO
bootável (Linux por baixo) que se comporta como um 360 de verdade.

## Status

| | Marco | Estado |
|---|---|---|
| M0 | Dumps do console (flash, KV, fuses, CPU key) | feito |
| M1 | Boot chain até o handoff do CD em `0x100` | feito |
| M2 | HV + kernel virtual, 6 threads, SMC, MMU, security engine | em curso |
| M3 | `xboxkrnl` real + SMC completo | |
| M4 | Southbridge: SATA/XTAF, USB HID, ethernet | |
| M5 | GPU Xenos | |
| M6 | `xam` + dash + plugins (Proto, GoldSpoof) | |
| M7 | Live em bridge, XOSC válido | |

M2 segue aberto, mas o núcleo já roda: **271.049.701 instruções** até o handoff
do CD, HV chegando ao idle guest, e o **security engine validado em hardware**
(AES-128 XEX decifrando um par real com as chaves de DRAM lidas da RAM do HV).

## Começando

```powershell
cargo build --release
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage cd --max-steps 400000000

cargo test -p xenon-cpu     # testes de unidade, sem dumps
```

Requisitos, flags e o layout de `dumps/`:
[docs/getting-started.md](docs/getting-started.md).

## Estrutura

| Caminho | Conteúdo |
|---|---|
| `crates/xenon-cpu/` | emulador: PPC, memória/MMU, HV, SMC, security engine, XOSC |
| `tools/` | análise Python da boot chain e dos XEX (`xbolib/` é a base compartilhada) |
| `tools/capture/` | captura no console: plugin de kernel e homebrews XDK |
| `research/` | engenharia reversa do console e dos plugins (um README por alvo) |
| `docs/` | guias, arquitetura, status e decisões |
| `assets/brand/` | identidade visual |
| `dumps/` | entradas e saídas locais (**fora do git**) |

## Documentação

O índice completo fica em [docs/README.md](docs/README.md). Os pontos de
entrada mais usados:

- [Arquitetura](docs/architecture.md) — módulos, mapa de memória, fluxo de boot.
- [Status](docs/status.md) — marcos M0–M7, o que está validado e o que falta.
- [Começando](docs/getting-started.md) — compilar, rodar, testar.
- [Pesquisa](research/README.md) — notas de engenharia reversa e disassembly.
- [Padrões do repositório](docs/padroes/README.md) — texto, organização e segurança.
