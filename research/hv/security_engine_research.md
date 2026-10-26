# Xbox 360 Security Engine — Memory Encryption Research

## Summary

- **Cipher candidate:** AES-128 (Rijndael, 10-round, 128-bit block/key). Confirmed by CB/bootloader notes ("AES128 for crypto") and by the GoobyCorp reverse-engineering replication (`MemCrypto.py`), which uses `AES(key, ECB)` as the core block cipher. No AES S-box/TEA constants appear in the HV because crypto is hardware-delegated to the on-die security engine (SoC).
- **Mode:** XEX (XOR-Encrypt-XOR) per 16-byte block, i.e. a tweakable block mode: `C = AES_K(P ⊕ T) ⊕ T` (and inverse `P = AES_K^{-1}(C ⊕ T) ⊕ T`). This is the Liskov–Rivest–Wagner (LRW/XEX) construction, not XTS (no ciphertext stealing, no second key) and not plain CBC/CTR. The tweak `T` is per-block and derived from the cache-line address + 10-bit whitening value, which is why repeated plaintext blocks at different addresses produce different ciphertext (the captured pair explicitly rules out ECB and repeating XOR keystreams).
- **Block size:** 16 bytes (AES block). Encryption unit is a full **128-byte cache line** (16 AES blocks); all 16 blocks of a cache line are transformed, with the tweak varying per 16-byte sub-block position (and per cache line address/whitening).
- **Integrity:** Protected pathway (P=1) uses the same XEX transform plus a **16-bit CRC per 128-byte cache line**, stored in CPU SRAM (64 slots × 512 entries). Encrypted pathway (P=3) uses XEX only (no CRC).
- **KDF for memory keys:** Not a software KDF from the CPU key. Memory-pathway keys (`white_key`, `aes_key`, `hash_key`) are **per-boot random values from the on-die security engine RNG**, seeded at boot from (a) a hash of the full fuse set, (b) the timebase counter, and (c) the hardware RNG. They are never exposed via MMIO (key registers at SoC +0x24000 read back as `0xffffffff`). The CPU key is used elsewhere (keyvault obfuscation, XEX keys) via HMAC-SHA1 over the "integrity violation signature" (`0x80000200_000250B8`), not for DRAM encryption.
- **Tweak derivation:** Toeplitz/GF(2) linear function (Gooby's `get_tweak_0`/`get_tweak_1` with `GF2_TAB`) combining `white_key` (128 bits) with the cache-line address (shifted right 4) and the 10-bit whitening value, producing a 128-bit tweak. This matches the Microsoft patent US7734926B2, which specifies the key-derivation circuit "preferably comprises a Toeplitz function" and address bits driving encryption.
- **Confidence:** High for AES-128 + XEX + address-derived tweak + 128-byte unit (multiple independent sources: CB notes, patents, Gooby replication, captured real pair). Medium for the exact Toeplitz polynomial / whitening-bit packing and the 10-bit whitening placement (reverse-engineered, not silicon-verified). Low for any claim that the CPU key feeds the memory key (it does not).

## 1. Cipher candidate

- **AES-128** (Rijndael, 128-bit key, 128-bit block, 10 rounds). Evidence:
  - Reset Glitch Hack / Free60: CB "is using AES128 for crypto".
  - GoobyCorp `Xbox-360-Crypto/MemCrypto.py`: `Cipher(AES(akey), ECB())` with 16-byte keys (`white_key`, `aes_key`, `hash_key`), block size 16.
  - Xenia implements `XeCryptAesEcb/Cbc` as AES-128 for XEX/keyvault, consistent with the SoC offloading model.
  - The HV contains no AES/TEA constants because encryption is delegated to the security engine (confirmed by hypervisor analysis in this repo).

## 2. KDF (CPU key → memory encryption key)

- **There is no direct KDF from CPU key to memory-encryption key.** The DRAM pathway keys are generated at boot (hardware RNG / key blob) and change every boot (replay protection). **Corrected 2026-10-25:** they are not confined to the engine — while the console is up the hypervisor keeps the live triple in its own RAM at `0x80000102_000100F0`, readable with the hypervisor copy helper from kernel mode (§9). The triple decrypted a captured 4 KiB hardware pair byte for byte, so the AES-128 XEX + Toeplitz model is hardware-validated.
- The CPU key (`5cb2...6f78 (redacted; ver dumps/console/)`, from OTP line 3/4/5/6) is used for:
  - Keyvault decryption (HMAC-SHA1-based RC4, see `XeCryptKeyVaultDecrypt`).
  - Derivation of software keys (XEX2, DVD auth, system link, roamable) via HMAC-SHA1 over the "integrity violation signature" read from MMIO `0x80000200_000250B8` (see `key_derivation.md` in x360-research).
- The integrity-violation signature itself is produced by the SoC and read with an `ld` from `0x80000200_000250B8` (expected CRC32 `0x4B17A409`, begins `81300D...`). Its CRC32 differs only on zero-fuse CPUs.
- Key packing/obfuscation for the security engine uses `pack_sec_eng_keys` (SHA1-cycle XOR stream) and RSA encryption to Microsoft's public key (see `MemCrypto.calc_key_blob`), which is how keys are sealed into the keyvault/NAND, not how DRAM keys are derived.

## 3. Block size and mode

- **AES block:** 16 bytes. **Unit of transformation:** 128-byte cache line (8 AES blocks per cache line conceptually, though the tweak varies per 16-byte sub-block).
- **Mode:** XEX (tweakable): XOR tweak before AES, AES core, XOR tweak after. Gooby's `encrypt_block`/`decrypt_block`:
  - `tweak = get_tweak_0(address)` (128-bit, from `white_key` + address + whitening)
  - `enc = AESdec(plaintext ⊕ tweak) ⊕ tweak` (and inverse for decrypt)
- **Tweak inputs:** per-boot `white_key` (128 bits), cache-line address (real-mode address, path bits P), 10-bit whitening value (from page table), and slot number (upper 6 bits of whitening select CRC slot).
- **Why "not ECB":** identical 16-byte plaintext blocks at different addresses (or different whitening) yield different tweaks → different ciphertext. The captured pair (`dumps/console/sec_pair_0x3b80.bin`, 4 KiB at `0x3B80..0x4B80`) confirms repeated plaintext blocks do not collapse to equal ciphertext, and the XOR difference has no period, ruling out a repeating keystream (CTR-like) and plain ECB.
- **Protected pathway (P=1):** same XEX transform + 16-bit CRC per cache line, incorporating ciphertext, plaintext, and `hash_key` (collision-resistant against ciphertext-only collisions). CRCs live in CPU SRAM (64 slots).
- **Encrypted pathway (P=3):** XEX only, no CRC; requires RMCI (LPCR bit 62) disabled.

## 4. Security engine registers

- SoC MMIO is accessed via pathway 2 (P=2), base `0x80000200_00000000` (32-bit view `0x20000000`...).
- Integrity-violation signature (used as HMAC key by HV): MMIO `0x80000200_000250B8` (32-bit `0x200250B8`), read with `ld` only.
- Key registers relative to SoC at `+0x24000` (i.e. `0x80000200_00024000`) read back as `0xffffffff` from kernel mode — write-only/masked, so key material is not recoverable via MMIO. This matches the README note ("registradores de chave do SECENG em 0x24000 lêem 0xffffffff").
- Security engine also exposes: eFuses (768 bits, per-console keys/revocation), 32 KB boot ROM, 64 KB SRAM, on-die RNG.

## 5. Patents / papers / emulators

- Microsoft patents (filed ~2004–2005):
  - US7734926B2 — "System and method for applying security to memory reads and writes" (Dinarte R. Morais, Microsoft). Describes using address bits to drive encryption, on-die security engine, and key-derivation circuit "preferably a Toeplitz function". Directly matches the Xbox 360 design.
  - US7822993B2 — "System and method for using address bits to affect encryption".
  - US9141558B2 — "Secure memory control parameters in table look aside buffer data fields and support memory array".
  - US7356668B2 — "System and method for using address bits to form an index into secure memory".
- Fail0verflow / RGH literature: CB initializes the security engine; AES-128 + "strong (Toeplitz?) hashing"; keys seeded from fuse hash + timebase + RNG; reset-glitch hack bypasses bootloaders, not the memory cipher.
- Free60 wiki / Xenon CPU docs: 4 pathways (0 unprotected, 1 protected, 2 SoC, 3 encrypted); 128-byte cache lines; 10-bit whitening; 64 CRC slots in SRAM.
- icode4.coffee "Hacking the Xbox 360 Hypervisor" (2025): detailed MMU pathways, HRMOR, RMCI (LPCR bit 62), page whitening tables, CRC storage. Confirms AES-ECB belief is the author's, with address tweak.
- GoobyCorp `Xbox-360-Crypto` (`MemCrypto.py`): the only public software replication of the memory-encryption algorithm (AES-128 + XOR tweak, Toeplitz tweak, CRC, key blob packing). This is the working reference for the emulator.
- Xenia / xenon-emu: do not emulate the security engine for HV memory; they rely on plaintext backing. Xenia implements XEX AES-128 CBC/ECB for executable loading only.
- Intel XTS-AES (IEEE 1619) is the closest standard analogue and is what later systems (Intel SGX/TME) use, but the 360 construction is the older XEX/LRW form with a custom Toeplitz tweak, not XTS (no ciphertext stealing, no alpha-multiplication tweak advancement per block in the XTS sense — though the tweak is address-derived rather than sequential).

## 6. Captured oracle (this project)

- `dumps/console/sec_pair_0x3b80.bin` (format `SECD`, offset `0x3B80`, length 4096): region 1 (hashed) is byte-identical to `kernel_17559.bin` at the same offsets; region 0 is ciphertext. Test `captured_security_pair_is_real_and_not_reproduced_by_the_placeholder` asserts plaintext == kernel and that the transform is not ECB/repeating-XOR. This is the oracle any future implementation must match.
- `dumps/console/seccap.bin` (66320 bytes) contains additional captures (region 0/1/2; region 3 traps the hypercall).
- CPU key captured from OTP block `0x20000`: `5cb2...6f78 (redacted; ver dumps/console/)`.

## 7. Open questions / next steps

1. Recover the exact Toeplitz polynomial and the 10-bit whitening bit placement by cryptanalysis against the captured pair (the per-boot `white_key`/`aes_key` are unknown but can be treated as unknowns; the address→tweak mapping is the structure to recover). AES-128 key recovery from known plaintext/ciphertext in XEX mode is feasible in principle (16-byte block, known PT/CT, unknown key and tweak; tweak depends on known address + unknown `white_key`).
2. Determine whether the AES core is used in the "standard" or "inverted" direction for encrypt (Gooby uses the decryptor for encrypt and encryptor for decrypt, which is a key-scheduling quirk to verify).
3. Map the protected-pathway 16-bit CRC (which bits of ciphertext/plaintext/hash_key enter) to reproduce SRAM checksums and pass the code-challenge hash at `0x100F0`.
4. Confirm whether region 3 (encrypted pathway) can be captured without trapping the hypercall, to get a second independent oracle.

## 8. Campanha de captura (2026-10-25) — processo guardado

Objetivo: obter as chaves de memoria reais do console (RGH Corona 4GB, plugin
Dashlaunch em kernel mode) para validar a cifra em hardware. Concluido no v25
(§9); o historico abaixo fica como registro do processo e das hipoteses
descartadas.

Capturas (`Seccapture.cpp`, plugin v14..v18) e o que cada uma provou:

| versao | alvo (via `HvxMemcpy`) | resultado |
|---|---|---|
| v14 | regiao 2 em `0x6000/0x10000/0x16000/0x60000` | dados gerais, nada de chave |
| v15 | regiao 2 `0x61000` (bloco do `0x84e0`) | hypercall trava antes de gravar |
| v16 | regiao 1 `0x40000` | fora do mapeamento, trava |
| v17 | regiao 1 `0x16000` (save area `r2+0x16360`) | RAM do HV e protegida, trava |
| v18 | regiao 2 `0x56000` + `0x611C0` | le, mas sao registradores de controle |
| v19 | janela do HV `0x80000102_00006000` (save area) | **trava no hypercall**, mas grava o par antes |
| v21 | so enderecos comprovados + `return TRUE` | captura completa, boot estavel |

Detalhe da v18 (`dumps/console/seccap_v18_0x56000.bin`):

```
region2 0x56000: 0000000000002000 ffffffffffffffff
                 0000000000000000 ffffffffffffffff
                 0000000000000000 ffffffffffffffff
                 000000000001056c ffffffffffffffff
region2 0x611C0: zero
```

Interpretacao: o engine **latcha** as chaves internamente. A rotina `0x84e0`
salva os 5 qwords de `0x6000..0x6040` (base `0x50000` = offset `0x56000`) e o
qword de `0x611C8` na area `r2+0x16360` do HV, zera os registradores e depois
restaura; no estado estavel eles devolvem bits de controle (`0x2000`,
`0x1056C`) e `0`/`0xFF`, nunca os 48 bytes de chave. A unica copia legivel fica
na RAM do HV, que o hypercall nao alcanca (regiao 1 mapeia so a primeira janela
de 64 KiB; `0x16000` dispara fault).

O par plaintext/ciphertext continua sendo capturado a cada boot
(`0x3B80`, regioes 1/0), mas como as chaves sao por-boot o par nao valida a
cifra sem a chave do mesmo boot.

### 8.1 Enderecamento do HV e o teste v19/v21 (2026-10-25)

Descoberta que fechou a duvida de enderecamento: cada segmento de 64 KiB do HV
tem a sua propria janela, `0x80000100_00000000 + n*0x200000000` (segmento 1 =
`0x80000102_...`, segmento 2 = `0x80000104_...`). A tabela de syscalls fica em
`0x80000102_00015FD0` e a save area das chaves em `0x80000102_00016360`
(`r2 = 0x8000010200000000`, `r2+0x16360`). Isso veio do `4thStage.asm` do
BadUpdate, nao de chute.

O v19 usou esses enderecos (chunk depois do par) e o resultado foi conclusivo:

- `dumps/console/seccap_v19b.bin` (8.336 B, v19): o par de 4 KiB foi gravado
  **inteiro** e o arquivo termina exatamente antes do primeiro read do
  segmento 1. Ou seja, o hypercall **trava** em `0x80000102_00006000`.
- O "crash" que aparecia depois da captura nao era do HV: o plugin retornava
  `FALSE` no `DllMain`, o que o dashlaunch interpreta como "plugin falhou ao
  carregar" e derruba o boot. Isso mascarou o resultado do v19 por varias
  rodadas.

Conclusao (corrigida em §9): o hang do v19 **nao** era protecao do HV, era
endereco invalido. A regra de mapeamento e por bloco de 64 KiB: o bloco `k`
mora em `0x80000100_00000000 + k*0x200000000`, e o offset usado no endereco e o
offset fisico absoluto dentro da imagem. Ou seja, `0x6000` pertence ao bloco 0
(`0x80000100_00006000`), nao ao bloco 1 — e `0x80000102_00006000` (bloco 1 com
offset de bloco 0) simplesmente nao esta mapeado. Com os enderecos certos
(`0x80000102_00015F00`, `0x80000102_00016360`, ...) o hypercall le a janela
decriptada do HV a partir do kernel, incluindo os 256 KiB completos.

O v21 (`tools/capture/seccapture.cpp`) e o plugin valido: sem reads impossiveis,
`return TRUE`, cabecalho gravado antes de cada copia e `FlushFileBuffers` por
registro, entao um arquivo parcial sempre aponta o endereco exato que falhou.

### 9. Chaves de DRAM encontradas na RAM do HV (2026-10-25)

Com o mapeamento correto, o v25 dumpou os 256 KiB do HV **e** o par
plaintext/ciphertext do mesmo boot (`dumps/console/seccap_v25.bin`,
268.416 B). Uma varredura de todas as janelas de 48 bytes da dump, testando
as seis atribuicoes `white`/`aes`/`hash`, achou a tabela de chaves em runtime
em `0x80000102_000100F0` (zeros na imagem retail):

```
0x100F0  23fb...2d2f   hash_key
0x10100  a06d...9855   white_key
0x10110  d611...552d   aes_key
```

Decifrando o ciphertext de hardware (4 KiB, 256 blocos, `0x3B80..0x4B80`) com
AES-128 XEX + tweak Toeplitz e essas chaves, o resultado e **byte-identico** ao
plaintext capturado (que ja era byte-identico a `kernel_17559.bin`). Teste:

```
cargo test -p xenon-cpu --test chain hv_dump_key_search -- --ignored
```

O teste nao conhece as chaves: ele varre a dump, testa so o primeiro bloco por
candidato e so re-decifra os 4 KiB no acerto (0,3 s). Evidencia e layout em
`dumps/console/sec_keys_v25.txt`.

Isso corrige o §2: as chaves de DRAM **nao** ficam so no die do engine — o HV
mantem a copia viva em RAM enquanto o console esta ligado. Elas continuam
por-boot (tres capturas do mesmo offset com plaintext igual tem ciphertext
diferente), entao a captura precisa ser do mesmo boot, que e o que o v25 faz.

Tentativas de contornar (para retomar depois, sem repetir):

1. **Patch do HV (chaves 0x55)** — precisa da boot chain decifrada e reassinada.
   Bloqueios encontrados:
   - o toolkit `Xbox-360-Crypto` (Gooby) exige chaves privadas BYOK
     (`Keys/SB_prv.bin`, `1BL_pub.bin`); sem elas nao gera imagem bootavel.
   - `dumps/FLASH_1.bin` e um freeboot de terceiros (header `0xFF4F` build
     1888, `CB_A` com watermark `15432 py builder`, cadeia `CB/CD/CE/CF`), nao
     a cadeia `SB/SC/SD/SE` que o toolkit espera.
   - o `CE` (HV+kernel) e RC4/LZX, mas **e decifravel** com a cadeia do proprio
     repo: `python tools/bootchain.py dumps/FLASH_1.bin <dir>` deriva as chaves
     dos headers (HMAC-SHA1 a partir da 1BL key e do campo de chave do CB_B) e
     descomprime o CE. O resultado, `dumps/stages/kernel_17559.bin`, e
     `HV (0x40000, build 17559) + kernel PE`; ou seja, o HV completo de
     256 KiB esta disponivel e o patch offline e possivel.
   - o console e Corona 4GB (eMMC): gravar boot chain errada exige leitor de
     eMMC por hardware; recovery por XeLL nao cobre esse caso como em NAND.
2. ~~**Leitura da save area do HV**~~ — **resolvido em §9.** O bloqueio era
   endereco errado, nao protecao: o hypercall le a janela decriptada do HV
   (bloco `k` em `0x80000100_00000000 + k*0x200000000`), e a tabela de chaves
   de DRAM vive em `0x80000102_000100F0`.
3. **XeCryptRandom com semente controlada** — o RNG e do engine; nao ha
   registrador de semente acessivel. Irrelevante agora: as chaves de DRAM do
   boot sao lidas direto da RAM do HV.
4. **BadUpdate (testado no console, funciona)** — o exploit do grimdoomer
   (v1.3) chega a HV mode neste console (3 piscadas verdes = stage 3 vencido;
   o que falha e so o `XamLoaderLaunchTitle` do payload). Ficou **sem uso**:
   nao foi preciso HV mode, o hypercall de copia ja lia a RAM do HV do kernel.
   Guardado como referencia (`research/hv/README.md`).

Para retomar, o que falta **nao e mais acesso ao HV nem chave** — e o lado do
emulador: modelar a cifra com a chave por-boot (a dump e o par do mesmo boot
sao o oraculo) e decidir o modelo de confianca (a chave real de um boot nao
existe no proximo). O caminho de patch do HV com chaves `0x55` continua
possivel, mas exige reassinar a boot chain e gravar o eMMC (Corona 4GB), sem
recovery por XeLL — nao e necessario para validar a cifra.
fica validada apenas contra a referencia (Gooby), nao contra o silicio.

A memoria de seguranca **nao e mais** `experimental/plaintext-backed`: a cifra
esta validada contra o silicio (par de 4 KiB, 256 blocos, mesmo boot, chaves
lidas da RAM do HV). O que segue experimental e so o modelo do emulador para a
chave por-boot.

## Sources

- GoobyCorp/Xbox-360-Crypto — `MemCrypto.py`, `XeCrypt.py`, `cpu_key.py` (https://github.com/GoobyCorp/Xbox-360-Crypto)
- icode4.coffee — "Hacking the Xbox 360 Hypervisor Part 1: System Overview" (2025-02-24) (https://icode4.coffee/?p=1047)
- Free60 wiki — Reset Glitch Hack; Hypervisor; CB Code; System Software (https://free60.org)
- invoxiplaygames.uk/x360-research — `official-software/hypervisor/key_derivation.md` (Tangled)
- Microsoft patents US7734926B2, US7822993B2, US9141558B2, US7356668B2 (Google Patents)
- Xenia source — `src/xenia/kernel/xboxkrnl/xboxkrnl_crypt.cc`, `src/xenia/cpu/xex_module.cc`, `src/xenia/memory.cc`
- This repository — `research/hv/README.md`, `research/hv/hypervisor_analysis.md`, `crates/xenon-cpu/src/security.rs`, `crates/xenon-cpu/tests/chain.rs`, `dumps/console/sec_pair_0x3b80.*`
