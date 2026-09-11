# Segurança e privacidade

## Escopo da VPN

O UrbanVPN cria uma rota de sistema. Enquanto ela estiver ativa, tráfego de
outros aplicativos também pode passar pelo provedor da VPN. O Dorion GoLive
mantém a conexão apenas na abertura/negociação e a desliga depois da confirmação
de mídia. Consulte os termos e a política de privacidade do provedor antes de
usar.

O programa grava um marcador local somente quando ele próprio iniciou a VPN.
Isso evita desligar uma conexão que já estava ativa manualmente. Após uma
finalização inesperada, o marcador permite limpar uma conexão temporária na
próxima execução.

## Dados processados

O serviço:

- conversa com o UrbanVPN apenas por IPC local em `127.0.0.1:19206`;
- expõe ao plugin somente um controle HTTP em `127.0.0.1:9352`;
- observa processos, adaptador de rede e eventos do log local do Dorion;
- não lê nem transmite mensagens, tokens, credenciais, áudio ou vídeo;
- não contém telemetria nem servidor externo próprio.

O controle HTTP não possui autenticação porque escuta apenas no loopback. Isso
impede acesso pela rede, mas outro processo já executando no mesmo usuário pode
solicitar a abertura ou o encerramento de uma janela temporária de VPN.

O log fica em `%LOCALAPPDATA%\DorionGoLive\dorion-golive.log`, é limitado a
512 KiB e pode ser apagado pelo usuário.

## Alterações persistentes do Dorion GoLive

O Dorion GoLive modifica somente o perfil do usuário:

- `%LOCALAPPDATA%\DorionGoLive`;
- `%USERPROFILE%\dorion\plugins\DorionGoLive.js` e `plugins.json`;
- entrada `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DorionGoLive`;
- entrada `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DorionGoLive`;
- variável `PATH` do usuário.

A desinstalação remove essas alterações e desliga uma VPN temporária de
propriedade do programa, mas não desinstala Dorion ou UrbanVPN.

Quando um requisito está ausente, seu instalador oficial pode solicitar UAC e
criar arquivos, serviços, drivers ou entradas de registro próprios. Essas
alterações pertencem ao Dorion ou ao UrbanVPN e seguem os instaladores e termos
desses projetos.

## Integridade dos downloads

O instalador busca a release do Dorion pela API oficial do GitHub e exige o
SHA-256 publicado no asset. Para o UrbanVPN, exige HTTPS no domínio oficial e
assinatura Authenticode válida da Urban Cyber Security Inc. Se qualquer
verificação falhar, o terceiro instalador não é executado.

Releases do Dorion GoLive incluem `SHA256SUMS.txt` e atestado de procedência do
GitHub Actions. Enquanto não houver assinatura Authenticode própria, o
SmartScreen pode mostrar um aviso e o Controle Inteligente de Aplicativos pode
bloquear o executável sem oferecer uma exceção. Mudar a aparência ou o formato
do instalador não cria reputação; isso exige assinatura de código confiável.
Não desative proteções do Windows apenas para testar e não contorne um alerta
se o hash não corresponder ao arquivo oficial.

## Relato de vulnerabilidade

Não publique senhas, tokens ou logs com dados pessoais em uma issue pública.
Use o recurso **Security advisories / Report a vulnerability** do repositório
quando ele estiver disponível.
