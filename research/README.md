# Pesquisa

Notas profundas de engenharia reversa do console e dos plugins. Os guias em
[`docs/`](../docs/README.md) resumem o que foi aprendido; aqui ficam os
detalhes, o disassembly e os experimentos que sustentam cada conclusão.

| Alvo | O que tem |
|---|---|
| [`cb/`](cb/README.md) | execução do CB_A/CB_B/CD até o handoff: resultados, limites do TLB, patches da NAND |
| [`flash/`](flash/README.md) | layout do dump de 48 MiB, chaves RC4 por estágio, validação byte a byte do kernel |
| [`hv/`](hv/README.md) | hypervisor: syscalls, SMC, MMU, captura de memória e o security engine (`security_engine_research.md`) |
| [`hv/analysis/`](hv/analysis/) | saída das ferramentas: disassembly, tabela de funções, rotina do security engine |
| [`proto/`](proto/README.md) | plugin Proto: imports, HUD, rede, requisitos para o emulador |

## Convenções

- Um README por alvo, com hashes dos binários analisados (os binários em si
  ficam em `samples/` e `dumps/`, fora do git).
- Toda afirmação que veio de execução real cita o comando ou o dump que a
  reproduz; inferência é marcada como inferência.
- Modelos do emulador são descritos como modelos, não como hardware.

## Como essas notas são geradas

As ferramentas de [`tools/`](../tools/) produzem os artefatos daqui:
`bootchain.py` (estágios), `cbscan.py` (varredura estática), `ppcdis.py` /
`ppcimports.py` (XEX) e `parse_seccap.py` (capturas do console). As capturas em
si seguem [docs/capturing-from-console.md](../docs/capturing-from-console.md).
