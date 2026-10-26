# Decisões

1. **LLE, não HLE (opção B).** Roda `xam.xex`, `hud.xex` e `dash.xex` reais dos dumps do dono. O Xenia serve só de fonte de componentes (frontend PPC/VMX128, Xenos, XMA), não de base.
2. **1BL em HLE.** É mask ROM dentro do silício. A emulação começa no CB, com a CPU key do dump.
3. **Sem glitch.** O RGH existe para burlar a assinatura do CB; no emulador a verificação é pulada e roda-se o CD/HV/kernel já patcheados da NAND do dono.
4. **Plugins existentes, sem trapaça própria.** Proto + GoldSpoof carregam como no console. Exige o kernel **17559** (o GoldSpoof é preso a ele).
5. **Live via rede da Microsoft.** O Proto usa o Live real, então o KV do console é visto pela Microsoft: risco de ban aceito pelo dono, e o XOSC do hypervisor emulado vira requisito.
6. **Nada da Microsoft no ISO.** Firmware, KV e dash vêm dos dumps do usuário, extraídos na instalação.
7. **Plugin de kernel carrega como no console.** O emulador lê `[Plugins]` do `launch.ini` e carrega o `.xex` que o usuário fornecer, do mesmo jeito que o Dashlaunch. O emulador não patcheia kernel, não embute plugin e não contém nada do Proto nem do GoldSpoof: o que o plugin faz depois de carregado é do plugin.
