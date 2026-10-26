# HV â†’ kernel 17559 (M2 em andamento)

## Ponto reproduzido em 2026-10-25

A cadeia original continua sendo executada atÃ© o CD (271.049.701 passos
com a resposta SMC `0x01` real; o valor anterior era 271.049.695).
`--stage hv` continua a partir desse estado vivo: nÃ£o injeta o kernel offline,
nÃ£o adiciona hooks de salto e nÃ£o modifica os fuses.

- Entrada do HV em PC `0x100`, HRMOR `0x10000000000`.
- InicializaÃ§Ã£o do HV e `rfid` para o kernel virtual em `0x80078DB0`.
- Misses de traduÃ§Ã£o passam pelos handlers PPC do prÃ³prio HV; esses handlers
  escrevem os SPRs de software-TLB e retornam Ã  instruÃ§Ã£o do kernel.
- Primeiro `sc` do kernel em `0x8007F97C` (`r0=15`) entra no handler real
  em `0xC00`, com SRR0 `0x8007F980`. HÃ¡ teste de integraÃ§Ã£o desse ponto.
- ExecuÃ§Ã£o posterior passa por chamadas do HV, PCI/SMC e Xenos, inicializa
  memÃ³ria e cria a primeira thread. No modo de seis threads, passa pela
  sincronizaÃ§Ã£o multicore e os seis threads chegam ao loop idle guest.
  **NÃ£o chega ao dash e isso ainda nÃ£o conclui a M2.**

```powershell
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-stop-pc c00
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-max-steps 20000000 --hv-stop-pc 8015d440
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 20000000 --hv-stop-pc 8015d44c
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 40000000 --log-pc 8015d44c:10
cargo test -p xenon-cpu --release -- --include-ignored
```

`--max-steps` permanece o orÃ§amento de cada estÃ¡gio CB_B/CD. O orÃ§amento
separado do HV/kernel Ã© `--hv-max-steps` (padrÃ£o: 5 milhÃµes); `--hv-stop-pc`
para antes de executar o PC indicado. O log inclui SRR0/1, DAR, DSISR, SLB
e as Ãºltimas entradas brutas do software-TLB.

ValidaÃ§Ã£o do checkpoint anterior (single-CPU):
passou com **98 testes unitÃ¡rios e 6 integraÃ§Ãµes com os dumps locais**. As
quatro regressÃµes M1 continuam byte-idÃªnticas; as duas novas cobrem a entrada
do syscall e a chegada Ã  barreira multicore. Isso nÃ£o valida hardware completo.

ValidaÃ§Ã£o atual: `cargo test -p xenon-cpu --release -- --include-ignored`
passou com **111 testes unitÃ¡rios + 8 integraÃ§Ãµes com os dumps**. As quatro
regressÃµes M1 permanecem byte-idÃªnticas. Os dois testes multicore novos
exigem a passagem guest da barreira e, na execuÃ§Ã£o longa, seis PCs em idle.
`cargo test -p xenon-cpu --lib` tambÃ©m passou com os 111 testes em debug.

## SincronizaÃ§Ã£o multicore: bloqueio anterior resolvido

No modo single-CPU, apÃ³s 100 milhÃµes de passos o processador permanece em `0x8015D440`.
O cÃ³digo em `0x8015D400` prepara os PCRs dos threads 1..5, chama a rotina
de inicializaÃ§Ã£o em `0x80077050` e aguarda:

```text
8015D440: lwz     r10,0x5748(r11)   ; r11 = 0x801A0000
8015D444: cmplwi  cr6,r10,5
8015D448: bne     cr6,0x8015D43C
```

O primeiro alcance desse PC ocorre em **10.713.316 passos HV/kernel**;
um teste de integraÃ§Ã£o verifica o marco, o contador zero e cÃ³digo do HV
preservado apÃ³s a inicializaÃ§Ã£o das pÃ¡ginas fÃ­sicas do kernel.
O contador nÃ£o chega a 5 com apenas uma CPU executando. A execuÃ§Ã£o longa
nÃ£o parou por opcode ausente: consumiu o orÃ§amento nessa espera. A continuaÃ§Ã£o
modelou os seis contextos, partida dos secundÃ¡rios, IIC/IPIs e
estado de traduÃ§Ã£o por thread/PPE, agora disponÃ­vel com `--hv-threads 6`.

