# M1 PPC Interpreter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Interpretador PPC64 big-endian em Rust que executa o CB_A decifrado da flash do dono até o handoff para o CB_B, com log de todo acesso a MMIO.

**Architecture:** Crate `xenon-cpu` com barramento (`Bus`) de RAM mais dispositivos, CPU em modo interpretador (sem JIT), e um binário `runcb` que carrega `dumps/stages/CB_A.bin`. Dispositivos nascem como falhas registradas; cada falha vira um modelo de dispositivo com teste. O conjunto de instruções é guiado por varredura do CB_A, não por cobertura total do PPC.

**Tech Stack:** Rust (cargo já instalado), Python 3 para a varredura estática (`tools/`), capstone para disassembly.

**Spec:** `docs/roadmap.md` (linha M1) e `research/flash/README.md` (estágios decifrados em `dumps/stages/`).

## Global Constraints

- Dados do console (`dumps/`) ficam fora do git; nenhum teste depende deles. Testes usam palavras PPC escritas à mão.
- Big-endian em toda leitura e escrita do barramento. Registradores de 64 bits (PPC64).
- Interpretador primeiro; JIT só depois de o CB_A rodar certo.
- Nada de binários ou chaves da Microsoft no repo.
- Cada instrução implementada tem pelo menos um teste; instrução fora da varredura do CB_A não entra.

## Review Focus

- Opcode desconhecido: `run` devolve `Stop::UnknownOpcode{pc, word}`, nunca panic.
- Acesso fora de qualquer região: `Stop::Unmapped` com endereço, tamanho e direção, nunca panic.
- Acesso que cruza o fim de uma região: erro, não leitura fora do buffer.
- Aritmética de 32 contra 64 bits: `rlwinm`, `extsw` e `cmpw` sobre registrador com bits altos sujos dão o resultado de 32 bits.
- Laço infinito no código emulado: `run` para em `Stop::StepLimit`.

---

### Task 1: Barramento e RAM

**Files:**
- Create: `Cargo.toml` (workspace), `crates/xenon-cpu/Cargo.toml`, `crates/xenon-cpu/src/lib.rs`, `crates/xenon-cpu/src/mem.rs`
- Test: módulo `tests` em `mem.rs`

**Interfaces:**
- Produces:
  - `pub enum BusError { Unmapped { addr: u64, size: usize, write: bool } }`
  - `pub trait Device { fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError>; fn write(&mut self, off: u64, size: usize, val: u64) -> Result<(), BusError>; }`
  - `pub struct Bus` com `new() -> Bus`, `map_ram(&mut self, base: u64, size: usize)`, `map_device(&mut self, base: u64, size: u64, dev: Box<dyn Device>)`, `load(&mut self, addr: u64, bytes: &[u8])`, `read(&mut self, addr: u64, size: usize) -> Result<u64, BusError>`, `write(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError>`.

- [ ] **Step 1: Failing tests**
  - `ram_is_big_endian`: escreve `0x1122334455667788` com size 8 em `0x1000`; ler size 4 em `0x1000` dá `0x11223344`.
  - `unmapped_read_errors`: ler `0x9000` sem mapa dá `Unmapped{addr:0x9000,size:4,write:false}`.
  - `access_crossing_region_end_errors`: RAM de 16 bytes em `0x1000`; ler size 8 em `0x100C` dá `Err`.
  - `device_gets_offset`: dispositivo que guarda o último `off`; leitura em `base+0x10` entrega `0x10`.
- [ ] **Step 2:** `cargo test -p xenon-cpu` falha (tipos não definidos).
- [ ] **Step 3:** Implementar o barramento com lista de regiões ordenada por base; RAM é `Vec<u8>`; o valor é montado byte a byte em big-endian.
- [ ] **Step 4:** `cargo test -p xenon-cpu` passa.
- [ ] **Step 5:** Commit `feat(cpu): bus with ram and mmio devices`.

### Task 2: Varredura estática do CB_A

**Files:**
- Create: `tools/cbscan.py`, `research/cb/README.md`

**Interfaces:**
- Produces: `research/cb/README.md` com (a) histograma de mnemônicos do CB_A, (b) constantes absolutas formadas por `lis` + `ori`/`addi` e o uso de cada uma, (c) endereço de carga provável com a evidência, (d) registradores lidos antes de escritos (estado que a 1BL deixa). Consumido pelas Tasks 3, 5 e 6.

