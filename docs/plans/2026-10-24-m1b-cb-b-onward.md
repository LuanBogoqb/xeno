# M1b — CB_B até o kernel patcheado

**Objetivo:** Fazer o interpretador rodar a cadeia de boot CB_B → CD → CE → CF/CG até o ponto em que o kernel 17559 está descomprimido e patcheado em RAM e o PC chega na primeira instrução que exige MMU ou VMX (início do M2).

**Arquitetura:** Mesmo método do M1: varredura estática do estágio, instruções novas com TDD, dispositivos nascem como falhas registradas e viram modelo com teste, `runcb` roda até o handoff seguinte. O que muda: um runner genérico com hooks de "sem glitch" (hoje hardcoded no `runcb`) e dois oráculos que já existem em `dumps/stages/` — `kernel_base.bin` e `kernel_17559.bin`, produzidos em Python/xenon-bltool — contra os quais comparar a RAM do emulador.

**Stack:** Rust (`crates/xenon-cpu`), Python 3 + capstone 5.0.7 (`tools/cbscan.py`).

**Base:** [`../status.md`](../status.md) (marcos M0–M7), [`../decisions.md`](../decisions.md) (1, 2, 3), [`../../research/flash/README.md`](../../research/flash/README.md) (cadeia, chaves, offsets), [`../../research/cb/README.md`](../../research/cb/README.md) (CB_A, mapa de memória).

## Restrições

- `dumps/` fora do git; nenhum teste automático depende dele. Testes que precisam dos dumps são `#[ignore]` e rodam com `cargo test -- --ignored`; os demais usam palavras PPC escritas à mão.
- Big-endian em todo acesso; GPR de 64 bits.
- Interpretador, sem JIT.
- Sem glitch (decisions.md 3): a verificação de digest/assinatura de cada estágio é pulada por hook, e o hook registra `hash_ok` real. Nada de binários ou chaves da Microsoft no repo.
- Instrução só entra com teste e só se algum estágio a executou (ou a varredura a mostra no caminho).
- Todo hook e todo registrador de SoC modelado cita a evidência (endereço no estágio) em comentário de uma linha ou no README do estágio.
- Commit message termina com `twinko`; sem trailer `Co-Authored-By`.

## Pontos de atenção

- Estágio carregado por DMA/cópia que sobrescreve o código em execução: o runner não pode assumir que o PC continua válido; handoff é detectado por PC, não por contagem de passos.
- Instrução desconhecida em estágio ainda não varrido: `Stop::UnknownOpcode{pc, word}` com o estágio no log, nunca panic.
- Hook de digest forçado em estágio cujo digest bate de verdade: `hash_ok=true` e nada alterado.
- LZX/RC4/SHA rodando no interpretador: orçamento de passos configurável por linha de comando, estouro vira `Stop::StepLimit` com o PC, e a duração do run do CE é medida (meta: < 5 min em release).
- Leitura de registrador de SoC/PCI/DRAM nunca escrito: retorna 0 e entra em `unknown_reads`, e o relatório do run lista cada um uma vez só, em ordem de primeiro acesso.

---

### Tarefa 1: Runner genérico com hooks e alias de modo real

**Arquivos:**
- Cria: `crates/xenon-cpu/src/hooks.rs`, `crates/xenon-cpu/src/runner.rs`
- Altera: `crates/xenon-cpu/src/lib.rs`, `crates/xenon-cpu/src/bin/runcb.rs`, `crates/xenon-cpu/src/map.rs`
- Teste: módulos `tests` em `hooks.rs` e `runner.rs`

