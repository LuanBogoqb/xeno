# HV → kernel 17559

## O que roda hoje

A cadeia original continua sendo executada até o CD (271.049.701 passos com a
resposta SMC `0x01` real; o valor anterior era 271.049.695). `--stage hv`
continua a partir desse estado vivo: não injeta o kernel offline, não adiciona
hooks de salto e não modifica os fuses.

- Entrada do HV em PC `0x100`, HRMOR `0x10000000000`.
- Inicialização do HV e `rfid` para o kernel virtual em `0x80078DB0`.
- Misses de tradução passam pelos handlers PPC do próprio HV; esses handlers
  escrevem os SPRs de software-TLB e retornam à instrução do kernel.
- Primeiro `sc` do kernel em `0x8007F97C` (`r0=15`) entra no handler real em
  `0xC00`, com SRR0 `0x8007F980`. Há teste de integração desse ponto.
- Depois disso, chamadas do HV, PCI/SMC e Xenos, inicialização de memória e a
  primeira thread. No modo de seis threads, a sincronização multicore passa e
  os seis chegam ao loop idle do guest.

A execução não chega à dash: falta evento de dispositivo para sair do idle.

Os vetores de SMC usados no modelo são do console real (ConsoleCapture v2,
`dumps/console/CAP00/`): `0x01` (`01 11 00 0A...`), versão `0x12`
(`12 62 02 05...`), temperatura `0x07`, bandeja `0x0A`, AV `0x0F` e orientação
`0x17`. Eles estão em `crates/xenon-cpu/src/dev/smc.rs`.

```powershell
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-stop-pc c00
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-max-steps 20000000 --hv-stop-pc 8015d440
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 20000000 --hv-stop-pc 8015d44c
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 40000000 --log-pc 8015d44c:10
cargo test -p xenon-cpu --release -- --include-ignored
```

`--max-steps` é o orçamento de cada estágio CB_B/CD; `--hv-max-steps` cobre o
HV/kernel (padrão: 5 milhões) e `--hv-stop-pc` para antes de executar o PC
indicado. O log inclui SRR0/1, DAR, DSISR, SLB e as últimas entradas brutas do
software-TLB.

Os testes que precisam dos dumps locais ficam marcados `#[ignore]` e cobrem a
entrada do syscall, a chegada à barreira multicore e as quatro regressões de M1,
que seguem byte-idênticas.

## Sincronização multicore

No modo single-CPU, depois de 100 milhões de passos o processador permanece em
`0x8015D440`. O código em `0x8015D400` prepara os PCRs dos threads 1..5, chama a
rotina de inicialização em `0x80077050` e aguarda:

```text
8015D440: lwz     r10,0x5748(r11)   ; r11 = 0x801A0000
8015D444: cmplwi  cr6,r10,5
8015D448: bne     cr6,0x8015D43C
```

O primeiro alcance desse PC ocorre em 10.713.316 passos HV/kernel, e um teste de
integração verifica o marco, o contador zero e o código do HV preservado depois
da inicialização das páginas físicas do kernel. O contador não chega a 5 com uma
CPU só: a execução longa consumia o orçamento nessa espera, sem parar por opcode
ausente.

A continuação modela os seis contextos, a partida dos secundários, IIC/IPIs e o
estado de tradução por thread/PPE, agora disponível com `--hv-threads 6`. O HV
`sc1` escreve os IDs lógicos 4/16 dos PPEs secundários, emite o pacote IPI
`0x140078` e habilita o sibling do PPE0 com `CTRL=0xC00000`. O modelo entrega
reset aos PPEs acordados e aos siblings habilitados; cada secundário executa o
vetor `0x100`, monta seu próprio PCR/SLB e entra em `KiSystemStartup`. As
instruções guest `lwarx/stwcx.` incrementam o contador, e nenhum hook escreve o
contador nem salta o loop. A CPU0 passa em `0x8015D44C` com r10=5 depois de
10.725.123 passos totais no primeiro teste de seis threads (quantum
determinístico de 256 instruções).