- [x] **Step 1:** `tools/cbscan.py <CB_A.bin>` disassembla de `entry = 0x3D0` em diante (capstone, palavra a palavra, como em `tools/ppcdis.py`) e imprime os itens (a) a (d).
- [x] **Step 2:** Rodar em `dumps/stages/CB_A.bin`. Esperado: histograma não vazio e pelo menos uma constante na faixa do SoC. Se não houver constante, registrar isso no README.
- [x] **Step 3:** Escolher o endereço de carga: o base para o qual todo `bl`/`b` absoluto e todo salto fica dentro da imagem. Registrar a evidência; se mais de um base servir, registrar a ambiguidade e usar o menor.
- [x] **Step 4:** Commit `docs(cb): static scan of CB_A`.

**Revalidação em 2026-10-24:** commit original `1083f09`; scanner executado novamente com Capstone 5.0.7. Histograma completo e evidências em `research/cb/README.md`. Ajuste ao Step 3: os 199 branches diretos são relativos e não determinam uma base; adota-se `0x8000_0200_0001_0000` pela construção de `r2` e leitura do header, em vez do menor valor arbitrário. Estado inicial de `r0`/`r31` permanece hipótese, não captura da 1BL.

### Task 3: Núcleo de inteiros

**Files:**
- Create: `crates/xenon-cpu/src/alu.rs`, `crates/xenon-cpu/src/cpu.rs`
- Modify: `crates/xenon-cpu/src/lib.rs`
- Test: módulos `tests` de `alu.rs` e `cpu.rs`

**Interfaces:**
- Consumes: `Bus` da Task 1; histograma da Task 2.
- Produces:
  - `alu.rs`: `pub fn mask64(mb: u32, me: u32) -> u64` (máscara PPC, com wrap quando `mb > me`), `pub fn rotl32(v: u32, n: u32) -> u32`, `pub fn rotl64(v: u64, n: u32) -> u64`.
  - `cpu.rs`: `pub struct Cpu { pub gpr: [u64; 32], pub pc: u64, pub lr: u64, pub ctr: u64, pub cr: u32, pub xer: u64, pub msr: u64 }`, `pub enum Stop { Unmapped(BusError), UnknownOpcode { pc: u64, word: u32 }, StepLimit, Trap { pc: u64 } }`, `Cpu::new(entry: u64) -> Cpu`, `Cpu::step(&mut self, bus: &mut Bus) -> Result<(), Stop>`, `Cpu::run(&mut self, bus: &mut Bus, max_steps: u64) -> Stop`.

- [ ] **Step 1: Failing tests em `alu.rs`:** `mask64_normal` (`mask64(56,63) == 0xFF`), `mask64_wraps` (`mask64(60,3) == 0xF000_0000_0000_000F`), `rotl32_wraps` (`rotl32(0x8000_0001,1) == 3`).
- [ ] **Step 2: Failing tests em `cpu.rs`** (palavras em RAM `0x1000`, `step`/`run`):
  - `lis_ori_build_constant`: `lis r3,0x8000` dá `0xFFFF_FFFF_8000_0000`; depois `ori r3,r3,0x1234` dá `0xFFFF_FFFF_8000_1234`.
  - `rlwinm_ignores_high_bits`: `gpr[3]=0xDEAD_BEEF_0000_00F0`, `rlwinm r4,r3,0,24,31` dá `0xF0`.
  - `extsw_sign_extends`, `cmpw_uses_low_32_bits` (bits altos diferentes, baixos iguais: CR0 EQ).
  - `bl_sets_lr_and_blr_returns`, `bdnz_loop_ends_with_ctr_zero`.
  - `lwz_stw_roundtrip_big_endian`, `ld_std_roundtrip`.
  - `unknown_opcode_stops`: palavra `0x0000_0000` dá `Stop::UnknownOpcode{pc:0x1000, word:0}`.
  - `step_limit_stops`: `b .` com `run(.., 100)` dá `Stop::StepLimit`.
  - `unmapped_load_stops`.
- [ ] **Step 3:** Implementar `alu.rs`, depois o decode por opcode primário e estendido em `Cpu::step`. Entram só as instruções do histograma da Task 2 mais as dos testes acima. Formas com ponto (`Rc`) atualizam CR0.
- [ ] **Step 4:** `cargo test -p xenon-cpu` passa.
- [ ] **Step 5:** Commit `feat(cpu): integer core driven by CB_A opcode scan`.

### Task 4: SPRs, MSR e modo hypervisor

**Files:**
- Modify: `crates/xenon-cpu/src/cpu.rs`
- Test: módulo `tests` de `cpu.rs`