**Interfaces:**
- Produz:
  - `pub enum Action { SetGpr { reg: usize, val: u64 }, Log(&'static str) }`
  - `pub struct Hooks` com `new() -> Hooks`, `add(&mut self, pc: u64, a: Action)`, `at(&self, pc: u64) -> &[Action]`
  - `pub enum StageEnd { Handoff { pc: u64 }, Stop(Stop), StepLimit { pc: u64 } }`
  - `pub fn run_until(cpu: &mut Cpu, bus: &mut Bus, hooks: &Hooks, handoff_pc: u64, max_steps: u64) -> (StageEnd, u64)` (segundo valor = passos executados)
  - `runcb` passa a montar `Hooks` com o hook do `0x7F0` relocado do CB_A (hoje inline) e chamar `run_until`; saída idêntica à atual (`HANDOFF pc=0x20003d0 hash_ok=false`, 2.028.852 passos).

- [ ] **Passo 1: Resolver o alvo real do handoff.** Desmontar o CB_A relocado em torno de `0x4A8` (`python tools/cbscan.py` + `tools/ppcdis.py`) e anotar em `research/cb/README.md` o valor de 64 bits exato do salto final. Se for `0x8000_0200_0000_03D0`, `runcb` usa esse endereço e o alias não é preciso. Se for 32 bits (`0x0200_03D0`), registrar a regra de modo real escolhida (tabela: bits do EA → região do `Bus`) em `map.rs` como `pub fn real_addr(ea: u64) -> u64`, aplicada em fetch/load/store quando `MSR.IR/DR = 0`.
- [ ] **Passo 2: Testes que falham**
  - `hooks::tests::at_returns_actions_for_pc_only`: `add(0x100, SetGpr{reg:3,val:0})`; `at(0x100).len()==1`, `at(0x104).is_empty()`.
  - `runner::tests::stops_at_handoff_pc`: programa de 3 `addi` + alvo em `pc+12`; `run_until` devolve `(Handoff{pc}, 3)`.
  - `runner::tests::hook_sets_gpr_before_fetch`: hook em `pc0` zera `r3`; `r3=5` antes; depois do run `r3==0`.
  - `runner::tests::step_limit_reports_pc`: laço `b .` com `max_steps=10` dá `StepLimit{pc}`, 10 passos.
  - `map::tests::real_addr_*` conforme o Passo 1 (se alias for necessário).
- [ ] **Passo 3:** `cargo test -p xenon-cpu` falha (símbolos não definidos).
- [ ] **Passo 4:** Implementar `hooks.rs`, `runner.rs` e migrar `runcb` (o traço das últimas 32 instruções e os dumps `addr:len` continuam). `Hooks::at` é `HashMap<u64, Vec<Action>>`.
- [ ] **Passo 5:** `cargo test -p xenon-cpu` passa; `cargo run -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin` imprime os mesmos `HANDOFF` e contagem de passos de antes.
- [ ] **Passo 6:** Commit `refactor(cpu): generic stage runner with pc hooks`.

### Tarefa 2: Varredura estática do CB_B

**Arquivos:**
- Altera: `tools/cbscan.py` (aceitar `--base` e `--entry`, hoje lê entry em `+0x8`), `research/cb/README.md` (nova seção "CB_B")
- Cria: `research/cb/cb_b.md` só se a seção passar de ~120 linhas

**Interfaces:**
- Consome: `dumps/stages/CB_B.bin` (em claro, `0x7B40` bytes, build 13121) e, para o código de runtime, o CB_B já decifrado na SRAM após o run da Tarefa 1 (dump `0:7B40` do runner).
- Produz: seção no README com (a) histograma de mnemônicos e **lista de mnemônicos que não existem em `cpu.rs`/`alu.rs`** (entrada da Tarefa 3), (b) mapa de MMIO/endereços absolutos formados por `lis`+`ori`/`rldicr` e o papel provável de cada um (SoC, PCI, controlador de DRAM, flash), (c) SPRs tocados, (d) pontos de comparação de digest/assinatura (candidatos a hook) e chamadas ao motor de criptografia do SoC se houver, (e) onde o código lê CD da flash e para onde o copia.