Os secundários não executaram a cadeia CB/CD completa: o bootstrap pós-CD começa
com eles desabilitados, herdando o HRMOR do boot por PPE. Esse estado inicial e
o reset por wake IPI são inferidos do protocolo guest, não uma validação
completa do power/reset do Xenon físico.

Depois da barreira, `andis.` e as formas indexadas com update entraram. Uma
execução de 100 milhões de passos terminou com os seis threads no loop idle
`0x8007588C..0x8007589C`, sem opcode ausente. `0x80075874` isoladamente não é
marco suficiente, porque a CPU0 também visita o idle antes da partida multicore;
a regressão longa exige o log de `0x8015D44C` com r10=5 e o PC final dos seis
threads no idle.

## Falha anterior, resolvida e não contornada

A primeira versão chegava a uma falta por ponteiro nulo em `0x80073144`
(`KeWaitForSingleObject`, DAR `0xA6`, `PCR+0x100` nulo). O chamador era
`ExAcquireReadWriteLockExclusive`: incrementava um lock `0xFFFFFFFF` e deveria
reconhecer zero em `addic.`. O interpretador comparava o resultado de 64 bits
(`0x100000000`) mesmo no kernel com SF=0, o que causava espera prematura antes
de existir thread atual. CR0 e carry/borrow agora respeitam o tamanho de palavra
no modo pós-CD, com testes de fronteira 32/64. Não houve preenchimento de PCR,
override de resposta SMC nem salto de handler.

## O que falta

- XOSC e o caminho de Live. O XOSC não é uma função criptográfica do HV: o
  `xam.mem` mostra a string `/xedl/chalresp/c%uv%u/xosc.xex` e uma chamada ao
  ordinal 410 (`XexLoadImageFromMemory`), então é um `.xex` separado, baixado do
  Live e executado. O modelo atual usa AES-CBC-MAC sobre nonce/flags/CPU key e
  ainda não é uma resposta válida. O `tools/capture/xosc_capture.c` é um homebrew
  XDK que grava `xosc_capture.bin` ao lado do `.xex` e consulta o SMC via
  `HalSendSMCMessage`; como o `.xex` real vem do Live, a captura ainda usa um
  placeholder. Para comparar, `runcb ... --xosc-test challenge.bin` usa
  `challenge.bin[20..52]` como resposta esperada.
- Chave de DRAM por boot: a captura resolve o lado da validação, falta decidir o
  modelo do runner para o boot seguinte.
- PP0 estendido (`bit 3` do RPN) e a decodificação confirmada do HID6; composição
  completa de `LPCR`/`RMOR`.
- PCI devolve vários registradores não inicializados e não há entrega completa de
  interrupções. O SMC é um mailbox com seis comandos validados, não o
  microcontrolador.
- Tags de integridade: a cifra é real, o CRC por linha ainda é modelo.

## Modelo e limites

- `lwarx/ldarx`, `stwcx./stdcx.`, `cntlzd`, operações lógicas de CR e aritmética
  com carry cobrem os caminhos executados. Reservas são de uma linha de 128
  bytes, com invalidação por escrita no backing físico e das reservas de todos os
  owners no modo de seis threads. Não é um modelo de cache coerente ou
  cycle-accurate.
- SLB/VSID e software-TLB: páginas de 4 KiB, 64 KiB e 16 MiB. O modo novo só
  ativa depois do handoff do CD, para não trocar a interpretação dos locked
  caches já validada em M1. A seleção grande LP=0/1 é a observada no boot
  (64 KiB/16 MiB), não uma decodificação confirmada do HID6 do Xenon.