O HV `sc1` escreve os IDs lÃ³gicos 4/16 dos PPEs secundÃ¡rios, emite o pacote
IPI `0x140078` e habilita o sibling do PPE0 com `CTRL=0xC00000`.
O modelo entrega reset aos PPEs acordados e aos siblings habilitados; cada
secundÃ¡rio executa o vetor `0x100`, monta seu prÃ³prio PCR/SLB e entra em
`KiSystemStartup`. As instruÃ§Ãµes guest `lwarx/stwcx.` incrementam o contador.
**Nenhum hook escreve o contador ou salta o loop.** A CPU0 passa em
`0x8015D44C`, com r10=5, apÃ³s **10.725.123 passos totais** no primeiro teste
de seis threads (quantum determinÃ­stico de 256 instruÃ§Ãµes).

Os secundÃ¡rios nÃ£o executaram a cadeia CB/CD completa: o bootstrap post-CD
comeÃ§a com eles desabilitados, herdando o HRMOR do boot por PPE. Esse estado
inicial e o reset por wake IPI sÃ£o inferidos do protocolo guest, nÃ£o uma
validaÃ§Ã£o completa do power/reset do Xenon fÃ­sico.

### Ponto atual e prÃ³ximos requisitos de M2

Depois da barreira, `andis.` e as formas indexadas com update foram
implementadas. Uma execuÃ§Ã£o de 100 milhÃµes
de passos ficou com os seis threads no loop idle `0x8007588C..0x8007589C`,
sem novo opcode ausente. Isso **nÃ£o prova que todo driver iniciou** nem
que os dispositivos estÃ£o funcionais. PCI/SMC continuam incompletos.

`0x80075874` isoladamente nÃ£o Ã© um marco suficiente: a CPU0 tambÃ©m visita
o idle temporariamente antes da partida multicore. A regressÃ£o longa exige
o log de `0x8015D44C` com r10=5 e o PC final dos seis threads no loop idle.

HÃ¡ timebase por PPE e decrementer por thread, latched e entregue em `0x900`
apenas com EE habilitado. O tempo Ã© determinÃ­stico (instruÃ§Ãµes emuladas),
nÃ£o 50 MHz calibrados; nÃ£o hÃ¡ HDEC completo ou fontes periÃ³dicas externas.
NÃ£o se fabrica IRQ de dispositivo para sair do idle.

Para fechar M2 ainda faltam: cifragem/integridade/isolamento por chave da
security engine; fidelidade restante da MMU (PP0 estendido, decodificaÃ§Ã£o
confirmada do HID6 e composiÃ§Ã£o completa de LPCR/RMOR); evidÃªncia de
execuÃ§Ã£o/resultado do XOSC. O page walker de hardware e a invalidaÃ§Ã£o
seletiva de TLB foram implementados. Os oito testes de boot nÃ£o sÃ£o testes
de XOSC. Um confronto com vetores do console Ã© necessÃ¡rio antes de declarar
equivalÃªncia criptogrÃ¡fica.

O SMC `0x01` jÃ¡ tem a resposta real do console integrada (ver abaixo);
PCI devolve vÃ¡rios registradores nÃ£o inicializados e nÃ£o hÃ¡ entrega
completa de interrupÃ§Ãµes. Esses modelos tambÃ©m continuam pendentes apÃ³s a
soluÃ§Ã£o da barreira multicore.

### Falha anterior resolvida, nÃ£o contornada

A primeira versÃ£o chegava a uma falta por ponteiro nulo em `0x80073144`
(`KeWaitForSingleObject`, DAR `0xA6`, `PCR+0x100` nulo). O chamador era
`ExAcquireReadWriteLockExclusive`: incrementava um lock `0xFFFFFFFF` e
deveria reconhecer zero em `addic.`. O interpretador comparava o resultado
de 64 bits (`0x100000000`) mesmo no kernel SF=0, causando uma espera
prematura antes de existir thread atual. CR0 e carry/borrow agora respeitam
o tamanho de palavra no modo post-CD. NÃ£o houve preenchimento de PCR,
override de resposta SMC ou salto do handler. HÃ¡ testes de fronteira 32/64.