- [ ] **Passo 1:** Rodar `python tools/cbscan.py dumps/stages/CB_B.bin` (com `PYLIB`, como no README do CB_A). Dado que o CB_B não tem o prefixo do CB_A, conferir primeiro que o `entry` do header (`+0x8`) cai dentro de `[0, 0x7B40)`; se não, registrar e usar o `--entry` manual.
- [ ] **Passo 2:** Teste de ferramenta `tools/test_cbscan.py::test_mnemonics_missing_from_cpu`: dado o histograma do CB_A, a função `missing(hist, implemented)` devolve `[]`; dado um histograma com `sc`/`dcbf` e `implemented` sem eles, devolve esses dois. `implemented` vem de `grep` dos `match` do `cpu.rs` (função `implemented_mnemonics(path)` na ferramenta).
- [ ] **Passo 3:** Rodar o teste e confirmar que falha; implementar `missing` e `implemented_mnemonics`; confirmar que passa.
- [ ] **Passo 4:** Escrever a seção do README com os itens (a)–(e). Itens sem evidência ficam marcados "candidato", como no CB_A.
- [ ] **Passo 5:** Commit `docs(cb): static scan of CB_B`.

### Tarefa 3: Instruções que faltam (guiado pela Tarefa 2)

**Arquivos:**
- Altera: `crates/xenon-cpu/src/cpu.rs`, `crates/xenon-cpu/src/alu.rs`
- Teste: módulo `tests` em `cpu.rs`

**Interfaces:**
- Consome: lista (a) da Tarefa 2.
- Produz: cada mnemônico da lista decodificado e executado; mnemônicos de barreira/cache (`sync`, `isync`, `eieio`, `dcbf`, `dcbst`, `dcbz`, `icbi`) viram no-ops (exceto `dcbz`, que zera 32 bytes alinhados no `Bus`); `mfmsr`/`mtmsr`/`mtmsrd`, `sc`, `rfid` só se constarem na lista.

- [ ] **Passo 1:** Para **cada** mnemônico da lista, um teste `op_<mnemonico>` com palavra escrita à mão e asserts de registrador/memória, incluindo o caso de 32 contra 64 bits quando a instrução tem forma de word (bits altos sujos no operando).
- [ ] **Passo 2:** `cargo test -p xenon-cpu op_` falha para todos.
- [ ] **Passo 3:** Implementar até passar, uma instrução por vez, seguindo os arms existentes de `step`.
- [ ] **Passo 4:** `cargo test -p xenon-cpu` inteiro passa (os 29 anteriores continuam verdes).
- [ ] **Passo 5:** Commit `feat(cpu): instructions needed by CB_B`.

### Tarefa 4: Rodar o CB_B até o handoff para o CD

**Arquivos:**
- Cria: `crates/xenon-cpu/src/dev/` novos arquivos conforme a Tarefa 2 (provável: `dram.rs`, `pci.rs`; só os que o trace pedir)
- Altera: `crates/xenon-cpu/src/map.rs`, `crates/xenon-cpu/src/bin/runcb.rs`, `research/cb/README.md` (seção "Execução do CB_B")

**Interfaces:**
- Consome: `run_until` e `Hooks` (Tarefa 1), lista (b), (d), (e) da Tarefa 2.
- Produz: `map.rs` ganha as constantes de endereço dos dispositivos novos e `pub const CD_FLASH_OFFSET: u64 = 0x11A30`; `runcb` ganha o argumento `--stage cb_b` que continua do estado final do CB_A (mesmo `Cpu`/`Bus`, sem recarregar) e termina em `HANDOFF` para o CD com `hash_ok` impresso; `--max-steps N` e `--hook-skip-digest` com o pc do hook por estágio numa tabela em `map.rs`.

