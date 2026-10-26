# Plugins

Como o emulador carrega plugin de kernel, do mesmo jeito que o Dashlaunch carrega
no console.

O emulador não patcheia kernel nem contém plugin nenhum. Ele lê a configuração do
usuário e carrega o que estiver lá. O que o plugin faz depois de carregado é do
plugin.

## A convenção

`launch.ini`, seção `[Plugins]`, chaves `plugin1` a `plugin5`:

```ini
[Plugins]
plugin1 = Hdd:\Plugins\Proto.xex
```

Regras, iguais às do console:

- cinco slots, na ordem `plugin1` a `plugin5`;
- a chave pode aparecer mais de uma vez no arquivo: vale a última;
- `;` e `#` iniciam comentário;
- aspas em volta do caminho são ignoradas;
- `plugin0` e `plugin6` em diante são ignorados.

Caminhos de drive do console são resolvidos dentro de `dumps/`:

| No ini | Neste repositório |
|---|---|
| `Hdd:\Plugins\Proto.xex` | `dumps/hdd/Plugins/Proto.xex` |
| `Usb0:\Plugins\Proto.xex` | `dumps/usb0/Plugins/Proto.xex` |
| `Flash:\...` | `dumps/flash/...` |

Os drives reconhecidos são `Hdd`, `Hdd1`, `Usb0`, `Usb1` e `Flash`. Qualquer
outro caminho é usado como está: absoluto fica absoluto, relativo é resolvido a
partir do diretório do ini.

## Como usar

```powershell
# lê dumps/hdd/launch.ini
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --plugins-dir dumps/hdd

# aponta outro ini
cargo run --release -p xenon-cpu --bin runcb -- ... --stage hv --launch-ini dumps/hdd/launch.ini

# plugin avulso, ocupa o primeiro slot livre; pode repetir
cargo run --release -p xenon-cpu --bin runcb -- ... --stage hv --plugin dumps/Plugins/Proto.xex
```

O log, depois do handoff para o kernel:

```
Plugins: 1 from dumps/hdd/launch.ini
  plugin1  dumps/hdd/Plugins/Proto.xex  XEX2 encryption=0 compression=LZX entry=0x91873910 base=0x91850000 image=389120 imports=[xam.xex=190 xboxkrnl.exe=167] (181 slots, 176 thunks)
Plugin loader: the image is extracted and its imports are classified; mapping into guest RAM and the entry call need the kernel (M6)
```

## O que já funciona

- leitura do `[Plugins]` do `launch.ini`, com comentário, aspas e slot repetido;
- mapeamento dos drives do console para `dumps/`;
- leitura do header XEX2: ponto de entrada, base da imagem, endereço de carga,
  tipo de cifra, tipo de compressão e as bibliotecas importadas;
- extração da imagem em `crates/xenon-cpu/src/xex/`: AES-128 quando o XEX é
  cifrado, LZX sempre, sem depender do Python;
- layout de carga: imagem plana quando o XEX não tem tabela de seções usável (é
  o caso dos plugins empacotados) e por seção quando tem;
- classificação dos imports a partir da imagem: slot ordinal (4 bytes) ou thunk
  de chamada (16 bytes), com o ordinal de cada um;
- patch no formato do console: o slot recebe o endereço resolvido e o thunk
  recebe `lis r11, hi; addi r11, r11, lo; mtctr r11; bctr`. O resolvedor é do
  chamador e recebe `(biblioteca, ordinal, tipo)`, porque o console aponta
  alguns thunks para um stub diferente da função (64 KiB acima, no Proto);
- relatório no início do estágio de kernel, já com a imagem extraída e os
  imports classificados.

O build 2.4.7 que roda no console (`Proto.xex`, não cifrado) serve de prova: a
imagem extraída bate com `dumps/console/CAP00/proto.mem` em `.pdata` e no stub de
entrada, e os 357 imports são patcheados byte a byte iguais à captura. O
`Proto.xex` cifrado de `samples/` também extrai e mapeia as seções.

### A API

```rust
let x = Xex::parse(bytes)?;
let image = x.map(&x.extract()?)?;
let report = x.patch_imports(&mut image, |library, ordinal, kind| {
    resolve(library, ordinal, kind)
});
```

`ImportReport` diz quantos slots e thunks foram resolvidos e lista os pares
`(biblioteca, ordinal)` que o resolvedor não respondeu.

## O que falta

O que resta do M6 depende do kernel já rodando:

1. mapear a imagem no `Bus` no endereço de carga, pelas páginas do guest;
2. resolver os imports pela tabela de exportação real de `xboxkrnl.exe` e
   `xam.xex` (M3+), com a distinção entre endereço da função e stub de chamada;
3. chamar a entrada com a semântica de DllMain (contexto de thread, retorno).

Hoje o resolvedor é do chamador: sem `xam` carregado não existe para onde
resolver, e o relatório mostra isso.
