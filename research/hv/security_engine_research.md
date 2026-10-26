# Security engine do Xbox 360: cifra de memória

## Resumo

- **Cifra:** AES-128 (Rijndael, 10 rodadas, bloco e chave de 128 bits). Confirmada
  pelas notas do CB/bootloader ("AES128 for crypto") e pela replicação de
  engenharia reversa do GoobyCorp (`MemCrypto.py`), que usa `AES(key, ECB)` como
  cifra de bloco. Não aparece S-box de AES nem constante de TEA no HV porque a
  criptografia é delegada ao security engine no die do SoC.
- **Modo:** XEX (XOR-Encrypt-XOR) por bloco de 16 bytes, modo de bloco com tweak:
  `C = AES_K(P ⊕ T) ⊕ T` (inverso `P = AES_K⁻¹(C ⊕ T) ⊕ T`). É a construção
  Liskov–Rivest–Wagner, não XTS (sem ciphertext stealing, sem segunda chave) e
  não CBC/CTR puro. O tweak `T` é por bloco e derivado do endereço da cache line
  mais o valor de whitening de 10 bits, que é o motivo de blocos de plaintext
  iguais em endereços diferentes gerarem ciphertext diferente.
- **Bloco:** 16 bytes (bloco de AES). A unidade de cifra é a **cache line de 128
  bytes** (8 sub-blocos); o tweak varia por posição de sub-bloco e por cache line.
- **Integridade:** o caminho protegido (P=1) usa a mesma transformação XEX mais um
  **CRC de 16 bits por cache line de 128 bytes**, guardado em SRAM da CPU (64
  slots × 512 entradas). O caminho cifrado (P=3) usa só XEX.
- **Chaves de memória:** não são KDF de software a partir da CPU key. `white_key`,
  `aes_key` e `hash_key` são **valores aleatórios por boot, gerados pelo RNG do
  die**, semeados no boot por um hash do conjunto de fuses, pelo timebase e pelo
  RNG de hardware. Nunca são expostos por MMIO (os registradores de chave em
  `+0x24000` do SoC leem `0xffffffff`), mas o hypervisor mantém uma cópia viva na
  própria RAM, que é o que a seção 9 captura.
- **Derivação do tweak:** função linear de Toeplitz/GF(2) (as `get_tweak_0` e
  `get_tweak_1` do Gooby, com `GF2_TAB`) combinando `white_key` (128 bits), o
  endereço da cache line deslocado à direita em 4 e o valor de whitening de 10
  bits. Bate com a patente US7734926B2, que especifica o circuito de derivação de
  chave "preferably a Toeplitz function" com bits de endereço conduzindo a cifra.
- **Confiança:** alta para AES-128 + XEX + tweak derivado de endereço + unidade
  de 128 bytes (várias fontes independentes: notas do CB, patentes, replicação do
  Gooby, par capturado). Média para o polinômio exato de Toeplitz e o
  empacotamento dos 10 bits de whitening. Baixa para qualquer afirmação de que a
  CPU key alimenta a chave de memória; ela não alimenta.

## 1. Cifra candidata

AES-128 (Rijndael, chave e bloco de 128 bits, 10 rodadas). Evidência:

- Reset Glitch Hack / Free60: o CB "is using AES128 for crypto".
- GoobyCorp `Xbox-360-Crypto/MemCrypto.py`: `Cipher(AES(akey), ECB())` com chaves
  de 16 bytes (`white_key`, `aes_key`, `hash_key`), bloco de 16.
- Xenia implementa `XeCryptAesEcb/Cbc` como AES-128 para XEX e keyvault,
  consistente com o modelo de offload para o SoC.
- O HV não contém constante de AES nem de TEA porque a cifra é delegada ao
  security engine (ver `hypervisor_analysis.md` neste diretório).

## 2. KDF (CPU key → chave de cifra de memória)

Não existe KDF direto da CPU key para a chave de cifra de memória. As chaves do
caminho de DRAM são geradas no boot pelo engine e mudam a cada boot, o que dá
proteção contra replay. Também não ficam confinadas ao die: enquanto o console
está ligado, o hypervisor mantém a tripla viva na própria RAM em
`0x80000102_000100F0`, legível a partir do kernel com o helper de cópia do
hypervisor (seção 9). Essa tripla decifra um par real de 4 KiB byte a byte, então
o modelo AES-128 XEX + Toeplitz está validado contra o silício.

