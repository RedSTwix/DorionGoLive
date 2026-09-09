# Requisitos

## Para instalar e usar

- Windows 10 ou 11, 64 bits;
- componentes nativos `curl.exe` e Windows PowerShell disponíveis em
  `%SystemRoot%\System32`;
- acesso HTTPS a `api.github.com`, `github.com` e
  `install.urban-vpn.com` para baixar requisitos ausentes;
- permissão para executar aplicativos baixados, aceitar o UAC quando necessário
  e alterar o autostart do usuário.

O instalador detecta Dorion e UrbanVPN. Se estiverem ausentes, baixa e instala
as versões oficiais automaticamente após uma confirmação inicial. Não são
requisitos para o usuário final: Rust, Node.js, Visual Studio, CMake ou Qt. O
auxiliar necessário já está embutido no executável da release.

O UrbanVPN pode solicitar a conclusão de sua configuração própria na primeira
utilização. O Dorion GoLive não aceita termos nem cria contas em nome do
usuário.

## Para compilar

- Rust estável, toolchain `x86_64-pc-windows-msvc`;
- Visual Studio Build Tools 2022, workload C++ e Windows SDK;
- CMake 3.24 ou posterior;
- Qt 6.8 para MSVC 2022 x64, módulos Core, Network e RemoteObjects;
- Node.js 20 ou posterior para executar o teste do plugin.

O aplicativo e a extensão de navegador do UrbanVPN são produtos diferentes;
este projeto requer o aplicativo de desktop.