## Modelo experimental e limites de validaÃ§Ã£o

- `lwarx/ldarx`, `stwcx./stdcx.`, `cntlzd`, operaÃ§Ãµes lÃ³gicas de CR e
  aritmÃ©tica com carry cobrem os novos caminhos executados. Reservas sÃ£o
  de uma linha de 128 bytes, com invalidaÃ§Ã£o por escrita no backing fÃ­sico;
  seis threads no modo opt-in, com invalidaÃ§Ã£o das reservas de todos os
  owners por escrita no backing correspondente. NÃ£o Ã© um modelo de cache
  coerente/cycle-accurate do hardware.
- SLB/VSID e software-TLB: pÃ¡ginas 4 KiB, 64 KiB e 16 MiB. O modo novo Ã©
  ativado somente apÃ³s o handoff do CD para nÃ£o trocar a interpretaÃ§Ã£o dos
  locked caches jÃ¡ validada em M1. A seleÃ§Ã£o grande LP=0/1 Ã© a observada no
  boot (64 KiB/16 MiB), nÃ£o uma decodificaÃ§Ã£o confirmada do HID6 Xenon.
- ExceÃ§Ãµes de storage/segment e `sc` entram no cÃ³digo guest do HV. A tabela
  base PP/Ks/Kp e NX de segmento/pÃ¡gina agora geram faltas de proteÃ§Ã£o reais,
  com testes de stores sem efeito e ISI/DSI. PP0 estendido Ã© rejeitado
  conservadoramente, nÃ£o implementado. A invalidaÃ§Ã£o agora Ã© seletiva por
  pÃ¡gina (`tlbie`/`tlbiel`) e o page walker de hardware (SDR1 hashed PTEG,
  primary/secundÃ¡rio, bits R/C) estÃ¡ implementado e testado. O boot usa TLB
  software-managed, entÃ£o o walker nÃ£o Ã© exercitado por ele. Ainda faltam
  PP0 estendido, decodificaÃ§Ã£o confirmada do HID6 e composiÃ§Ã£o completa de
  LPCR/RMOR.
- RegiÃµes security-engine 1 e 3 tÃªm **backing plaintext, nÃ£o criptografia**.
  ApÃ³s o CD, a visÃ£o hashed recebe um snapshot esparso da DRAM viva; as
  escritas fÃ­sicas/encrypted deixam de sobrescrever o cÃ³digo plaintext do
  HV. Os key selectors ainda compartilham a mesma visÃ£o hashed. Isso modela
  isolamento entre duas visÃµes, nÃ£o cache seguro real, tags, autenticaÃ§Ã£o,
  isolamento por chave ou cifragem de RAM.
- A regiÃ£o encrypted ainda compartilha plaintext com a DRAM fÃ­sica. Fuses
  permanecem os originais; o Ãºnico override de digest Ã© o CB_A preexistente.
- **Security engine (2026-10-26): cifra XEX real agora esta no caminho de dados
  para as regioes 1/3 quando o engine esta habilitado (leitura/escrita
  transformam on-the-fly sobre a DRAM fisica). Tags de integridade continuam
  XOR/FNV placeholder; PP0 estendido, HID6 e XOSC ainda pendentes. M2 segue
  incompleto.**
- **XOSC (2026-10-26): modelo refinado para usar AES-CBC-MAC do security engine
  sobre nonce/flags/CPU key/fuse-placeholder; ainda nao e resposta Live valida
  (exige xosc.xex capturado).**
  LPCR/RMOR/HID6; PP0 encodings estendidos (bit 3 do RPN) implementados na
  tabela de proteÃ§Ã£o HPTE. DecodificaÃ§Ã£o confirmada do HID6 ainda pendente.**
- **M2 nÃ£o estÃ¡ concluÃ­do.** NÃ£o houve conexÃ£o ao Live.

