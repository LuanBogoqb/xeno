# Proto (`Proto.xex` + `Proto.ini`)

Plugin de HUD/rede da comunidade. Roda como plugin de kernel no RGH.

- `Proto.xex` sha256 `50e1e9959c71cc717f757416ca805a46642115c75984db7e6df657cca7d1d0e5`
- `Proto.ini` sha256 `1b5f3d73db539e8ce9011b2614e0c18402baa603b6479ec92ca8671d0392040a`
- XEX2, AES + LZX. DLL, base `0x91850000`, entry `0x91883800`, 271 imports (`xam.xex` e `xboxkrnl.exe`). Código em `.text` e `.ptext`.

## O que o código faz (por imports)

| Função | Evidência |
|---|---|
| Altera o HUD/guide | `XuiRegisterClass`, `XuiElementSetPosition`, `XuiResource*`, `XuiSendMessage`, `XNotifyQueueUI`, `XamShowCustomPlayerListUI` |
| Rede própria (TCP) | `NetDll_socket/connect/send/recv/setsockopt`, `XNetGetEthernetLinkStatus` |
| Identidade e Gold | `XamUserGetSigninInfo/State`, `XamUserGetMembershipTier`, `XamGetCurrentTitleId`, `XamGetExecutionId` |
| Ids do console | `ExGet/SetXConfigSetting`, `XeKeysUnObfuscate`, `HalSendSMCMessage`, `XboxHardwareInfo` |
| Criptografia | `XeCryptSha`, `HmacSha`, `Rc4`, `XeCryptBnQwNeRsaPubCrypt`, `XeCryptBnDwLePkcs1Verify` (verifica assinatura RSA, provavelmente de respostas do servidor) |
| Carrega plugins e dash | `XexLoadImage`, `XexGetProcedureAddress`, `XamLoaderLaunchTitleEx`, `XamReloadSysApp`, `ObCreateSymbolicLink` |
| Downloads em jogo | `XamBackgroundDownloadSetMode` |
| Tarefas | `ExCreateThread`, `XamTaskSchedule` |

## `Proto.ini`

`protoHud`, `disableHudRestriction`, `customDash`/`customDashPath` (só devkit, não muda o boot), `disableOnlinePresence` (presença é entre usuários Proto), `backgroundDownloadPatch`, `[Plugins] plugin1..5` (só plugins ao lado do `Proto.xex`).

## Limites

- `.rdata` tem blobs ofuscados e quase nenhuma string legível: **endpoints, portas e protocolo não saem de análise estática simples.** Não tentei desofuscar.
- Para saber com quem ele fala: capturar o tráfego no RGH real com o Proto carregado (ver o marco de Live em [`docs/status.md`](../../docs/status.md)).

## Requisitos para o emulador

Pilha de rede (`NetDll_*`), `XNotify`/XUI do `xam` real e o loader de plugins do
emulador (`[Plugins]` do `launch.ini`, ver [plugins.md](../../docs/plugins.md)).
Teste de aceitação: o HUD do Proto aparece sobre a dash emulada.