**Interfaces:**
- Consumes: `Cpu`, `Stop`.
- Produces: `Cpu.spr: std::collections::HashMap<u16, u64>`; `pub const MSR_HV: u64 = 1 << 60`; `mfspr`/`mtspr` para LR, CTR, XER e SPRs de lista aberta; `sc` com `LEV=2` (hypercall) devolve `Stop::Trap{pc}` com o `pc` da instrução.

- [ ] **Step 1: Failing tests:** `mtspr_mfspr_roundtrip_unknown_spr`, `mflr_mtlr`, `msr_hv_bit_is_bit60` (`MSR_HV == 0x1000_0000_0000_0000`), `sc_lev2_traps` (palavra `0x4400_0042`).
- [ ] **Step 2:** Rodar e ver falhar. **Step 3:** Implementar. **Step 4:** Rodar e ver passar.
- [ ] **Step 5:** Commit `feat(cpu): spr, msr hv bit, hypercall trap`.

### Task 5: Carga e execução do CB_A

**Files:**
- Create: `crates/xenon-cpu/src/bin/runcb.rs`, `crates/xenon-cpu/src/map.rs`
- Modify: `crates/xenon-cpu/src/lib.rs`

**Interfaces:**
- Consumes: constantes da Task 2 (`research/cb/README.md`), `Bus::load`, `Cpu::run`.
- Produces: `map.rs` com `pub const CB_A_LOAD: u64`, `pub const SRAM_BASE: u64`, `pub const SRAM_SIZE: usize` e `pub fn initial_regs(cpu: &mut Cpu)` (estado da 1BL, item (d) da Task 2); `runcb <CB_A.bin>` imprime as últimas 32 instruções executadas e o `Stop`.

- [ ] **Step 1: Failing test** `initial_regs_matches_readme` em `map.rs`: chama `initial_regs` e confere cada registrador listado no item (d) do README.
- [ ] **Step 2:** Rodar e ver falhar. **Step 3:** Implementar `map.rs` e `runcb` (SRAM como RAM, resto não mapeado). **Step 4:** Teste passa.
- [ ] **Step 5:** `cargo run -p xenon-cpu --bin runcb -- dumps/stages/CB_A.bin`. Esperado: para em `Stop::Unmapped` com endereço de SoC ou em `Stop::UnknownOpcode`. Colar a saída em `research/cb/README.md`.
- [ ] **Step 6:** Commit `feat(cpu): runcb loads CB_A with 1BL state`.

### Task 6: Laço de dispositivos até o handoff para o CB_B

**Files:**
- Create: `crates/xenon-cpu/src/dev/` (um arquivo por dispositivo descoberto)
- Modify: `crates/xenon-cpu/src/bin/runcb.rs`, `research/cb/README.md`

**Interfaces:**
- Consumes: `Device`, saída do `runcb`.
- Produces: um `Device` por bloco MMIO que o CB_A toca; ao fim, `runcb` imprime `HANDOFF pc=<entry do CB_B> hash_ok=<bool>`.

Cada volta do laço repete até o critério de saída:
- [ ] **Step 1:** Rodar `runcb` e anotar o acesso que parou a execução (endereço, tamanho, leitura ou escrita, `pc`).
- [ ] **Step 2:** Escrever o teste do dispositivo com o valor que o código espera, deduzido do disassembly ao redor do `pc` e registrado no README.
- [ ] **Step 3:** Implementar o `Device` mínimo, registrar em `runcb`, ver o teste passar.
- [ ] **Step 4:** Commit `feat(cpu): <nome> device`.

O controlador de flash do Corona (eMMC) entra quando o CB_A começar a ler o CB_B; ele serve bytes de `dumps/FLASH_1.bin`.

**Critério de saída:** o CB_A decifra o CB_B em memória, o RotSumSha bate com o do cabeçalho do CB_A e o PC chega ao entry do CB_B. Registrar o resultado e atualizar a linha M1 de `docs/roadmap.md`.

---

## Self-Review

- **Cobertura do spec:** o M1 do roadmap pede CPU com modo hypervisor (Task 4) e rodar a cadeia com log (Tasks 5 e 6). O trecho "CB → CD → CE → CF → CG" do roadmap fica **fora**: este plano termina no handoff CB_A → CB_B. CD em diante vira plano próprio.
- **Passos abertos:** a Task 2 e o laço da Task 6 são descobertas, com critério de saída explícito. O resto tem teste nomeado.
- **Tipos:** `Cpu`, `Stop`, `Bus`, `BusError` e `Device` têm a mesma assinatura em todas as tasks.
- **Review Focus:** os cinco itens têm teste nas Tasks 1 e 3.
- **Proporção:** o spec são duas linhas do roadmap e metade do plano é descoberta, não código.
