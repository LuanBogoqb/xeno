# Créditos

O que foi usado de trabalho de outras pessoas, e sob qual licença. Nada disso é
redistribuído neste repositório.

## Plugins

| Projeto | O que é | Como entra aqui |
|---|---|---|
| [Proto](https://www.freestealth.com/) | Plugin de HUD e rede, com autenticação no Live nativo | Carregado como plugin de kernel; é o caminho para o Live. O binário não é distribuído aqui — vem do usuário |

O Proto é distribuído pela comunidade em [freestealth.com](https://www.freestealth.com/).
O README do alvo em `research/proto/` registra o que foi analisado e o hash do
binário usado.

## Emuladores e referência técnica

| Projeto | Uso | Licença |
|---|---|---|
| [Xenia](https://xenia.jp) | tabelas ordinal → nome de `xboxkrnl.exe` e `xam.xex`, e a referência de comportamento do Xenos | tabelas em BSD-3 (`tools/data/`) |
| [xenon-emu](https://github.com/xenon-emu/xenon) | formato de página, MMU e offsets do controlador de interrupções, usados como referência numérica | nenhum código copiado |
| [xenon-bltool](https://github.com/InvoxiPlayGames/xenon-bltool) | conferência da boot chain contra uma implementação independente | GPL-2; só os shims de build estão aqui |
| [GoobyCorp/Xbox-360-Crypto](https://github.com/GoobyCorp/Xbox-360-Crypto) | `MemCrypto.py` como referência do algoritmo do security engine; a implementação em Rust é própria | consulta de algoritmo |
| [J-Runner](https://github.com/Octal450/J-Runner-with-Extras) | layout do header e cifra do firmware do SMC, referência de `tools/smcextract.py` | consulta de algoritmo |
| [Dashlaunch](https://www.realmodscene.com) | convenção de `launch.ini` e de plugin de kernel que o loader segue | convenção, não código |
| XeLL | dump da NAND e da CPU key do próprio console | ferramenta do usuário |
| [free60](https://free60.org), [icode4.coffee](https://icode4.coffee) | documentação pública da plataforma, do hypervisor e do security engine | referência |

O BadUpdate (grimdoomer) aparece em `research/hv/` só como referência de
endereçamento e de exploit; não é usado pelo emulador nem distribuído.

## Ferramentas

`capstone` e `cryptography` no Python, e os crates declarados em
`crates/xenon-cpu/Cargo.toml`. Cada um mantém a própria licença.