- Exceções de storage/segment e `sc` entram no código guest do HV. A tabela base
  PP/Ks/Kp e o NX de segmento/página geram faltas de proteção reais, com testes
  de store sem efeito e de ISI/DSI. PP0 estendido é rejeitado de forma
  conservadora. A invalidação é seletiva por página (`tlbie`/`tlbiel`) e o page
  walker de hardware (SDR1 hashed PTEG, primário/secundário, bits R/C) está
  implementado e testado, mas o boot usa TLB software-managed e não o exercita.
- Regiões 1 e 3 leem e escrevem através da cifra quando o engine está ligado, mas
  o backing continua plaintext: depois do CD, a visão hashed recebe um snapshot
  esparso da DRAM viva e as escritas físicas/encrypted deixam de sobrescrever o
  código plaintext do HV. Os key selectors ainda compartilham a mesma visão
  hashed. O modelo cobre o isolamento entre duas visões, não cache seguro, tags
  de integridade ou autenticação.
- Timebase por PPE e decrementer por thread, latched e entregue em `0x900` apenas
  com EE habilitado. O tempo é determinístico em instruções emuladas, não 50 MHz
  calibrados, e não há HDEC completo nem fontes periódicas externas. IRQ de
  dispositivo não é fabricada para sair do idle.
- Fuses permanecem os originais; o único override de digest é o do CB_A, já
  preexistente.

## Captura no console

A tentativa de ler a região 1 (hashed RAM, `0x0100_0000_0000`) e a DRAM física
(`0x8000_0000_0000_0000`) a partir de um título falhou com `r1_ok=0` e
`dram_ok=0`: um título de usuário não alcança esses ranges. A captura só saiu em
kernel mode, com um plugin do Dashlaunch (`tools/capture/seccapture.cpp`) usando
o memcpy do freeboot (`HvxGetVersions`, magic `0x72627472`, op 5) para copiar das
regiões reais (bits 40..43): 0 = física, 1 = hashed, 2 = SoC, 3 = encrypted.

Dois detalhes custaram rodadas: o hypercall só funciona chamado do `DllMain` (de
uma thread `ExCreateThread`/`XapiThreadStartup` ele trava o console), e o plugin
entra como `plugin4` por um `launch.ini` no USB, com o do HDD limpo, de modo que
tirar o pendrive recupera o boot.

Cada bloco de 64 KiB do HV tem janela própria,
`0x80000100_00000000 + k*0x200000000`, e o offset usado no endereço é o offset
físico absoluto dentro da imagem. Foi o que explicou o travamento do v19: usar o
bloco 1 com offset do bloco 0 (`0x80000102_00006000`) cai em região não mapeada.
Com os endereços certos (`0x80000102_00015FD0` para a tabela de syscalls,
`0x80000102_00016360` para a save area, `0x80000102_000100F0` para as chaves), o
hypercall lê a RAM do HV a partir do kernel, inclusive os 256 KiB completos.

Resultados (`dumps/console/`):

- Par plaintext/ciphertext real de 4 KiB (`0x3B80..0x4B80`): a região 1 é
  byte-idêntica ao `kernel_17559.bin` e a região 0 no mesmo offset é o
  ciphertext. O teste
  `captured_security_pair_is_real_and_not_reproduced_by_the_placeholder` confirma
  o par e que a transformação não é ECB nem XOR de keystream repetido.
- Os registradores de chave do SECENG em `0x24000` leem `0xffffffff`
  (write-only/mascarados), então a chave não sai por MMIO.
- O bloco OTP (`0x20000`) contém a CPU key, igual à mostrada pelo XeLL.
- A região 3 (encrypted) trava o hypercall, então não é acessível por software.
- O `MmMapIoSpace` do kernel guest devolve NULL para o SoC do security engine; a
  leitura só é possível pelo memcpy do hypervisor.
