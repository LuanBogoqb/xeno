# Console capture

Homebrew que copia as imagens dos módulos já carregados (`xboxkrnl`, `xam`,
`launch`, `proto`) para uma pasta `CAPnn` ao lado do XEX:

- `.mem`: imagem do módulo, no tamanho registrado pelo loader;
- `.ldr`: registro do loader (0x60 bytes);
- `.pages`: mapa de páginas, com offset, proteção e bytes copiados por página
  (três u32 big-endian, passo de 4096);
- `report.txt`: versões do kernel, estatísticas de memória, módulos encontrados
  e 15 respostas SMC (`0x12`, `0x07`, `0x0A`, `0x0F`, `0x17`, três amostras cada);
- `manifest.json`: tamanho e SHA-256 de cada arquivo.

Roda como título, não como plugin: substitui o título atual (Aurora) e a saída é
recolhida por FTP quando o Aurora voltar. O resultado deste console está em
`dumps/console/CAP00/`.

## O que não é

Páginas sem leitura, guard, nocache ou writecombine são puladas e ficam zeradas
no `.mem`, marcadas no `.pages`; zero ali não é valor medido. O console continua
executando durante a cópia, então não é um snapshot atômico, nem RAM física,
nem hypervisor/MMIO, nem registrador de boot. Não busca CPU key nem KV.

## Build

```powershell
./tools/smc-capture/build.ps1 -InstallToolchain   # instala a toolchain fixada
./tools/console-capture/build.ps1
```

Usa Rust 1.97.1, `xbox360-sys` 0.2.0 e o mesmo `xex-link` público, sem XDK.
Saída em `dist/Console-Capture/` (`default.xex` e ZIP), fora do git.

## No console

Abra `default.xex` pelo Aurora ou copie a pasta de um pendrive FAT32. A tela pode
ficar preta; ao terminar, o programa tenta voltar ao dashboard. Recolha a pasta
`CAPnn` inteira.

`link.x` e `powerpc-unknown-xbox360.json` vêm de `rust-xbox360` (MIT ou
Apache-2.0); o aviso está em `THIRD-PARTY-LICENSE.txt`.
