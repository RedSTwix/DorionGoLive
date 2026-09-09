# Solução de problemas

## A live não abre ou a transmissão não inicia

1. Confirme que o aplicativo UrbanVPN, e não somente a extensão do navegador,
   está instalado em `C:\Program Files\UrbanVPN\bin`.
2. Abra o UrbanVPN manualmente uma vez e feche qualquer tela de configuração.
3. Execute `dorion-golive status` em um PowerShell novo.
4. Confirme que `UrbanVPN`, `plugin` e `rodando` aparecem como `sim`.
5. Feche e abra o Dorion pelo atalho normal e tente novamente.

Não clique repetidamente enquanto a notificação informa que a VPN está sendo
confirmada: a ação original permanece retida e é liberada pelo plugin.

## Pessoas ao vivo não aparecem depois de reiniciar

O Dorion precisa iniciar enquanto a VPN já está confirmada. O monitor detecta
uma abertura direta e faz uma única reabertura automática pelo atalho real. Se
isso não ocorrer, verifique o log e se o autostart está ativo.

## Coletar diagnóstico

```powershell
dorion-golive status
Get-Content "$env:LOCALAPPDATA\DorionGoLive\dorion-golive.log" -Tail 200
```

Antes de compartilhar o log, revise nomes de usuário e caminhos locais. O
programa não registra conteúdo de mensagens ou da transmissão.

## O instalador não consegue baixar um requisito

Confirme que firewall, DNS e filtro de rede permitem HTTPS para
`api.github.com`, `github.com` e `install.urban-vpn.com`. O instalador também
precisa encontrar `curl.exe` e Windows PowerShell dentro de
`%SystemRoot%\System32`; ele não executa ferramentas homônimas encontradas na
pasta do download.

## Reinstalação limpa

```powershell
& "$env:LOCALAPPDATA\DorionGoLive\dorion-golive.exe" desinstalar
```

Depois, execute novamente `DorionGoLive-Setup.exe`.