A CPU key (`5cb2...6f78 (redacted; ver dumps/console/)`, das linhas 3/4/5/6 do
OTP) serve para:

- decifrar o keyvault (RC4 baseado em HMAC-SHA1, `XeCryptKeyVaultDecrypt`);
- derivar as chaves de software (XEX2, autenticação de DVD, system link,
  roamable) por HMAC-SHA1 sobre a "integrity violation signature" lida de
  `0x80000200_000250B8` (ver `key_derivation.md`, do x360-research).

A própria integrity violation signature é produzida pelo SoC e lida com `ld` de
`0x80000200_000250B8` (CRC32 esperado `0x4B17A409`, começa em `81300D...`). O
CRC32 muda apenas em CPUs com fuse zerado.

O empacotamento de chaves do security engine usa `pack_sec_eng_keys` (fluxo XOR
com ciclo de SHA1) e cifra RSA com a chave pública da Microsoft (ver
`MemCrypto.calc_key_blob`). É assim que as chaves são seladas no keyvault/NAND, e
não é assim que as chaves de DRAM são derivadas.

## 3. Bloco e modo

- **Bloco de AES:** 16 bytes. **Unidade de transformação:** cache line de 128
  bytes, com o tweak variando por sub-bloco de 16 bytes.
- **Modo:** XEX. As `encrypt_block`/`decrypt_block` do Gooby fazem
  `tweak = get_tweak_0(address)` e depois
  `enc = AESdec(plaintext ⊕ tweak) ⊕ tweak`, com o inverso na decifra.
- **Entradas do tweak:** `white_key` por boot (128 bits), endereço da cache line
  (endereço de modo real, com os bits de caminho P), valor de whitening de 10 bits
  vindo da page table e o número do slot (os 6 bits altos do whitening selecionam
  o slot do CRC).
- **Por que não é ECB:** blocos de plaintext iguais em endereços diferentes (ou
  com whitening diferente) geram tweaks diferentes, portanto ciphertext
  diferente. O par capturado (`dumps/console/sec_pair_0x3b80.bin`, 4 KiB em
  `0x3B80..0x4B80`) confirma que blocos repetidos não colapsam em ciphertext
  igual e que a diferença XOR não tem período, o que descarta keystream repetido e
  ECB puro.
- **Caminho protegido (P=1):** mesma transformação XEX mais um CRC de 16 bits por
  cache line sobre ciphertext, plaintext e `hash_key`, resistente a colisão só de
  ciphertext. Os CRCs ficam na SRAM da CPU (64 slots).
- **Caminho cifrado (P=3):** só XEX, sem CRC; exige RMCI (LPCR bit 62) desligado.

## 4. Registradores do security engine

- A MMIO do SoC é acessada pelo caminho 2 (P=2), base `0x80000200_00000000`
  (visão de 32 bits `0x20000000`).
- Integrity violation signature (usada como chave HMAC pelo HV): MMIO
  `0x80000200_000250B8` (32 bits `0x200250B8`), lida só com `ld`.
- Os registradores de chave em `+0x24000` do SoC leem `0xffffffff` a partir do
  kernel: são write-only ou mascarados, então a chave não é recuperável por MMIO.
- O engine também expõe 768 bits de eFuses (chaves e revogação por console), 32
  KB de boot ROM, 64 KB de SRAM e um RNG no die.

## 5. Patentes, artigos e emuladores

- Patentes da Microsoft (depositadas por volta de 2004 e 2005):
  - US7734926B2, "System and method for applying security to memory reads and
    writes" (Dinarte R. Morais). Bits de endereço conduzem a cifra por um
    security engine no die, e o circuito de derivação de chave é "preferably a
    Toeplitz function". Bate direto com o desenho do Xbox 360.
  - US7822993B2, "System and method for using address bits to affect encryption".
  - US9141558B2, "Secure memory control parameters in table look aside buffer
    data fields and support memory array".
  - US7356668B2, "System and method for using address bits to form an index into
    secure memory".
