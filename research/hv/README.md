# HV → kernel 17559 (M2 em andamento)

## Ponto reproduzido em 2026-10-25

A cadeia original continua sendo executada até o CD (271.049.701 passos
com a resposta SMC `0x01` real; o valor anterior era 271.049.695).
`--stage hv` continua a partir desse estado vivo: não injeta o kernel offline,
não adiciona hooks de salto e não modifica os fuses.

- Entrada do HV em PC `0x100`, HRMOR `0x10000000000`.
- Inicialização do HV e `rfid` para o kernel virtual em `0x80078DB0`.
- Misses de tradução passam pelos handlers PPC do próprio HV; esses handlers
  escrevem os SPRs de software-TLB e retornam à instrução do kernel.
- Primeiro `sc` do kernel em `0x8007F97C` (`r0=15`) entra no handler real
  em `0xC00`, com SRR0 `0x8007F980`. Há teste de integração desse ponto.
- Execução posterior passa por chamadas do HV, PCI/SMC e Xenos, inicializa
  memória e cria a primeira thread. No modo de seis threads, passa pela
  sincronização multicore e os seis threads chegam ao loop idle guest.
  **Não chega ao dash e isso ainda não conclui a M2.**

```powershell
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-stop-pc c00
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-max-steps 20000000 --hv-stop-pc 8015d440
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 20000000 --hv-stop-pc 8015d44c
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-threads 6 --hv-max-steps 40000000 --log-pc 8015d44c:10
cargo test -p xenon-cpu --release -- --include-ignored
```

`--max-steps` permanece o orçamento de cada estágio CB_B/CD. O orçamento
separado do HV/kernel é `--hv-max-steps` (padrão: 5 milhões); `--hv-stop-pc`
para antes de executar o PC indicado. O log inclui SRR0/1, DAR, DSISR, SLB
e as últimas entradas brutas do software-TLB.

Validação do checkpoint anterior (single-CPU):
passou com **98 testes unitários e 6 integrações com os dumps locais**. As
quatro regressões M1 continuam byte-idênticas; as duas novas cobrem a entrada
do syscall e a chegada à barreira multicore. Isso não valida hardware completo.

Validação atual: `cargo test -p xenon-cpu --release -- --include-ignored`
passou com **111 testes unitários + 8 integrações com os dumps**. As quatro
regressões M1 permanecem byte-idênticas. Os dois testes multicore novos
exigem a passagem guest da barreira e, na execução longa, seis PCs em idle.
`cargo test -p xenon-cpu --lib` também passou com os 111 testes em debug.

## Sincronização multicore: bloqueio anterior resolvido

No modo single-CPU, após 100 milhões de passos o processador permanece em `0x8015D440`.
O código em `0x8015D400` prepara os PCRs dos threads 1..5, chama a rotina
de inicialização em `0x80077050` e aguarda:

```text
8015D440: lwz     r10,0x5748(r11)   ; r11 = 0x801A0000
8015D444: cmplwi  cr6,r10,5
8015D448: bne     cr6,0x8015D43C
```

O primeiro alcance desse PC ocorre em **10.713.316 passos HV/kernel**;
um teste de integração verifica o marco, o contador zero e código do HV
preservado após a inicialização das páginas físicas do kernel.
O contador não chega a 5 com apenas uma CPU executando. A execução longa
não parou por opcode ausente: consumiu o orçamento nessa espera. A continuação
modelou os seis contextos, partida dos secundários, IIC/IPIs e
estado de tradução por thread/PPE, agora disponível com `--hv-threads 6`.

O HV `sc1` escreve os IDs lógicos 4/16 dos PPEs secundários, emite o pacote
IPI `0x140078` e habilita o sibling do PPE0 com `CTRL=0xC00000`.
O modelo entrega reset aos PPEs acordados e aos siblings habilitados; cada
secundário executa o vetor `0x100`, monta seu próprio PCR/SLB e entra em
`KiSystemStartup`. As instruções guest `lwarx/stwcx.` incrementam o contador.
**Nenhum hook escreve o contador ou salta o loop.** A CPU0 passa em
`0x8015D44C`, com r10=5, após **10.725.123 passos totais** no primeiro teste
de seis threads (quantum determinístico de 256 instruções).

