# Começando

## Requisitos

| Item | Para quê |
|---|---|
| Rust estável (edition 2021) | emulador e testes |
| Python 3.10+ | ferramentas de `tools/` |
| `cryptography` (pip) | AES/RC4 da boot chain e dos XEX |
| `capstone` (pip) | disassembly PPC (`ppcdis.py`, `cbscan.py`, `opscan`-comparação) |
| Xbox 360 XDK (`XEDK`) | só para recompilar os binários de captura em `tools/capture/` |

Os dumps do console **não** são necessários para compilar nem para os testes de
unidade; só para os testes de cadeia (marcados `#[ignore]`) e para rodar a
cadeia de verdade.

## Compilar e rodar

```powershell
cargo build --release

# Cadeia completa até o handoff do CD (precisa de dumps/stages/CB_A.mem e dumps/FLASH_1.bin)
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage cd --max-steps 400000000

# HV + kernel virtual
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --max-steps 400000000 --hv-max-steps 40000000

# modo experimental de seis threads
cargo run --release -p xenon-cpu --bin runcb -- ... --stage hv --hv-threads 6
```

### Flags do `runcb`

| Flag | Efeito |
|---|---|
| `--stage cb_a\|cb_b\|cd\|hv` | estágio onde o runner começa/para |
| `--max-steps N` | limite de instruções do estágio corrente |
| `--hv-max-steps N` | limite depois do handoff para o HV |
| `--hv-threads 1\|6` | uma CPU (default) ou o modo de seis threads |
| `--fuses FILE` | usa fuses reais em vez dos embutidos |
| `--security-engine off\|generic\|real` | modelo do security engine (default: `real` se as chaves capturadas existirem, senão `generic`) |
| `--security-keys FILE` | arquivo com a tripla `hash`/`white`/`aes` (default `dumps/console/sec_keys_v25.txt`) |
| `--xosc-test FILE` | compara a resposta XOSC com um arquivo capturado |
| `--log-pc PC:r,r` | imprime registradores quando o PC bate |
| `--cd-stop-pc HEX` / `--hv-stop-pc HEX` | para em um PC interno (snapshots de teste) |
| `endereço:tamanho` | dump de memória no fim (`8000017c04000000:5400`) |

Os testes de cadeia usam `--cd-stop-pc` para congelar fronteiras internas e
comparar byte a byte com os arquivos de referência.

## Testes

```powershell
cargo test -p xenon-cpu                      # testes de unidade, sem dumps
cargo test -p xenon-cpu --release --test chain -- --ignored   # cadeia real (precisa de dumps)

# as duas provas do security engine (precisam da captura v25)
cargo test -p xenon-cpu --test chain hv_dump_key_search -- --ignored
cargo test -p xenon-cpu --test chain captured_pair_with_real_keys_validates_cipher -- --ignored
```

O CI (`.github/workflows/ci.yml`) roda build, clippy, testes de unidade e
compila os testes de integração; os `#[ignore]` não rodam lá porque dependem
dos dumps.

## Dumps

`dumps/` está no `.gitignore`: nada do console entra no repositório.

| Arquivo | Origem | Usado por |
|---|---|---|
| `dumps/FLASH_1.bin` | NAND/eMMC via XeLL (48 MiB) | `bootchain.py`, `runcb`, testes de cadeia |
| `dumps/FUSE.bin`, `dumps/KV.bin` | XeLL | `runcb --fuses`, referência |
| `dumps/stages/CB_A.mem` | CB_A já decifrado e carregado em SRAM | ponto de entrada do `runcb` |
| `dumps/stages/*.bin` | saída de `bootchain.py` | oráculos dos testes |
| `dumps/console/*` | capturas do console (pares, HV, SMC) | validação do security engine |

Gerar os estágios:

```powershell
python tools/bootchain.py dumps/FLASH_1.bin dumps/stages
```

## Problemas comuns

- **`Unmapped`/`Translation` no meio do boot**: confira se está rodando com o
  `dumps/FLASH_1.bin` certo (build 17559) e com `CB_A.mem` gerado por
  `bootchain.py`.
- **`capstone` não encontrado**: `python -m pip install --user capstone`, ou
  aponte `PYLIB` para o diretório onde instalou.
- **Testes de cadeia pulando**: eles começam com `if !path.exists() { return; }`
  quando o dump não existe; rode com `--ignored` e com `dumps/` populado.