- Literatura de fail0verflow e RGH: o CB inicializa o security engine; AES-128 com
  hashing derivado de Toeplitz; chaves semeadas por hash de fuse, timebase e RNG.
  O RGH burla os bootloaders, não a cifra de memória.
- Free60 wiki e a documentação da CPU Xenon: 4 caminhos (0 desprotegido, 1
  protegido, 2 SoC, 3 cifrado), cache lines de 128 bytes, whitening de 10 bits, 64
  slots de CRC na SRAM.
- icode4.coffee, "Hacking the Xbox 360 Hypervisor" (2025): caminhos da MMU,
  HRMOR, RMCI (LPCR bit 62), tabelas de whitening de página e armazenamento de
  CRC em detalhe.
- GoobyCorp `Xbox-360-Crypto` (`MemCrypto.py`): a única replicação pública em
  software do algoritmo de cifra de memória (AES-128 com tweak XOR, tweak
  Toeplitz, CRC e empacotamento de key blob). É a referência de trabalho do
  emulador.
- Xenia e xenon-emu não emulam o security engine para a memória do HV; usam
  backing plaintext. Xenia implementa XEX AES-128 CBC/ECB só para carregar
  executável.
- O XTS-AES da Intel (IEEE 1619) é o análogo padrão mais próximo e é o que
  sistemas posteriores usam, mas a construção do 360 é a forma XEX/LRW mais
  antiga, com tweak Toeplitz próprio: sem ciphertext stealing, e o tweak vem do
  endereço em vez de avançar por bloco.

## 6. Oráculo capturado

- `dumps/console/sec_pair_0x3b80.bin` (formato `SECD`, offset `0x3B80`, tamanho
  4096): a região 1 (hashed) é byte-idêntica ao `kernel_17559.bin` nos mesmos
  offsets e a região 0 é o ciphertext. O teste
  `captured_security_pair_is_real_and_not_reproduced_by_the_placeholder` verifica
  a igualdade e que a transformação não é ECB nem XOR repetido. É o oráculo que
  qualquer implementação precisa reproduzir.
- `dumps/console/seccap.bin` (66.320 bytes) guarda as capturas anteriores
  (regiões 0/1/2; a região 3 trava o hypercall).
- CPU key capturada no bloco OTP `0x20000`: `5cb2...6f78 (redacted; ver
  dumps/console/)`.

## 7. Questões abertas

1. Recuperar o polinômio exato de Toeplitz e a posição dos 10 bits de whitening
   por criptoanálise contra o par capturado. Com `white_key`/`aes_key` conhecidas
   para um boot (seção 9) isso vira verificação, não busca: as incógnitas
   estruturais que sobram são o polinômio e o empacotamento dos bits.
2. Determinar se o núcleo de AES roda na direção "normal" ou "invertida" para
   cifrar. O Gooby usa o decifrador para cifrar e o cifrador para decifrar, o que
   é uma particularidade de key schedule a confirmar contra o silício.
3. Mapear o CRC de 16 bits do caminho protegido (quais bits de ciphertext,
   plaintext e `hash_key` entram) para reproduzir os checksums da SRAM.
4. Confirmar se a região 3 (caminho cifrado) pode ser capturada sem travar o
   hypercall, para obter um segundo oráculo independente.

## 8. Campanha de captura

Objetivo: obter as chaves de memória reais do console (RGH Corona 4GB, plugin de
kernel do Dashlaunch) para validar a cifra contra o hardware. Fechada no v25
(seção 9); o histórico abaixo fica como registro do processo e das hipóteses que
falharam.

Capturas (`seccapture.cpp`, versões v14 a v21) e o que cada uma provou:

| versão | alvo (via `HvxMemcpy`) | resultado |
|---|---|---|
| v14 | região 2 em `0x6000/0x10000/0x16000/0x60000` | dados gerais, nenhuma chave |
| v15 | região 2 `0x61000` (bloco da `0x84e0`) | hypercall trava antes de gravar |
| v16 | região 1 `0x40000` | fora do mapeamento, trava |
| v17 | região 1 `0x16000` (save area `r2+0x16360`) | RAM do HV é protegida, trava |
| v18 | região 2 `0x56000` + `0x611C0` | lê, mas são registradores de controle |
| v19 | janela do HV `0x80000102_00006000` (save area) | trava, mas grava o par antes |
| v21 | só endereços comprovados + `return TRUE` | captura completa, boot estável |