Captura de vetores para fechar M2:
- `tools/capture/xosc_capture.c`: homebrew XDK que grava `xosc_capture.bin` ao lado do
  `.xex` e consulta o SMC via `HalSendSMCMessage`. O caminho XOSC real exige o
  `.xex` baixado do Live, entÃ£o o capture ainda usa um placeholder.
- Modo de teste: `runcb ... --stage hv --xosc-test challenge.bin` compara a
  resposta do modelo com `challenge.bin[20..52]`.

Vetores reais capturados do console RGH em 2026-10-25 (ConsoleCapture v2,
`dumps/console/CAP00/`): SMC `0x01` (`01 11 00 0A...`), versÃ£o `0x12`
(`12 62 02 05...`), temperatura `0x07`, bandeja `0x0A`, AV `0x0F` e
orientaÃ§Ã£o `0x17`. Esses valores estÃ£o no modelo `dev/smc.rs`.

A tentativa de capturar a regiÃ£o 1 (hashed RAM, 0x0100_0000_0000) e a DRAM
fÃ­sica (0x8000_0000_0000_0000) a partir de um tÃ­tulo falhou com
`r1_ok=0` e `dram_ok=0` (faults de leitura). Um tÃ­tulo de usuÃ¡rio nÃ£o alcanÃ§a
esses ranges.

**Captura em kernel mode (2026-10-25).** Um plugin do Dashlaunch
(`tools/capture/seccapture.cpp`) roda em kernel mode e usa o memcpy do freeboot
(`HvxGetVersions`, magic `0x72627472`, op 5) para copiar das regiÃµes reais
(bits 40..43): 0 = fÃ­sica, 1 = hashed, 2 = SoC, 3 = encrypted. Um detalhe
crÃ­tico: o hypercall **sÃ³ funciona chamado do `DllMain`**; de uma thread
`ExCreateThread`/`XapiThreadStartup` ele trava o console. O plugin se chama
`plugin4` por um `launch.ini` no USB (o do HDD fica limpo), entÃ£o tirar o
pendrive recupera o boot. Isso jÃ¡ foi exercitado e o console foi limpo.

Resultados (`dumps/console/seccap.bin`, vetor em
`dumps/console/sec_pair_0x3b80.bin`):

- **Par plaintext/ciphertext real de 4 KiB** (`0x3B80..0x4B80`): a regiÃ£o 1
  (hashed) Ã© **byte-idÃªntica** ao `kernel_17559.bin`; a regiÃ£o 0 (fÃ­sica) no
  mesmo offset Ã© o **ciphertext**. O plaintext bate byte a byte com o kernel,
  confirmado pelo teste `captured_security_pair_is_real_and_not_reproduced_by_the_placeholder`.
- A transformaÃ§Ã£o real **nÃ£o Ã© ECB nem XOR de keystream repetido**: blocos de
  plaintext iguais geram ciphertexts diferentes e o XOR nÃ£o tem perÃ­odo. Ã‰
  modo encadeado/tweakable (tipo XTS/XEX), com chave derivada da CPU key.
- Os registradores de chave do SECENG em `0x24000` lÃªem `0xffffffff`
  (write-only/mascarados), entÃ£o a chave nÃ£o sai por MMIO.
- O bloco OTP (`0x20000`) contÃ©m a **CPU key**
  (`5cb2...6f78 (redacted; ver dumps/console/)`), igual Ã  mostrada pelo Xell.
- A regiÃ£o 3 (encrypted) **trava o hypercall**, entÃ£o nÃ£o Ã© acessÃ­vel por esse
  caminho.
- O `MmMapIoSpace` do kernel guest devolve NULL para o SoC do security engine;
  a leitura sÃ³ Ã© possÃ­vel pelo memcpy do hypervisor.

