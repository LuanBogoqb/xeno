# Arquitetura

Emulador LLE: o único atalho é o 1BL (mask ROM dentro do silício), que começa
já no CB_A com a CPU key do dump. Do CB_A até a dash, tudo é código real
executado pelo interpretador. Não há glitch: a verificação de assinatura do CB
é pulada, e a cadeia já vem patchada da NAND do dono (ver
[decisions.md](decisions.md)).

## Módulos (`crates/xenon-cpu/src`)

| Módulo | Papel |
|---|---|
| `cpu/` | interpretador PPC (estado, `step`, SPRs, exceções, syscalls); testes em `cpu/tests.rs` |
| `alu.rs` | helpers de 32/64 bits usados pelo interpretador |
| `mem/` | `bus` (regiões, aliases, RAM/SRAM/flash), `tlb`, `walk` (SDR1), `cipher` (regiões 1/3), `shared` (reservas/IIC); testes em `mem/tests.rs` |
| `mmu.rs` | SLB/TLB por PPE, HID6 (`0x3F9`), `DLP`/`LB`, PP0 e NX |
| `map.rs` | mapa de memória e estado que o 1BL deixa para o CB_A (chaves públicas, SRAM, fuses) |
| `runner.rs` | roda um estágio até o handoff, com hooks de PC |
| `hooks.rs` | ações por PC ("sem glitch") e logging de registradores |
| `multicore.rs` | escalonador determinístico de 6 threads (modo opt-in) |
| `security.rs` | modelo do security engine: AES-128 XEX + tweak Toeplitz |
| `xex/` | XEX2: header, extração AES+LZX (`lzx`), layout da imagem, imports e thunks |
| `xosc.rs` | modelo da resposta XOSC (experimental, não é resposta válida de Live) |
| `dev/` | dispositivos: `flash`, `smc`, `soc`, `fuses`, `iic` |
| `bin/runcb.rs` | CLI que monta o console e roda um estágio |
| `bin/opscan.rs` | lista palavras que o interpretador ainda não decodifica |

## Fluxo de boot

```
CB_A ──► CB_B ──► CD ──► (CE, CF, CG) ──► rfid ──► HV ──► kernel ──► dash
 SRAM     SRAM     RAM    decrypt/patch     PC 0x100
```

- **CB_A/CB_B**: verificações, configuração de hardware, carga do CD.
- **CD**: decifra e descomprime o CE, autentica e executa o CF, aplica o CG e
  os descritores de patch da NAND, monta os argumentos de boot e faz o `rfid`
  final. Termina com **PC `0x100`, MSR `0x1000000000000000`, HRMOR
  `0x10000000000`** e **271.049.701 instruções** nos dumps atuais.
- **HV**: inicializa o security engine, as MMUs e o kernel virtual; os handlers
  de exceção/syscall do guest são executados de verdade (sem HLE de retorno).
- **Kernel**: inicializa memória e threads até a barreira multicore.

Detalhes por estágio (offsets, chaves RC4, hashes de validação):
[`research/flash/README.md`](../research/flash/README.md) e
[`research/cb/README.md`](../research/cb/README.md).

## Mapa de memória

| Janela | O que é |
|---|---|
| `0x8000_0200_0000_0000` | SRAM do SoC (CB, 1BL, stack) |
| `0x8000_0200_0006_0000` | registradores do SoC |
| `0x8000_0200_EA00_1000` | mailbox do SMC (PCI BAR) |
| `0x8000_0000_0000_0000` + físico | visão física crua (ciphertext) |
| `0x8000_0100_0000_0000` + bloco·`0x200000000` + físico | RAM do HV (janela decriptada; 256 KiB em 4 blocos) |
| `0x8000_0300_0000_0000` + físico | kernel/guest |

A janela do HV não é um detalhe acadêmico: é por ela que o plugin de captura lê
a RAM do hypervisor a partir do kernel, incluindo a tabela de chaves de DRAM
(ver [security-engine.md](security-engine.md)).

## MMU

O page walker hasheado (SDR1) vive em `mem/`: calcula Hash0/Hash1, varre o PTEG
primário e o secundário (8 PTEs cada) e instala a tradução no TLB. Por cima:
TLB por PPE, `HRMOR` por PPE, invalidação seletiva (`tlbie`) e as checagens de
`PP0`/`NX`. O `HID6` (`SPR 0x3F9`; `0x3F6` é `HID5`) honra o bit `DLP` em
software e no walker, e o `LB` fica documentado. O TLB continua experimental:
formas observadas 2/4/8, sem SLB/VSID completos.

## HV, SMC e threads

- O HV executa o kernel virtual e mantém a barreira multicore.
- SMC é um mailbox de 4 palavras em `0xEA00_1000`; os comandos
  `0x01/0x12/0x07/0x0A/0x0F/0x17` foram validados contra respostas reais do
  console (ver [`research/hv/README.md`](../research/hv/README.md)).
- O modo de 6 threads (`--hv-threads 6`) é opt-in e determinístico: registradores
  e SLBs por thread, TLB/HRMOR por PPE, memória/IIC/reservas compartilhadas. Não
  é SMT fiel a ciclos. O default continua sendo uma CPU.

## Security engine

`security.rs` implementa AES-128 XEX com tweak Toeplitz/GF(2) e chaves
configuráveis. As chaves de DRAM são **por boot**; o HV mantém a tripla viva em
`0x80000102_000100F0` enquanto o console está ligado. O modelo foi validado
contra um par plaintext/ciphertext real de 4 KiB capturado do console — ver
[security-engine.md](security-engine.md).

## Onde os dados entram

`dumps/` (fora do git) é a fronteira: flash, keyvault, fuses, CPU key, estágios
decifrados (`dumps/stages/`) e capturas (`dumps/console/`). Nada disso é
necessário para compilar ou rodar os testes de unidade: só os testes de cadeia,
que são `#[ignore]` e leem esses arquivos como oráculo. Layout completo em
[getting-started.md](getting-started.md#dumps).
