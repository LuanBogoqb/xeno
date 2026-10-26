# Security engine

O security engine (SE) do Xenon cifra a memória de DRAM no caminho do
barramento. O CPU não vê ciphertext: o HV, o kernel e os títulos leem plaintext
porque o SE decripta por página, conforme os bits de caminho da MMU.

O emulador precisa do mesmo modelo para (a) rodar o HV/kernel com memória
cifrada de verdade e (b) decifrar dumps e capturas do console.

## O modelo (`crates/xenon-cpu/src/security.rs`)

- **Cifra**: AES-128 em modo **XEX** (tweakable).
- **Tweak**: Toeplitz/GF(2), derivado de `white_key`, endereço da linha e de um
  valor de whitening de 10 bits vindo da page table.
- **Bloco**: 16 bytes; unidade de transformação: linha de cache de 128 bytes.
- **Chaves**: `white_key`, `aes_key`, `hash_key` (16 bytes cada).

```rust
let e = SecurityEngine::with_keys(0, white, aes, hash);
let plain = e.xex_decrypt(&ciphertext, 0x3B80);
```

O mesmo algoritmo foi conferido byte a byte contra o `MemCrypto.py` do Gooby
para a chave `0x55`.

## Chaves por boot

As chaves de DRAM **mudam a cada boot**. Três capturas do mesmo `0x3B80` com
plaintext idêntico têm ciphertext diferente:

```
v18  9be6b3f0...   v21  da352ee9...   v24  bbb9629a...
```

Não existe KDF da CPU key para a chave de DRAM: ela é gerada no boot. Mas o HV
mantém a tripla viva na própria RAM enquanto o console está ligado, em
`0x80000102_000100F0` (tabela que é zero na imagem retail):

| Endereço | Conteúdo |
|---|---|
| `0x80000102_000100F0` | `hash_key` |
| `0x80000102_00010100` | `white_key` |
| `0x80000102_00010110` | `aes_key` |

Ler isso do kernel só é possível pela janela do HV via o hypercall de cópia —
o procedimento está em [capturing-from-console.md](capturing-from-console.md).

## Evidência de validação

Captura `dumps/console/seccap_v25.bin` (268.416 B), mesmo boot:

1. ciphertext de hardware em `0x3B80..0x4B80` (4 KiB, 256 blocos);
2. plaintext da região 1, byte-idêntico a
   `dumps/stages/kernel_17559.bin[0x3B80..0x4B80]`;
3. a tripla de chaves acima.

Decifrar o ciphertext com o modelo e essas chaves reproduz o plaintext **byte a
byte nos 256 blocos**. A chance de um falso positivo numa varredura de ~200 mil
candidatos é da ordem de 2⁻¹²⁸.

```powershell
cargo test -p xenon-cpu --test chain hv_dump_key_search -- --ignored
cargo test -p xenon-cpu --test chain captured_pair_with_real_keys_validates_cipher -- --ignored
```

O primeiro teste não hardcoda nada: varre todas as janelas de 48 bytes do dump,
testa só o primeiro bloco por candidato (seis atribuições de chave) e só
re-decifra os 4 KiB completos no acerto — 0,3 s.

Detalhes da campanha, hipóteses descartadas e o disassembly da rotina `0x84e0`
(que salva/restaura os registradores do SE):
[`research/hv/security_engine_research.md`](../research/hv/security_engine_research.md).

## Limites conhecidos

- A chave é **por boot**: decifrar um dump exige a captura do mesmo boot. Para
  rodar do zero, o emulador pode gerar a própria chave (modo genérico `0x55`),
  que é autoconsistente mas não decifra memória de outro boot.
- `security.rs` modela a cifra e o caminho protegido (CRC por linha) de forma
  experimental; a validação cobre a transformação XEX + tweak, não o CRC.
- O `--security-engine` do `runcb` liga o modelo no runner; o default é
  desligado para não mascarar acessos crus em testes de boot chain.