- [ ] **Passo 1:** Rodar o CB_B e anotar o primeiro `Stop`. A cada falha (`Unmapped`, `UnknownOpcode`, leitura de registrador não escrito que faz o laço travar) o ciclo é: escrever um teste que reproduz o comportamento mínimo do dispositivo (`reads_<reg>_returns_<valor>` com a evidência no nome ou no comentário), implementar, rodar de novo. Repetir até o handoff.
- [ ] **Passo 2:** Os testes de dispositivo ficam em `#[cfg(test)]` do próprio arquivo e passam em `cargo test -p xenon-cpu`.
- [ ] **Passo 3:** Hook de digest do CD (provável: o mesmo padrão `RotSumSha` do CB_A, função equivalente no CB_B; confirmar pelo (d) da Tarefa 2). O hook força "igual" **só** quando o digest real difere, e `hash_ok` reflete a comparação real.
- [ ] **Passo 4:** Verificação: `cargo run -p xenon-cpu --release --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage cb_b` imprime `HANDOFF pc=<entry do CD> hash_ok=<bool>`; `--dump` do corpo do CD na SRAM/DRAM decifrado bate com `dumps/stages/CD.bin` no teste ignorado `cd_in_ram_matches_stage_file` (`cargo test -- --ignored`).
- [ ] **Passo 5:** Registrar no README: contagem de passos, regs de SoC/PCI/DRAM modelados e quais continuam só "escreve e lê de volta". Commit `feat(cpu): CB_B runs to CD handoff`.

### Tarefa 5: CD (4º estágio) até o handoff para o CE

**Arquivos:**
- Altera: `crates/xenon-cpu/src/map.rs`, `crates/xenon-cpu/src/bin/runcb.rs`, dispositivos conforme o trace, `tools/cbscan.py` (usar `--base` para o CD relocado), `research/cb/README.md` (seção "CD")

**Interfaces:**
- Consome: estado final da Tarefa 4; chave do CD derivada pelo próprio código (`HMAC(CB_B.key, CD.salt)`, ver `research/flash/README.md`).
- Produz: `--stage cd` termina em `HANDOFF` para o CE; `CE` decifrado em RAM.

- [ ] **Passo 1:** Varredura do CD já decifrado (dump da RAM, Tarefa 4 Passo 4) com `cbscan.py`; as instruções novas entram como na Tarefa 3 (um `op_` por mnemônico, falha → passa).
- [ ] **Passo 2:** Mesmo ciclo da Tarefa 4 Passo 1 até o handoff. Hook do digest do CE, mesma regra.
- [ ] **Passo 3:** Verificação: teste ignorado `ce_in_ram_matches_stage_file`: a imagem do CE na RAM (decifrada pelo emulado) é igual a `dumps/stages/CE.bin` nos `0x5606A` bytes.
- [ ] **Passo 4:** Atualizar README, commit `feat(cpu): CD runs to CE handoff`.

### Tarefa 6: CE até `kernel_base` em RAM

**Arquivos:**
- Altera: `map.rs`, `runcb.rs`, dispositivos conforme o trace, `research/cb/README.md` (seção "CE")
- Teste: `crates/xenon-cpu/tests/kernel_base.rs` (`#[ignore]`)

**Interfaces:**
- Consome: estado da Tarefa 5. O CE contém o descompressor LZX (janela 128 KiB, tradução E8) que o Python em `tools/` já reproduz.
- Produz: `--stage ce` termina quando o CE salta para o kernel/HV base; `kernel_base` em RAM com 0x120000 bytes.

- [ ] **Passo 1:** Medir o custo: rodar com `--max-steps 2000000000` em release e anotar passos e tempo. Se passar de 5 min, otimizar o laço quente (decodificação cacheada por palavra) **só então**, com benchmark antes/depois no README.
- [ ] **Passo 2:** Teste ignorado `ce_decompresses_to_kernel_base`: após o run, os `0x120000` bytes na região de destino (endereço registrado no README) são iguais a `dumps/stages/kernel_base.bin`. Falha até o LZX do emulado estar certo.
- [ ] **Passo 3:** Ciclo de falhas da Tarefa 4 Passo 1 até o teste passar. Se divergir, o primeiro offset divergente é o dado de depuração (o bug E8 do Python foi achado assim em `0x121C`).
- [ ] **Passo 4:** Commit `feat(cpu): CE decompresses kernel_base in emulated RAM`.

