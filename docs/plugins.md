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
  plugin1  dumps/hdd/Plugins/Proto.xex  XEX2 encryption=1 compression=LZX entry=0x91883800 base=0x91850000 imports=[xboxkrnl.exe xam.xex]
Plugin loader: M6; the list and the headers are read, the images are not mapped or executed
```

## O que já funciona

- leitura do `[Plugins]` do `launch.ini`, com comentário, aspas e slot repetido;
- mapeamento dos drives do console para `dumps/`;
- leitura do header XEX2: ponto de entrada, base da imagem, endereço de carga,
  tipo de cifra, tipo de compressão e as bibliotecas importadas;
- relatório no início do estágio de kernel, dizendo o que o boot carregaria.

## O que falta

O mapeamento e a chamada de entrada são M6, e dependem de duas peças que ainda
não existem no emulador:

1. extrair a imagem do XEX (AES-128 + LZX) para RAM no endereço de carga;
2. resolver os imports pelo kernel real, que é o que `xam.xex` exige.

Enquanto isso, o relatório já mostra o primeiro bloqueio de cada plugin: as
bibliotecas importadas. Um plugin que importa `xam.xex` não tem para onde
resolver hoje, e é isso que o M6 destrava.