O detalhe da v18 (`dumps/console/seccap_v18_0x56000.bin`):

```
region2 0x56000: 0000000000002000 ffffffffffffffff
                 0000000000000000 ffffffffffffffff
                 0000000000000000 ffffffffffffffff
                 000000000001056c ffffffffffffffff
region2 0x611C0: zero
```

O engine latcha as chaves internamente. A rotina `0x84e0` salva os cinco qwords
de `0x6000..0x6040` (base `0x50000`, então o offset é `0x56000`) e o qword de
`0x611C8` na área do HV em `r2+0x16360`, zera os registradores e restaura depois.
No estado estável eles devolvem bits de controle (`0x2000`, `0x1056C`) e
`0`/`0xFF`, nunca os 48 bytes de chave.

O par plaintext/ciphertext era capturado a cada boot (`0x3B80`, regiões 1/0), mas
como as chaves são por boot o par sozinho não validava a cifra.

### 8.1 Endereçamento do HV e o teste v19/v21

A questão de endereçamento foi fechada ao descobrir que cada segmento de 64 KiB
do HV tem a sua própria janela, `0x80000100_00000000 + k*0x200000000` (segmento 1
= `0x80000102_...`, segmento 2 = `0x80000104_...`). A tabela de syscalls fica em
`0x80000102_00015FD0` e a save area das chaves em `0x80000102_00016360`
(`r2 = 0x8000010200000000`). Isso veio do `4thStage.asm` do BadUpdate, não de
chute.

O v19 usou esses endereços e o resultado foi conclusivo:

- `dumps/console/seccap_v19b.bin` (8.336 B, v19): o par de 4 KiB foi gravado
  **inteiro** e o arquivo termina exatamente antes da primeira leitura do segmento
  1. O hypercall trava em `0x80000102_00006000`.
- O "crash" que aparecia depois da captura não era do HV: o plugin retornava
  `FALSE` no `DllMain`, o que o Dashlaunch entende como "falha ao carregar o
  plugin" e derruba o boot. Isso mascarou o resultado do v19 por várias rodadas.

O travamento não era proteção do HV, era endereço inválido. A regra de mapeamento
é por bloco de 64 KiB: o bloco `k` mora em `0x80000100_00000000 + k*0x200000000`
e o offset usado no endereço é o offset físico absoluto dentro da imagem. Assim,
`0x6000` pertence ao bloco 0 (`0x80000100_00006000`), não ao bloco 1, e
`0x80000102_00006000` (bloco 1 com offset do bloco 0) simplesmente não existe.
Com os endereços certos (`0x80000102_00015F00`, `0x80000102_00016360`, ...) o
hypercall lê a janela decifrada do HV a partir do kernel, incluindo os 256 KiB
completos.

O plugin v21 é o que funcionou: sem leituras impossíveis, `return TRUE`, header
gravado antes de cada cópia e `FlushFileBuffers` a cada registro, de modo que um
arquivo parcial sempre aponta o endereço exato que falhou.

## 9. Chaves de DRAM na RAM do HV

Com o mapeamento correto, a v25 dumpou os 256 KiB do HV **e** o par
plaintext/ciphertext do mesmo boot (`dumps/console/seccap_v25.bin`, 268.416 B).
Uma varredura de todas as janelas de 48 bytes do dump, testando as seis
atribuições `white`/`aes`/`hash`, achou a tabela de chaves em runtime em
`0x80000102_000100F0` (que é zero na imagem retail):

```
0x100F0  hash_key    23fb...2d2f
0x10100  white_key   a06d...9855
0x10110  aes_key     d611...552d
```

Os valores completos ficam em `dumps/console/sec_keys_v25.txt`, fora do git.