### Tarefa 7: CF/CG até `kernel_17559` em RAM

**Arquivos:**
- Altera: `map.rs`, `runcb.rs`, dispositivos conforme o trace, `research/cb/README.md` (seção "CF/CG")
- Teste: `crates/xenon-cpu/tests/kernel_17559.rs` (`#[ignore]`)

**Interfaces:**
- Consome: estado da Tarefa 6; `CF` em `0xB0000` e `CG` em `0xB4560` (não contíguo: o bloco `0xC0000` quase todo `0xFF` é pulado — o código da flash decide isso, o emulado deve reproduzir o salto lendo a flash cru).
- Produz: `--stage cf` termina com `kernel_17559` em RAM.

- [ ] **Passo 1:** Descobrir quem carrega o CF/CG (CD? CE? o kernel base?) pela varredura/trace e registrar no README **antes** de implementar; a ordem real pode não ser a da tabela da flash.
- [ ] **Passo 2:** Teste ignorado `cg_patch_yields_kernel_17559`: SHA-1 dos `0x1F0000` bytes na RAM é `f1b66f2aa794f0117a8a2d565c9fec0b601fed83` e os bytes são iguais a `dumps/stages/kernel_17559.bin`.
- [ ] **Passo 3:** Ciclo de falhas até passar. O patch LZX-delta usa janela de 32 KiB pré-preenchida com os bytes antigos; se o emulado divergir, comparar primeiro com a janela do Python.
- [ ] **Passo 4:** Commit `feat(cpu): CF/CG patch yields kernel_17559 in emulated RAM`.

### Tarefa 8: Fechar o M1 e marcar a fronteira do M2

**Arquivos:**
- Altera: `../status.md`, `../../research/cb/README.md`, `../README.md`

**Interfaces:**
- Consome: Tarefas 4–7.
- Produz: status com M1 concluído e uma linha "Fronteira M2" com o PC, a instrução (MMU/SLB/VMX128/`mtmsr` com IR/DR) e o `Stop` em que o runner para depois do kernel patcheado.

- [ ] **Passo 1:** Rodar a cadeia inteira de uma vez (`--stage cb_b` até o fim) e salvar o relatório (passos por estágio, tempo, `hash_ok` por estágio, lista de `unknown_reads`) em `research/cb/chain-run.md`.
- [ ] **Passo 2:** Teste ignorado `full_chain_is_deterministic`: duas execuções seguidas dão o mesmo SHA-1 de RAM no ponto final.
- [ ] **Passo 3:** `cargo test -p xenon-cpu` e `cargo test -p xenon-cpu -- --ignored` passam; atualizar o marco M1 em `../status.md`.
- [ ] **Passo 4:** Commit `docs: close M1, mark M2 boundary`.

---

## Notas de risco (não são tarefas)

- Pode ser que o CB_B use o motor de criptografia/aleatoriedade do SoC ou treine a DRAM com laços que dependem de bits de status; cada um vira um registrador modelado com evidência, e o conjunto pode ser maior que o do CB_A. É o maior desconhecido do plano.
- O handoff do CB_A hoje para em `0x0200_03D0`, que pode ser artefato de leitura de 32 bits do salto; a Tarefa 1 Passo 1 resolve isso antes de qualquer outra coisa.
- Tudo desta cadeia roda em modo real (HV). Se algum estágio ligar a MMU antes do CE terminar, a fronteira do M2 chega mais cedo e a Tarefa 6/7 precisam de TLB mínimo; isso muda o escopo e deve ser decidido com o dono.