Os secundários não executaram a cadeia CB/CD completa: o bootstrap post-CD
começa com eles desabilitados, herdando o HRMOR do boot por PPE. Esse estado
inicial e o reset por wake IPI são inferidos do protocolo guest, não uma
validação completa do power/reset do Xenon físico.

### Ponto atual e próximos requisitos de M2

Depois da barreira, `andis.` e as formas indexadas com update foram
implementadas. Uma execução de 100 milhões
de passos ficou com os seis threads no loop idle `0x8007588C..0x8007589C`,
sem novo opcode ausente. Isso **não prova que todo driver iniciou** nem
que os dispositivos estão funcionais. PCI/SMC continuam incompletos.

`0x80075874` isoladamente não é um marco suficiente: a CPU0 também visita
o idle temporariamente antes da partida multicore. A regressão longa exige
o log de `0x8015D44C` com r10=5 e o PC final dos seis threads no loop idle.

Há timebase por PPE e decrementer por thread, latched e entregue em `0x900`
apenas com EE habilitado. O tempo é determinístico (instruções emuladas),
não 50 MHz calibrados; não há HDEC completo ou fontes periódicas externas.
Não se fabrica IRQ de dispositivo para sair do idle.

Para fechar M2 ainda faltam: cifragem/integridade/isolamento por chave da
security engine; fidelidade restante da MMU (PP0 estendido, decodificação
confirmada do HID6 e composição completa de LPCR/RMOR); evidência de
execução/resultado do XOSC. O page walker de hardware e a invalidação
seletiva de TLB foram implementados. Os oito testes de boot não são testes
de XOSC. Um confronto com vetores do console é necessário antes de declarar
equivalência criptográfica.

O SMC `0x01` já tem a resposta real do console integrada (ver abaixo);
PCI devolve vários registradores não inicializados e não há entrega
completa de interrupções. Esses modelos também continuam pendentes após a
solução da barreira multicore.

### Falha anterior resolvida, não contornada

A primeira versão chegava a uma falta por ponteiro nulo em `0x80073144`
(`KeWaitForSingleObject`, DAR `0xA6`, `PCR+0x100` nulo). O chamador era
`ExAcquireReadWriteLockExclusive`: incrementava um lock `0xFFFFFFFF` e
deveria reconhecer zero em `addic.`. O interpretador comparava o resultado
de 64 bits (`0x100000000`) mesmo no kernel SF=0, causando uma espera
prematura antes de existir thread atual. CR0 e carry/borrow agora respeitam
o tamanho de palavra no modo post-CD. Não houve preenchimento de PCR,
override de resposta SMC ou salto do handler. Há testes de fronteira 32/64.

## Modelo experimental e limites de validação

- `lwarx/ldarx`, `stwcx./stdcx.`, `cntlzd`, operações lógicas de CR e
  aritmética com carry cobrem os novos caminhos executados. Reservas são
  de uma linha de 128 bytes, com invalidação por escrita no backing físico;
  seis threads no modo opt-in, com invalidação das reservas de todos os
  owners por escrita no backing correspondente. Não é um modelo de cache
  coerente/cycle-accurate do hardware.
- SLB/VSID e software-TLB: páginas 4 KiB, 64 KiB e 16 MiB. O modo novo é
  ativado somente após o handoff do CD para não trocar a interpretação dos
  locked caches já validada em M1. A seleção grande LP=0/1 é a observada no
  boot (64 KiB/16 MiB), não uma decodificação confirmada do HID6 Xenon.
- Exceções de storage/segment e `sc` entram no código guest do HV. A tabela
  base PP/Ks/Kp e NX de segmento/página agora geram faltas de proteção reais,
  com testes de stores sem efeito e ISI/DSI. PP0 estendido é rejeitado
  conservadoramente, não implementado. A invalidação agora é seletiva por
  página (`tlbie`/`tlbiel`) e o page walker de hardware (SDR1 hashed PTEG,
  primary/secundário, bits R/C) está implementado e testado. O boot usa TLB
  software-managed, então o walker não é exercitado por ele. Ainda faltam
  PP0 estendido, decodificação confirmada do HID6 e composição completa de
  LPCR/RMOR.