Decifrar o ciphertext de hardware (4 KiB, 256 blocos, `0x3B80..0x4B80`) com
AES-128 XEX + tweak Toeplitz e essas chaves reproduz o plaintext capturado **byte
a byte** (que já era byte-idêntico ao `kernel_17559.bin`):

```
cargo test -p xenon-cpu --test chain hv_dump_key_search -- --ignored
```

O teste não conhece as chaves: varre o dump, testa só o primeiro bloco por
candidato e só decifra os 4 KiB completos no acerto (0,3 s). Evidência e layout
em `dumps/console/sec_keys_v25.txt`.

Isso corrige a seção 2 em um ponto específico: as chaves não ficam confinadas ao
die do engine, e o HV mantém uma cópia viva em RAM enquanto o console está
ligado. Continuam sendo por boot (três capturas do mesmo offset com plaintext
igual têm ciphertext diferente), então a captura tem de ser do mesmo boot, que é
o que a v25 faz.

Caminhos tentados antes, mantidos para não se repetir:

1. **Patch do HV com chaves all-`0x55`**: exige a boot chain decifrada e
   reassinada. Bloqueios: o toolkit do Gooby pede chaves privadas BYOK
   (`Keys/SB_prv.bin`, `1BL_pub.bin`); `dumps/FLASH_1.bin` é um freeboot de
   terceiros (header `0xFF4F`, build 1888, `CB_A` com a marca `15432 py builder`,
   cadeia `CB/CD/CE/CF`), não a cadeia `SB/SC/SD/SE` que o toolkit espera; e o
   console é Corona 4GB (eMMC), então gravar uma boot chain errada exige leitor de
   eMMC por hardware, porque o recovery por XeLL não cobre esse caso como cobre em
   NAND. O CE (HV + kernel) é RC4/LZX e é decifrável com a cadeia do próprio
   repositório (`tools/bootchain.py`), que produz
   `dumps/stages/kernel_17559.bin` = HV (`0x40000`, build 17559) + kernel PE.
   Ou seja, o patch offline é possível, só não foi necessário.
2. **Leitura da save area do HV**: resolvido na seção 9. O bloqueio era endereço
   errado, não proteção.
3. **`XeCryptRandom` com semente controlada**: o RNG é do engine e não há
   registrador de semente acessível. Irrelevante agora que as chaves são lidas
   direto da RAM do HV.
4. **BadUpdate**: o exploit do grimdoomer (v1.3) chega a HV mode neste console
   (três piscadas verdes significam stage 3 vencido; só o
   `XamLoaderLaunchTitle` do payload falha). Ficou sem uso: HV mode não era
   necessário, porque o hypercall de cópia já lia a RAM do HV a partir do kernel.
   Guardado como referência.

O que falta está do lado do emulador: modelar a cifra com a chave por boot (o dump
e o par do mesmo boot são o oráculo) e decidir o modelo de confiança, já que a
chave real de um boot não existe no seguinte. As peças estão em
`crates/xenon-cpu/src/security.rs` e validadas contra o par do silício.

## Fontes

- GoobyCorp/Xbox-360-Crypto: `MemCrypto.py`, `XeCrypt.py`, `cpu_key.py`
  (https://github.com/GoobyCorp/Xbox-360-Crypto)
- icode4.coffee, "Hacking the Xbox 360 Hypervisor Part 1: System Overview"
  (2025-02-24) (https://icode4.coffee/?p=1047)
- Free60 wiki: Reset Glitch Hack, Hypervisor, CB Code, System Software
  (https://free60.org)
- invoxiplaygames.uk/x360-research: `official-software/hypervisor/key_derivation.md`
- Patentes da Microsoft US7734926B2, US7822993B2, US9141558B2, US7356668B2
  (Google Patents)
- Código do Xenia: `src/xenia/kernel/xboxkrnl/xboxkrnl_crypt.cc`,
  `src/xenia/cpu/xex_module.cc`, `src/xenia/memory.cc`
- Este repositório: `research/hv/README.md`,
  `research/hv/hypervisor_analysis.md`, `crates/xenon-cpu/src/security.rs`,
  `crates/xenon-cpu/tests/chain.rs`, `dumps/console/sec_pair_0x3b80.*`