**Janelas do HV (2026-10-25, v19/v21/v25).** Cada bloco de 64 KiB do HV tem
janela prÃ³pria: `0x80000100_00000000 + k*0x200000000`, com o offset fÃ­sico
absoluto no endereÃ§o (syscall table em `0x80000102_00015FD0`, save area em
`0x80000102_00016360`, tabela de chaves em `0x80000102_000100F0`). O v19 travou
por usar bloco 1 com offset de bloco 0 (`0x80000102_00006000`, nÃ£o mapeado);
com os endereÃ§os certos o hypercall **lÃª a RAM do HV a partir do kernel**,
inclusive os 256 KiB completos. Foi assim que o v25 achou as chaves de DRAM do
boot em `0x80000102_000100F0` e validou a cifra contra o par de hardware
(`dumps/console/sec_keys_v25.txt`). O plugin tambÃ©m corrigiu o `DllMain` que
retornava `FALSE` (derrubava o boot depois da captura), grava o cabeÃ§alho antes
de cada cÃ³pia e faz flush por registro.

O `security.rs` continua com primitivas placeholder (XOR/FNV). Implementar a
cifra real exige a cifra (XTS/XEX) + a KDF da CPU key; o vetor acima Ã© o
orÃ¡culo para validar essa implementaÃ§Ã£o. M2 continua incompleto.

O `xam.mem` mostra a string `/xedl/chalresp/c%uv%u/xosc.xex` e uma chamada
ao ordinal 410 do kernel (`XexLoadImageFromMemory`). O XOSC nÃ£o Ã© uma funÃ§Ã£o
criptogrÃ¡fica do HV: Ã© um `.xex` separado, baixado do Live e executado.

