# Arquitetura

```text
Dorion/plugin JavaScript
        │ HTTP local 127.0.0.1:9352
        ▼
dorion-golive.exe (Rust)
        │ processo auxiliar + IPC local 127.0.0.1:19206
        ▼
urban-ipc.exe (C++/Qt) ──► serviço local do UrbanVPN
```

## Serviço Rust

O serviço instala os componentes, monitora o processo e os eventos nativos do
Dorion, mantém as gerações de cada janela temporária e confirma o estado do
adaptador `UrbanVPN`. O servidor HTTP escuta somente no loopback.

O módulo `instalador_ui.rs` implementa o assistente gráfico e mantém operações
longas fora da thread da interface. `instalador.rs` detecta dependências em
caminhos conhecidos, atalhos e registros de desinstalação, consulta a API
oficial do Dorion, relata o progresso, valida downloads e redetecta cada
componente após seu instalador terminar. A tela de conclusão depende também da
confirmação do plugin e da porta local do serviço. Nenhum script externo é
distribuído.

## Plugin JavaScript

O plugin captura as ações específicas de assistir, adicionar transmissão e
compartilhar tela. Ele não presume que um temporizador significa conexão:
aguarda a resposta confirmada do serviço, preserva a ação React original e
monitora quadros em avanço quando o usuário está assistindo.

## Auxiliar UrbanVPN

O UrbanVPN não oferece uma CLI pública usada por este projeto. O auxiliar Qt
se conecta ao serviço local de Remote Objects e solicita uma localização ou a
desconexão. A confirmação final não depende dos tipos privados do UrbanVPN; ela
é feita pelo estado operacional do adaptador no Windows.

O auxiliar compilado é embutido no executável Rust e extraído para
`%LOCALAPPDATA%\DorionGoLive` durante a instalação.