- Regiões security-engine 1 e 3 têm **backing plaintext, não criptografia**.
  Após o CD, a visão hashed recebe um snapshot esparso da DRAM viva; as
  escritas físicas/encrypted deixam de sobrescrever o código plaintext do
  HV. Os key selectors ainda compartilham a mesma visão hashed. Isso modela
  isolamento entre duas visões, não cache seguro real, tags, autenticação,
  isolamento por chave ou cifragem de RAM.
- A região encrypted ainda compartilha plaintext com a DRAM física. Fuses
  permanecem os originais; o único override de digest é o CB_A preexistente.
- **Security engine tem modelo experimental de chaves, cifragem por região e
  tags de integridade (XOR/FNV, não validados) e não está no caminho de dados
  por padrão. O backing padrão continua plaintext; M2 não está concluído.**
- **XOSC ganhou modelo de resposta determinístico dependente da CPU key/nonce,
  mas não é resposta Live válida.**
- **MMU ganhou invalidação seletiva de TLB, page walker SDR1 e registradores
  LPCR/RMOR/HID6; PP0 encodings estendidos (bit 3 do RPN) implementados na
  tabela de proteção HPTE. Decodificação confirmada do HID6 ainda pendente.**
- **M2 não está concluído.** Não houve conexão ao Live.

Captura de vetores para fechar M2:
- `tools/capture/xosc_capture.c`: homebrew XDK que grava `xosc_capture.bin` ao lado do
  `.xex` e consulta o SMC via `HalSendSMCMessage`. O caminho XOSC real exige o
  `.xex` baixado do Live, então o capture ainda usa um placeholder.
- Modo de teste: `runcb ... --stage hv --xosc-test challenge.bin` compara a
  resposta do modelo com `challenge.bin[20..52]`.

Vetores reais capturados do console RGH em 2026-10-25 (ConsoleCapture v2,
`dumps/console/CAP00/`): SMC `0x01` (`01 11 00 0A...`), versão `0x12`
(`12 62 02 05...`), temperatura `0x07`, bandeja `0x0A`, AV `0x0F` e
orientação `0x17`. Esses valores estão no modelo `dev/smc.rs`.

A tentativa de capturar a região 1 (hashed RAM, 0x0100_0000_0000) e a DRAM
física (0x8000_0000_0000_0000) a partir de um título falhou com
`r1_ok=0` e `dram_ok=0` (faults de leitura). Um título de usuário não alcança
esses ranges.

**Captura em kernel mode (2026-10-25).** Um plugin do Dashlaunch
(`tools/capture/seccapture.cpp`) roda em kernel mode e usa o memcpy do freeboot
(`HvxGetVersions`, magic `0x72627472`, op 5) para copiar das regiões reais
(bits 40..43): 0 = física, 1 = hashed, 2 = SoC, 3 = encrypted. Um detalhe
crítico: o hypercall **só funciona chamado do `DllMain`**; de uma thread
`ExCreateThread`/`XapiThreadStartup` ele trava o console. O plugin se chama
`plugin4` por um `launch.ini` no USB (o do HDD fica limpo), então tirar o
pendrive recupera o boot. Isso já foi exercitado e o console foi limpo.

Resultados (`dumps/console/seccap.bin`, vetor em
`dumps/console/sec_pair_0x3b80.bin`):

- **Par plaintext/ciphertext real de 4 KiB** (`0x3B80..0x4B80`): a região 1
  (hashed) é **byte-idêntica** ao `kernel_17559.bin`; a região 0 (física) no
  mesmo offset é o **ciphertext**. O plaintext bate byte a byte com o kernel,
  confirmado pelo teste `captured_security_pair_is_real_and_not_reproduced_by_the_placeholder`.
- A transformação real **não é ECB nem XOR de keystream repetido**: blocos de
  plaintext iguais geram ciphertexts diferentes e o XOR não tem período. É
  modo encadeado/tweakable (tipo XTS/XEX), com chave derivada da CPU key.
