# Publicar uma release

## Verificação antes da tag

1. Atualize a versão em `Cargo.toml` e prepare as notas da release no GitHub.
2. Execute `cargo fmt --check`.
3. Execute `node --test tests/dorion-plugin.test.mjs`.
4. Execute `cargo test --locked`.
5. Execute `cargo clippy --all-targets --locked -- -D warnings`.
6. Compile com `cargo build --release --locked`.
7. Teste o instalador em três cenários: ambos os requisitos presentes, somente
   Dorion ausente e somente UrbanVPN ausente.

## Eventos do workflow

- **Pull request:** valida e compila, mas não publica.
- **Push em `main`:** valida, compila e guarda o instalador como artefato por
  14 dias, mas não cria release.
- **Execução manual:** faz a mesma validação de um push comum e não publica.
- **Tag `v*`:** executa todo o build e, somente se ele passar, inicia o job de
  publicação.

## Job `build`

O job usa um runner `windows-latest` com acesso somente de leitura ao
repositório. Ele:

1. instala Rust, Node.js e a versão disponível mais recente do Qt 6.8;
2. compila `src/urban-ipc/urban-ipc.cpp`;
3. executa formatação, teste do plugin, testes Rust e Clippy;
4. compila `dorion-golive.exe` em modo release;
5. em uma tag, confere se `vX.Y.Z` corresponde ao `Cargo.toml`;
6. cria `DorionGoLive-Setup.exe` e `SHA256SUMS.txt`;
7. envia os dois arquivos como artefato interno do GitHub Actions.

Esse job não instala Dorion ou UrbanVPN e não executa o instalador gráfico. Os
cenários de instalação precisam ser testados separadamente em máquinas virtuais
limpas.

## Job `release`

Esse job existe somente em tags `v*` e depende do sucesso do job `build`. Ele:

1. baixa exatamente o artefato aprovado pelo build;
2. gera um atestado de procedência para `DorionGoLive-Setup.exe`;
3. cria a release com notas automáticas;
4. anexa o instalador e `SHA256SUMS.txt`.

Somente esse job recebe permissão para gravar releases e emitir o atestado. Se
qualquer teste, compilação ou verificação de versão falhar, a release não é
criada.

Para publicar, crie e envie uma tag `vX.Y.Z` igual à versão do `Cargo.toml`.