- A captura v25 trouxe os 256 KiB do HV, o par do mesmo boot e a tabela de chaves
  em `0x80000102_000100F0`, que é zero na imagem retail, no layout `hash`,
  `white`, `aes`. A tabela de syscalls em `0x80000102_00015FD0` tem `0x2540` na
  entrada `0xD` (`HvxPostOutput`), que é o valor que o stage 3 do BadUpdate troca
  por `0x354`.

## Security engine

`crates/xenon-cpu/src/security.rs` implementa o algoritmo do `MemCrypto.py` do
Gooby: AES-128 em modo XEX com tweak Toeplitz/GF(2) derivado de `white_key`, do
endereço de cache line e do valor de whitening de 10 bits, em blocos de 16 bytes
sobre linhas de 128. A saída bate byte a byte com o `MemCrypto.py` para a chave
all-`0x55`, e a tripla lida da RAM do HV decifra o par real de 4 KiB. O runner
liga o modelo por padrão, usando as chaves capturadas quando
`dumps/console/sec_keys_v25.txt` existe e o modo `generic` all-`0x55` caso
contrário.

As chaves de memória são per-boot: três capturas do mesmo offset com plaintext
igual têm ciphertext diferente. O HV mantém a tripla viva na própria RAM
enquanto o console está ligado, que é o que a v25 explora.

O que já foi tentado e não fecha, para não repetir: AES-128/256 com cerca de 60
derivações candidatas (CPU key, SHA-256, MD5, campos do header do HV,
derivações AES, shifts, XORs) e os modos ECB, CBC, CTR, XEX e XTS com setores de
16/512/4096, mais DES/3DES, TEA e XTEA; nenhum bateu. Também não existe chave de
16 bytes contígua no hypervisor. A conclusão é que a cifra é de hardware, com
geração de chave dentro do engine, e não uma derivação de software da CPU key.

Algoritmo, campanha e evidência completos em
[`security_engine_research.md`](security_engine_research.md).

## HID6 (SPR 0x3F9) e tamanho de página

Referência: `arch/powerpc/include/asm/reg.h` do Linux define `SPRN_HID6 = 0x3F9`
(`0x3F6` é HID5), `HID6_LB = 0x0F << 12` (modos de página grande concorrentes) e
`HID6_DLP = 1 << 20` (desliga página grande, só 4 KiB).

- O registrador é guardado em `0x3F9` e exposto ao MMU (`Mmu::hid6`).
- A boot chain real escreve `HID6 = 0x1003800000000`: DLP e LB limpos, então
  página grande segue habilitada e o campo LP do SLB escolhe 64 KiB (LP=0) ou
  16 MiB (LP=1), como já validado.
- `DLP` é honrado: com o bit 20 ligado, o SLB é tratado como 4 KiB tanto na busca
  do software-TLB quanto no page walker SDR1.
- `LB` fica documentado mas não modelado (limpo na imagem atual).
- Testes: `mmu::tests::hid6_dlp_forces_4k_translation`,
  `cpu::tests::hid6_lives_at_spr_3f9_and_is_stored_for_the_mmu` e
  `cpu::tests::hid5_spr_3f6_stays_independent_of_hid6`.

Cadeia completa depois da mudança: CD entrega em `0x100` após 271.049.701
passos e o HV chega a `Handoff { pc: 0x8015D440 }` com 10.937.059 passos e
`HID6=0x1003800000000`, o mesmo estado final de antes.

## Referências

Campos numéricos, não código incorporado: IBM CBE handbook; [xenon-emu,
PPC_MMU.cpp](https://github.com/xenon-emu/xenon/blob/main/Xenon/Core/XCPU/Interpreter/PPC_MMU.cpp)
e `PPC_System.cpp`. Para os offsets do controlador de interrupções,
`Xenon/Core/XCPU/Context/XenonIIC/XenonIIC.cpp` do mesmo projeto, além da
sequência executada do HV em `0xC44..0xCE4`. O formato e o tamanho de páginas
usados aqui continuam experimentais e precisam de confronto com o Xenon real.