- Os registradores de chave do SECENG em `0x24000` lêem `0xffffffff`
  (write-only/mascarados), então a chave não sai por MMIO.
- O bloco OTP (`0x20000`) contém a **CPU key**
  (`5cb2...6f78 (redacted; ver dumps/console/)`), igual à mostrada pelo Xell.
- A região 3 (encrypted) **trava o hypercall**, então não é acessível por esse
  caminho.
- O `MmMapIoSpace` do kernel guest devolve NULL para o SoC do security engine;
  a leitura só é possível pelo memcpy do hypervisor.

**Janelas do HV (2026-10-25, v19/v21/v25).** Cada bloco de 64 KiB do HV tem
janela própria: `0x80000100_00000000 + k*0x200000000`, com o offset físico
absoluto no endereço (syscall table em `0x80000102_00015FD0`, save area em
`0x80000102_00016360`, tabela de chaves em `0x80000102_000100F0`). O v19 travou
por usar bloco 1 com offset de bloco 0 (`0x80000102_00006000`, não mapeado);
com os endereços certos o hypercall **lê a RAM do HV a partir do kernel**,
inclusive os 256 KiB completos. Foi assim que o v25 achou as chaves de DRAM do
boot em `0x80000102_000100F0` e validou a cifra contra o par de hardware
(`dumps/console/sec_keys_v25.txt`). O plugin também corrigiu o `DllMain` que
retornava `FALSE` (derrubava o boot depois da captura), grava o cabeçalho antes
de cada cópia e faz flush por registro.

O `security.rs` continua com primitivas placeholder (XOR/FNV). Implementar a
cifra real exige a cifra (XTS/XEX) + a KDF da CPU key; o vetor acima é o
oráculo para validar essa implementação. M2 continua incompleto.

O `xam.mem` mostra a string `/xedl/chalresp/c%uv%u/xosc.xex` e uma chamada
ao ordinal 410 do kernel (`XexLoadImageFromMemory`). O XOSC não é uma função
criptográfica do HV: é um `.xex` separado, baixado do Live e executado.

Referências de campos numéricos, não código incorporado: IBM CBE handbook;
[xenon-emu, PPC_MMU.cpp](https://github.com/xenon-emu/xenon/blob/main/Xenon/Core/XCPU/Interpreter/PPC_MMU.cpp)
e `PPC_System.cpp`. O formato e o tamanho de páginas usados aqui continuam
experimentais e precisam de confronto com o Xenon real.
Para os offsets numéricos do controlador de interrupções, foi consultado
`Xenon/Core/XCPU/Context/XenonIIC/XenonIIC.cpp` do mesmo projeto, além da
sequência executada do HV em `0xC44..0xCE4`.

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
(`XeCryptRandom(0x10100, 0x30)`), nao derivadas da CPU key. O padrão do
emulador e all-0x55 (equivale a desabilitar `XeCryptRandom`), conforme notas
do Hypervisor Manager. Para validar contra o par real
(`dumps/console/sec_pair_0x3b80.bin`) e necessario patchar o HV para chaves
conhecidas (ex.: Hypervisor Manager) ou capturar as chaves via JTAG. M2
permanece incompleto ate validacao genuina.

Antes desta continuação, os 22 arquivos locais modificados foram copiados e
verificados por SHA-256 em
`C:\Users\Luan Bogo\AppData\Local\Temp\opencode\xb-emu-before-m2-20261025-093643`.
O diretório contém `SHA256SUMS.txt`, o status e o HEAD de origem. Nenhuma
alteração anterior foi descartada, resetada ou incluída em commit automático.

Antes da etapa multicore, os 25 arquivos modificados/novos foram novamente
preservados com hashes em
`C:\Users\Luan Bogo\AppData\Local\Temp\opencode\xb-emu-before-multicore-20261025-101518`.
Os 15 arquivos preexistentes fora do escopo desta continuação foram
comparados com esse checkpoint e permaneceram idênticos.
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
`HID6=0x1003800000000` — mesmo estado final de antes.