ReferÃªncias de campos numÃ©ricos, nÃ£o cÃ³digo incorporado: IBM CBE handbook;
[xenon-emu, PPC_MMU.cpp](https://github.com/xenon-emu/xenon/blob/main/Xenon/Core/XCPU/Interpreter/PPC_MMU.cpp)
e `PPC_System.cpp`. O formato e o tamanho de pÃ¡ginas usados aqui continuam
experimentais e precisam de confronto com o Xenon real.
Para os offsets numÃ©ricos do controlador de interrupÃ§Ãµes, foi consultado
`Xenon/Core/XCPU/Context/XenonIIC/XenonIIC.cpp` do mesmo projeto, alÃ©m da
sequÃªncia executada do HV em `0xC44..0xCE4`.

## Security engine: algoritmo implementado (2026-10-26)

O modelo experimental de `crates/xenon-cpu/src/security.rs` foi substituido pela
implementacao real do algoritmo de Gooby (`MemCrypto.py`): AES-128 em modo XEX
(XOR-tweak) com tweak Toeplitz/GF(2) derivado de `white_key`, endereco de
cache line e valor de whitening de 10 bits. Bloco de 16 bytes, unidade de
transformacao de 128 bytes (cache line).

Verificado byte-a-byte contra a saida de `MemCrypto.py` para chave all-0x55
(tweak e bloco cifrado em 0x3B80 coincidem). 8 testes de security passam;
1 teste ignorado (`captured_pair_decryption_with_all_55`) valida apenas se o HV
for modificado para usar chaves all-0x55.

Chaves (`white_key`, `aes_key`, `hash_key`) sao per-boot RNG no hardware real
(`XeCryptRandom(0x10100, 0x30)`), nao derivadas da CPU key. O padrÃ£o do
emulador e all-0x55 (equivale a desabilitar `XeCryptRandom`), conforme notas
do Hypervisor Manager. Para validar contra o par real
(`dumps/console/sec_pair_0x3b80.bin`) e necessario patchar o HV para chaves
conhecidas (ex.: Hypervisor Manager) ou capturar as chaves via JTAG. M2
permanece incompleto ate validacao genuina.

Antes desta continuaÃ§Ã£o, os 22 arquivos locais modificados foram copiados e
verificados por SHA-256 em
`C:\Users\Luan Bogo\AppData\Local\Temp\opencode\xb-emu-before-m2-20261025-093643`.
O diretÃ³rio contÃ©m `SHA256SUMS.txt`, o status e o HEAD de origem. Nenhuma
alteraÃ§Ã£o anterior foi descartada, resetada ou incluÃ­da em commit automÃ¡tico.

Antes da etapa multicore, os 25 arquivos modificados/novos foram novamente
preservados com hashes em
`C:\Users\Luan Bogo\AppData\Local\Temp\opencode\xb-emu-before-multicore-20261025-101518`.
Os 15 arquivos preexistentes fora do escopo desta continuaÃ§Ã£o foram
comparados com esse checkpoint e permaneceram idÃªnticos.
## Tentativas de cryptanalysis (2026-10-25, decifracao)

Par real capturado: dumps/console/sec_pair_0x3b80.bin (4 KiB, regiao 1 = plaintext, regiao 0 = ciphertext).
Hypervisor completo capturado: dumps/console/hypervisor_17559.bin (64 KiB, regiao 1).
CPU key: 5cb2...6f78 (redacted; ver dumps/console/) (offset 0x20 do hypervisor).

Testamos AES-128/256 com ~60 KDFs (CPU key, SHA-256, MD5, header values do
hypervisor, AES-derivacoes, shifts, XORs) e modos (ECB, CBC, CTR, XEX, XTS com
sector sizes 16/512/4096) mais DES/3DES, TEA, XTEA. Buscamos chave de 16 bytes
no hypervisor (64 KB), kernel dump, plaintext e ciphertext. Nenhum bateu.

A chave de memoria nao esta no hypervisor como 16 bytes contiguos. A cifra e
provavelmente de hardware (security engine/SoC) com KDF nao trivial.

Regiao 3 (encrypted) trava o hypercall mesmo em 1 byte -> inacessivel por software.

Disassembly do hypervisor delegado a sub-agente (procurando funcao de
configuracao do security engine, MMIO, IIC, referencias a CPU key).

### Resultado da analise do hypervisor (sub-agente)

A analise confirmou que a criptografia e de HARDWARE (security engine), nao
software. O hypervisor configura via rotina 0x84e0 (save/restore de 5
registradores 0x6000..0x6040 do security engine, MMIO em
0x8000020000050000). Nao ha AES S-box, TEA constants (0x9e3779b9) ou qualquer
cifra em software no HV. A CPU key e consumida pelo 0xe14 (copy) e pelo setup
do security engine.

Conclusao: a cifra e implementada no hardware (security engine/SoC) com KDF
provavelmente nao trivial. A chave de memoria e derivada da CPU key pelo hardware.

Proximo passo para fechar M2: implementar o security engine como dispositivo
MMIO (SoC) no emulador, e identificar a cifra por cryptanalysis com mais pares
(plaintext/ciphertext) capturados em diferentes offsets do kernel.

> Correcao (2026-10-25): a hipotese acima de "chave derivada da CPU key" foi
> descartada. As chaves de memoria mudam a cada boot (RNG do proprio engine) e
> nao sao recuperaveis por software; ver `security_engine_research.md` secao 8.

## HID6 (SPR 0x3F9) e tamanho de pagina

Referencia: `arch/powerpc/include/asm/reg.h` do Linux define `SPRN_HID6 = 0x3F9`
(0x3F6 e HID5), `HID6_LB = 0x0F << 12` ("concurrent large page modes") e
`HID6_DLP = 1 << 20` ("disable all large page modes, 4 KiB only").

- O registrador passou a ser guardado em `0x3F9` (antes `0x3F6`, que e HID5) e
  exposto ao MMU (`Mmu::hid6`).
- A boot chain real escreve `HID6 = 0x1003800000000`: DLP e LB limpos, portanto
  paginas grandes seguem habilitadas e o campo LP do SLB continua escolhendo
  64 KiB (LP=0) / 16 MiB (LP=1), como ja validado.
- `DLP` agora e honrado: com o bit 20 ligado, o SLB e tratado como 4 KiB tanto
  na busca do software-TLB quanto no page walker SDR1.
- `LB` fica documentado mas nao modelado (limpo na imagem atual).
- Testes: `mmu::tests::hid6_dlp_forces_4k_translation`,
  `cpu::tests::hid6_lives_at_spr_3f9_and_is_stored_for_the_mmu` e
  `cpu::tests::hid5_spr_3f6_stays_independent_of_hid6`.

Cadeia completa depois da mudanca: CD entrega em `0x100` apos 271.049.701
passos; HV chega a `HV end: Handoff { pc: 0x8015D440 }` com 10.937.059 passos e
`HID6=0x1003800000000` â€” mesmo estado final de antes.

