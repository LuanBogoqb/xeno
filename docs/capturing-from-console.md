# Capturando do console

Como os dados que o emulador usa saem do console. Duas classes:

1. **Dumps estáticos**: flash/eMMC, keyvault, fuses e CPU key. Saem por XeLL e
   estão descritos em [`research/flash/README.md`](../research/flash/README.md).
2. **Capturas de runtime**: RAM do hypervisor, registradores do security engine
   e pares plaintext/ciphertext. Precisam de um plugin em kernel mode — é o que
   este documento cobre.

O resultado da campanha (as chaves de DRAM e a validação da cifra) está em
[security-engine.md](security-engine.md).

## O hypercall de cópia

O plugin usa o helper de cópia do hypervisor:

```
r0 = 0            (syscall de hypercall)
magic = 0x72627472, op = 5   (memcpy)
src/dst = 0x8000_0000_0000_0000 | região << 40 | janela << 33 | offset
```

Regiões: **0** = física (ciphertext), **1** = HV/kernel decriptado, **2** = SoC
(security engine, OTP), **3** = encrypted. O tamanho é livre (4 KiB por registro
funciona bem).

A RAM do HV tem 256 KiB divididos em 4 blocos de 64 KiB, e cada bloco mora numa
janela de 8 GiB:

```
bloco k  ->  0x80000100_00000000 + k*0x200000000 + offset_físico
```

O `offset` é o offset físico absoluto dentro da imagem. Foi isso que custou
várias tentativas: `0x80000102_00006000` (bloco 1 com offset do bloco 0) não
está mapeado e trava o console, enquanto `0x80000102_00016360` (bloco 1,
offset certo) funciona. A tabela de referência é a do stage 4 do BadUpdate, que
escreve em `0x80000104_00029B04` e `0x80000106_00030000`.

## Plugin `SecCapture` (`tools/capture/`)

Plugin de kernel do Dashlaunch que copia memória para
`Hdd1:\Plugins\seccap.bin`. Fonte: [`tools/capture/seccapture.cpp`](../tools/capture/seccapture.cpp).

```powershell
tools\capture\build.bat plugin          # XEDK necessário; gera tools\capture\SecCapture.xex
```

Deploy e recuperação:

1. Copie `SecCapture.xex` para `Hdd1:\Plugins\SecCapture.xex`.
2. Ligue o plugin **só** pelo `launch.ini` do pendrive:
   `plugin4 = Hdd:\Plugins\SecCapture.xex`.
3. Deixe o `launch.ini` do **HDD** limpo (`plugin4` vazio). O ini do USB é lido
   primeiro, então tirar o pendrive e reiniciar volta ao boot limpo.
4. Boot com o pendrive → ele captura, cai na dash (Aurora) e o FTP fica
   disponível para puxar o arquivo.

### O que aprendemos na marra

- **`DllMain` tem que retornar `TRUE`.** Retornar `FALSE` faz o Dashlaunch
  tratar o plugin como falho e derrubar o boot *depois* da captura — foi o que
  mascarou o resultado de várias rodadas.
- **Plugin em `Usb:` não carrega** nessa fase do boot; use o caminho do HDD.
- **Grave o cabeçalho antes da cópia e faça flush por registro.** Se um
  endereço travar o console, o arquivo parcial termina exatamente no registro
  que falhou, e você sabe qual endereço foi.
- **Chamadas de HV só funcionam a partir do `DllMain`**; de uma thread de
  sistema o console trava.
- **Região 3** é a única que não usamos; não coloque nada importante depois
  dela.

## Formato do arquivo

```
u32 magic 'SECC' | u32 versão | u32 build do kernel | u32 nº de chunks
repetido: u32 tag (0x3000|região) | u32 janela | u32 offset | u32 len | u8 dados[len]
```

Leitura e conferência contra a imagem retail:

```powershell
python tools/parse_seccap.py dumps/console/seccap_v25.bin
```

O parser mostra cada registro, compara a região 1 com
`dumps/stages/kernel_17559.bin` e imprime os marcos (tabela de syscalls, save
area do SE, tabela de chaves).

## O que a captura v25 pegou

- Os 256 KiB do HV (4 blocos) mais o par ciphertext de `0x3B80..0x4B80` do
  **mesmo boot**.
- A tabela de chaves de DRAM em `0x80000102_000100F0` (zeros na imagem retail),
  no layout `hash`, `white`, `aes`.
- A tabela de syscalls em `0x80000102_00015FD0` — com `0x2540` na entrada `0xD`
  (`HvxPostOutput`), que é exatamente o valor que o stage 3 do BadUpdate troca
  por `0x354`.

Isso permitiu a validação em hardware descrita em
[security-engine.md](security-engine.md).

## Segurança e ética

- **Nada de Live durante experimentos.** Rode com o cabo de rede desconectado;
  autenticar com identidade errada é justamente o padrão que bane console.
- As capturas contêm chaves do **seu** console e ficam em `dumps/` (fora do
  git). Não publique.
- O plugin não patcheia nada: ele só lê. O BadUpdate (exploit de HV) não é
  necessário para essa captura e ficou documentado apenas como referência em
  [`research/hv/README.md`](../research/hv/README.md).
